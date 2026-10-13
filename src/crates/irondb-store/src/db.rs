use tokio_rusqlite::{rusqlite, Connection};

use crate::errors::StoreError;
use crate::models::{SavedConnection, Setting};

/// All SQL is fixed strings with bound params — never interpolate user input.
pub async fn upsert_connection(conn: &Connection, sc: SavedConnection) -> Result<(), StoreError> {
    conn.call(move |c| {
        c.execute(
            "INSERT INTO connections (id, engine, payload_json) VALUES (?1, ?2, ?3)
             ON CONFLICT(id) DO UPDATE SET engine=excluded.engine, payload_json=excluded.payload_json",
            rusqlite::params![sc.id, sc.engine, sc.payload_json],
        )?;
        Ok::<_, rusqlite::Error>(())
    })
    .await
    .map_err(|e| StoreError::Query(e.to_string()))
}

pub async fn list_connections(conn: &Connection) -> Result<Vec<SavedConnection>, StoreError> {
    conn.call(|c| {
        let mut stmt =
            c.prepare("SELECT id, engine, payload_json FROM connections ORDER BY id")?;
        let rows = stmt
            .query_map([], |row| {
                Ok(SavedConnection {
                    id: row.get(0)?,
                    engine: row.get(1)?,
                    payload_json: row.get(2)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok::<_, rusqlite::Error>(rows)
    })
    .await
    .map_err(|e| StoreError::Query(e.to_string()))
}

pub async fn delete_connection(conn: &Connection, id: String) -> Result<(), StoreError> {
    conn.call(move |c| {
        c.execute("DELETE FROM connections WHERE id = ?1", rusqlite::params![id])?;
        Ok::<_, rusqlite::Error>(())
    })
    .await
    .map_err(|e| StoreError::Query(e.to_string()))
}

pub async fn set_setting(conn: &Connection, key: String, value: String) -> Result<(), StoreError> {
    conn.call(move |c| {
        c.execute(
            "INSERT INTO settings (key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value=excluded.value",
            rusqlite::params![key, value],
        )?;
        Ok::<_, rusqlite::Error>(())
    })
    .await
    .map_err(|e| StoreError::Query(e.to_string()))
}

pub async fn get_setting(conn: &Connection, key: String) -> Result<Option<Setting>, StoreError> {
    conn.call(move |c| {
        let mut stmt = c.prepare("SELECT key, value FROM settings WHERE key = ?1")?;
        let mut rows = stmt.query(rusqlite::params![key])?;
        if let Some(row) = rows.next()? {
            Ok::<_, rusqlite::Error>(Some(Setting {
                key: row.get(0)?,
                value: row.get(1)?,
            }))
        } else {
            Ok(None)
        }
    })
    .await
    .map_err(|e| StoreError::Query(e.to_string()))
}
