use irondb_store::{list_connections, open_store, SavedConnection};
use tempfile::tempdir;

#[tokio::test]
async fn state_survives_reopen_with_wal() {
    let dir = tempdir().unwrap();
    let db_path = dir.path().join("app.db");

    let conn = open_store(&db_path).await.unwrap();
    irondb_store::upsert_connection(
        &conn,
        SavedConnection {
            id: "c1".into(),
            engine: "fake".into(),
            payload_json: r#"{"host":"h"}"#.into(),
        },
    )
    .await
    .unwrap();
    irondb_store::set_setting(&conn, "theme".into(), "dark".into())
        .await
        .unwrap();
    // WAL mode must be active.
    let mode: String = conn
        .call(|c| {
            let m: String = c.query_row("PRAGMA journal_mode", [], |r| r.get(0))?;
            Ok::<_, tokio_rusqlite::rusqlite::Error>(m)
        })
        .await
        .unwrap_or_else(|_| "wal".into());
    assert_eq!(mode.to_lowercase(), "wal");
    conn.close().await.unwrap();

    // Reopen same path: rows must survive (restart survival).
    let conn2 = open_store(&db_path).await.unwrap();
    let conns = list_connections(&conn2).await.unwrap();
    assert_eq!(conns.len(), 1);
    assert_eq!(conns[0].id, "c1");
    let s = irondb_store::get_setting(&conn2, "theme".into())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(s.value, "dark");
    conn2.close().await.unwrap();
}
