//! HEAD-03: concurrent UI + HTTP access against one DB file serializes
//! with no user-visible SQLITE_BUSY. Two store handles (UI-sim + HTTP-sim)
//! share one temp file via WAL + busy_timeout 5s (reuses `open_store`, never
//! hand-rolls pragmas); two `AppFacade` opens do the same at facade level.
//! Same-process `Arc` sharing is asserted via clones of one facade.

use irondb_store::{open_store, SavedConnection};

fn assert_no_busy(err: &str) {
    let lower = err.to_ascii_lowercase();
    assert!(
        !lower.contains("busy") && !lower.contains("locked"),
        "lock error surfaced to user: {err}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
async fn two_handles_one_file_serialize_writes() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("app.db");

    // UI-sim + HTTP-sim handles on the SAME file.
    let ui = open_store(&db_path).await.unwrap();
    let http = open_store(&db_path).await.unwrap();

    // Confirm WAL mode on the shared file (busy_timeout retry inside is fine;
    // a user-visible SQLITE_BUSY/database-is-locked is failure).
    let mode: String = ui
        .call(|c| {
            let m: String = c.query_row("PRAGMA journal_mode;", [], |r| r.get(0))?;
            Ok::<_, tokio_rusqlite::rusqlite::Error>(m)
        })
        .await
        .unwrap();
    assert_eq!(mode.to_ascii_lowercase(), "wal");
    let timeout_ms: i64 = http
        .call(|c| {
            let t: i64 = c.query_row("PRAGMA busy_timeout;", [], |r| r.get(0))?;
            Ok::<_, tokio_rusqlite::rusqlite::Error>(t)
        })
        .await
        .unwrap();
    assert_eq!(timeout_ms, 5000);

    // Concurrent write+write across both handles.
    let mut tasks = Vec::new();
    for i in 0..32u32 {
        let h = if i % 2 == 0 { ui.clone() } else { http.clone() };
        tasks.push(tokio::spawn(async move {
            irondb_store::upsert_connection(
                &h,
                SavedConnection {
                    id: format!("c{i}"),
                    engine: "fake".into(),
                    payload_json: r#"{}"#.into(),
                },
            )
            .await
        }));
    }
    for t in tasks {
        if let Err(e) = t.await.unwrap() {
            assert_no_busy(&e.to_string());
            panic!("write failed: {e}");
        }
    }

    // Concurrent read+write across both handles.
    let mut tasks = Vec::new();
    for i in 32..48u32 {
        let h = if i % 2 == 0 { ui.clone() } else { http.clone() };
        tasks.push(tokio::spawn(async move {
            if i % 3 == 0 {
                irondb_store::list_connections(&h).await.map(|_| ()).map_err(|e| e.to_string())
            } else {
                irondb_store::upsert_connection(
                    &h,
                    SavedConnection {
                        id: format!("c{i}"),
                        engine: "fake".into(),
                        payload_json: r#"{}"#.into(),
                    },
                )
                .await
                .map_err(|e| e.to_string())
            }
        }));
    }
    for t in tasks {
        if let Err(e) = t.await.unwrap() {
            assert_no_busy(&e);
            panic!("read/write batch failed: {e}");
        }
    }

    let conns = irondb_store::list_connections(&ui).await.unwrap();
    assert!(conns.len() >= 32, "expected serialized writes, got {}", conns.len());
    ui.close().await.unwrap();
    http.close().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
async fn two_facades_one_file_plus_arc_sharing() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("app.db");

    // Two facade opens (UI-sim + HTTP-sim) on the same file + Arc clones.
    let ui = irondb_facade::AppFacade::open(&db_path).await.unwrap();
    let http = irondb_facade::AppFacade::open(&db_path).await.unwrap();
    let shared = ui.clone();
    assert!(std::sync::Arc::ptr_eq(&shared, &ui));

    ui.connect("t", "fake", "").await.unwrap();
    http.connect("t", "fake", "").await.unwrap();

    let mut tasks = Vec::new();
    for i in 0..16u32 {
        let f = if i % 2 == 0 { ui.clone() } else { http.clone() };
        tasks.push(tokio::spawn(async move {
            f.execute_query("t", "SELECT 1", 100, 5000, &format!("q{i}")).await
        }));
    }
    for t in tasks {
        let res = t.await.unwrap();
        assert!(res.success, "concurrent query failed: {}", res.message);
    }

    // Drain-then-teardown: cancel in-flight (none left) + checkpoint works.
    ui.shutdown().await.unwrap();
    http.shutdown().await.unwrap();
}
