//! SQLite adapter — wraps irondb-store's tokio-rusqlite handle.
//! Single-writer discipline + ATTACH/DETACH ExternalAttachSpec support.

use std::collections::HashMap;
use std::sync::Arc;

use base64::Engine;
use irondb_drivers::{CapabilityFlags, ColumnDef, DatabaseDriver, DriverError, FakeRows};
use tokio_rusqlite::Connection;

use crate::quirks::quirks;

/// SQLite adapter wrapping a tokio-rusqlite Connection.
pub struct SqliteAdapter {
    conn: Option<Arc<Connection>>,
    quirks: &'static crate::quirks::EngineQuirks,
    /// Path for ATTACH/DETACH external databases
    attached_dbs: std::sync::Mutex<HashMap<String, String>>, // alias -> path
}

impl SqliteAdapter {
    /// Create new unconnected adapter.
    pub fn new() -> Self {
        let quirks = quirks().get("sqlite").cloned().unwrap_or_default();
        Self {
            conn: None,
            quirks: Box::leak(Box::new(quirks)),
            attached_dbs: std::sync::Mutex::new(HashMap::new()),
        }
    }

    /// ATTACH external database per ExternalAttachSpec.
    pub async fn attach_external(&self, alias: &str, path: &str) -> Result<(), DriverError> {
        let conn = self.conn.as_ref().ok_or(DriverError::Closed)?;
        crate::quirks::check_ident(alias)?;
        if path.contains('\'') {
            return Err(DriverError::Query("invalid path".to_string()));
        }
        let sql = format!("ATTACH DATABASE '{}' AS {}", path, alias);
        let sql = sql.clone();
        conn.call(move |c| c.execute_batch(&sql))
            .await
            .map_err(|e| DriverError::Query(e.to_string()))?;
        
        self.attached_dbs.lock().unwrap().insert(alias.to_string(), path.to_string());
        Ok(())
    }

    /// DETACH external database.
    pub async fn detach_external(&self, alias: &str) -> Result<(), DriverError> {
        let conn = self.conn.as_ref().ok_or(DriverError::Closed)?;
        let sql = format!("DETACH DATABASE {}", crate::quirks::check_ident(alias)?);
        let sql = sql.clone();
        conn.call(move |c| c.execute_batch(&sql))
            .await
            .map_err(|e| DriverError::Query(e.to_string()))?;
        
        self.attached_dbs.lock().unwrap().remove(alias);
        Ok(())
    }

    /// Query rows and return (column_names, rows).
    async fn query_rows(&self, sql: &str) -> Result<(Vec<String>, Vec<HashMap<String, serde_json::Value>>), DriverError> {
        let conn = self.conn.as_ref().ok_or(DriverError::Closed)?;
        let conn = conn.clone();
        let sql = sql.to_string();
        
        conn.call(move |c| {
            let mut stmt = c.prepare(&sql)?;
            let col_names: Vec<String> = stmt.column_names().iter().map(|s| s.to_string()).collect();
            let rows = stmt.query_map([], |row| {
                let mut map = HashMap::new();
                for (idx, name) in col_names.iter().enumerate() {
                    let value: serde_json::Value = match row.get_ref(idx)? {
                        tokio_rusqlite::rusqlite::types::ValueRef::Null => serde_json::Value::Null,
                        tokio_rusqlite::rusqlite::types::ValueRef::Integer(i) => (i as i64).into(),
                        tokio_rusqlite::rusqlite::types::ValueRef::Real(f) => f.into(),
                        tokio_rusqlite::rusqlite::types::ValueRef::Text(t) => {
                            std::str::from_utf8(t).unwrap_or("").to_string().into()
                        }
                        tokio_rusqlite::rusqlite::types::ValueRef::Blob(b) => {
                            base64::engine::general_purpose::STANDARD.encode(b).into()
                        }
                    };
                    map.insert(name.clone(), value);
                }
                Ok(map)
            })?.collect::<Result<Vec<_>, _>>()?;
            Ok::<_, tokio_rusqlite::rusqlite::Error>((col_names, rows))
        }).await.map_err(|e| DriverError::Query(e.to_string()))
    }
}

impl Default for SqliteAdapter {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl DatabaseDriver for SqliteAdapter {
    async fn connect(&mut self, dsn: &str) -> Result<(), DriverError> {
        let path = dsn.strip_prefix("sqlite://").unwrap_or(dsn);
        let conn = Connection::open(path).await
            .map_err(|e| DriverError::Connect(e.to_string()))?;
        
        // Enable WAL mode and busy timeout for concurrent access
        conn.call(|c| {
            c.execute_batch("PRAGMA journal_mode = WAL; PRAGMA busy_timeout = 5000;")
        }).await.map_err(|e| DriverError::Query(e.to_string()))?;

        self.conn = Some(Arc::new(conn));
        Ok(())
    }

    async fn close(&mut self) -> Result<(), DriverError> {
        self.conn = None;
        Ok(())
    }

