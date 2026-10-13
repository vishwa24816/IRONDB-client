//! irondb-store: SQLite app-state store behind a tokio-rusqlite async bridge.
//!
//! One long-lived [`Connection`] handle lives in the facade (Plan 03).
//! WAL sidecar files (`-wal`/`-shm`) are expected; run
//! [`checkpoint_on_shutdown`] on clean exit so backups are self-contained.

pub mod db;
pub mod errors;
pub mod models;
pub mod pragmas;

pub use db::{delete_connection, get_setting, list_connections, set_setting, upsert_connection};
pub use errors::StoreError;
pub use models::{SavedConnection, Setting};
use std::path::Path;
pub use tokio_rusqlite::Connection;

/// Open (or create) the app-state DB, applying WAL + busy_timeout + schema.
///
/// Never hold a `call()` guard across `.await`; closures materialize owned rows.
pub async fn open_store(path: &Path) -> Result<Connection, StoreError> {
    let conn = Connection::open(path)
        .await
        .map_err(|e| StoreError::Open(e.to_string()))?;
    conn.call(|c| {
        pragmas::apply_pragmas(c)?;
        pragmas::create_schema(c)?;
        Ok::<_, tokio_rusqlite::rusqlite::Error>(())
    })
    .await
    .map_err(|e| StoreError::Pragma(e.to_string()))?;
    Ok(conn)
}

/// Checkpoint WAL on clean shutdown.
pub async fn checkpoint_on_shutdown(conn: &Connection) -> Result<(), StoreError> {
    pragmas::checkpoint(conn).await
}
