use irondb_store::{list_connections, open_store, SavedConnection};
use tempfile::tempdir;

#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
async fn concurrent_writes_do_not_busy_fail() {
    let dir = tempdir().unwrap();
    let db_path = dir.path().join("app.db");
    let conn = open_store(&db_path).await.unwrap();

    // Spawn N writers through the single cloned handle. tokio-rusqlite serializes
    // calls onto its background thread; busy_timeout covers any residual lock.
    let mut tasks = Vec::new();
    for i in 0..32u32 {
        let h = conn.clone();
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
        t.await.unwrap().unwrap(); // zero SQLITE_BUSY tolerated: any Err fails the test
    }

    let conns = list_connections(&conn).await.unwrap();
    assert_eq!(conns.len(), 32);
    conn.close().await.unwrap();
}