    async fn ping(&self) -> Result<(), DriverError> {
        let conn = self.conn.as_ref().ok_or(DriverError::Closed)?;
        conn.call(|c| c.execute_batch("SELECT 1"))
            .await
            .map_err(|e| DriverError::Connect(e.to_string()))?;
        Ok(())
    }

    async fn query(&self, sql: &str) -> Result<FakeRows, DriverError> {
        let (columns, rows) = self.query_rows(sql).await?;
        Ok(FakeRows { columns, rows })
    }

    async fn exec(&self, sql: &str) -> Result<i64, DriverError> {
        let conn = self.conn.as_ref().ok_or(DriverError::Closed)?;
        let conn = conn.clone();
        let sql = sql.to_string();
        
        let changes = conn.call(move |c| {
            c.execute_batch(&sql)?;
            Ok::<u64, tokio_rusqlite::rusqlite::Error>(c.changes())
        }).await.map_err(|e| DriverError::Query(e.to_string()))?;

        Ok(changes as i64)
    }

    async fn get_databases(&self) -> Result<Vec<String>, DriverError> {
        // SQLite doesn't have multiple databases in the same sense
        // Return "main" plus attached databases
        let mut dbs = vec!["main".to_string()];
        for alias in self.attached_dbs.lock().unwrap().keys() {
            dbs.push(alias.clone());
        }
        Ok(dbs)
    }

    async fn get_tables(&self, db: &str) -> Result<Vec<String>, DriverError> {
        let conn = self.conn.as_ref().ok_or(DriverError::Closed)?;
        let conn = conn.clone();
        // Validate before the blocking call (T-04-07: identifiers never interpolated raw).
        let db = crate::quirks::check_ident(db)?.to_string();

        let tables = conn.call(move |c| {
            let sql = if db == "main" {
                "SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%'".to_string()
            } else {
                format!("SELECT name FROM {db}.sqlite_master WHERE type = 'table'")
            };
            let mut stmt = c.prepare(&sql)?;
            let rows = stmt.query_map([], |row| row.get::<_, String>(0))?
                .collect::<Result<Vec<_>, _>>()?;
            Ok::<_, tokio_rusqlite::rusqlite::Error>(rows)
        }).await.map_err(|e| DriverError::Query(e.to_string()))?;

        Ok(tables)
    }

    async fn get_columns(&self, db: &str, table: &str) -> Result<Vec<ColumnDef>, DriverError> {
        let conn = self.conn.as_ref().ok_or(DriverError::Closed)?;
        let conn = conn.clone();
        let db = crate::quirks::check_ident(db)?.to_string();
        let table = crate::quirks::check_ident(table)?.to_string();
        
        let columns = conn.call(move |c| {
            let sql = if db == "main" {
                format!("PRAGMA table_info({})", table)
            } else {
                format!("PRAGMA {}.table_info({})", db, table)
            };
            let mut stmt = c.prepare(&sql)?;
            let rows = stmt.query_map([], |row| {
                Ok(ColumnDef {
                    name: row.get::<_, String>(1)?,
                    data_type: row.get::<_, String>(2)?,
                    nullable: row.get::<_, i32>(3)? == 0, // notnull=0 means nullable
                })
            })?.collect::<Result<Vec<_>, _>>()?;
            Ok::<_, tokio_rusqlite::rusqlite::Error>(rows)
        }).await.map_err(|e| DriverError::Query(e.to_string()))?;

        Ok(columns)
    }

    fn capabilities(&self) -> CapabilityFlags {
        CapabilityFlags::IN_MEMORY | self.quirks.extra_caps
    }

    fn engine_id(&self) -> &'static str {
        "sqlite"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::quirks::quirks;

    #[test]
    fn sqlite_quirks_registered() {
        let q = quirks();
        let sqlite = q.get("sqlite").unwrap();
        assert!(sqlite.sqlite_single_writer);
        assert!(sqlite.sqlite_attach_external);
    }

    /// Per-connection handles allow concurrent readers on the same file (OQ1).
    #[tokio::test]
    async fn sqlite_concurrent_handles() {
        use irondb_drivers::DatabaseDriver;
        let dir = std::env::temp_dir().join("irondb-sqlite-conc-test.db");
        let dsn = format!("sqlite://{}", dir.to_string_lossy());
        let mut a = SqliteAdapter::new();
        let mut b = SqliteAdapter::new();
        a.connect(&dsn).await.unwrap();
        b.connect(&dsn).await.unwrap();
        a.exec("CREATE TABLE IF NOT EXISTS t (id INTEGER)").await.unwrap();
        let (r1, r2) = tokio::join!(a.query("SELECT * FROM t"), b.query("SELECT * FROM t"));
        assert!(r1.is_ok() && r2.is_ok());
        a.close().await.unwrap();
        b.close().await.unwrap();
        let _ = std::fs::remove_file(&dir);
        let _ = std::fs::remove_file(dir.with_extension("db-wal"));
        let _ = std::fs::remove_file(dir.with_extension("db-shm"));
    }
}