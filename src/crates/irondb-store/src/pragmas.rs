use tokio_rusqlite::{rusqlite, Connection};

use crate::errors::StoreError;

/// Apply WAL + busy_timeout + synchronous pragmas. WAL sidecars (-wal/-shm) are expected.
pub fn apply_pragmas(conn: &mut rusqlite::Connection) -> Result<(), rusqlite::Error> {
    conn.execute_batch(
        "PRAGMA journal_mode=WAL; PRAGMA busy_timeout=5000; PRAGMA synchronous=NORMAL;",
    )
}

/// Create app-state schema (idempotent).
pub fn create_schema(conn: &rusqlite::Connection) -> Result<(), rusqlite::Error> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS connections (
            id TEXT PRIMARY KEY,
            engine TEXT NOT NULL,
            payload_json TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS settings (
            key TEXT PRIMARY KEY,
            value TEXT NOT NULL
        );",
    )
}

/// Run a WAL checkpoint on clean shutdown so a file copy/backup is self-contained.
pub async fn checkpoint(conn: &Connection) -> Result<(), StoreError> {
    conn.call(|c| {
        c.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")?;
        Ok::<_, rusqlite::Error>(())
    })
    .await
    .map_err(|e| StoreError::Query(e.to_string()))
}
