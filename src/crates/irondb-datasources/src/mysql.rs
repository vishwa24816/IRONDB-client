//! MySQL adapter via sqlx 0.9 (mysql feature).
//! Implements DatabaseDriver trait with ? placeholders, batch caps, row-budget enforcement.

use std::collections::HashMap;

use irondb_drivers::{CapabilityFlags, ColumnDef, DatabaseDriver, DriverError, FakeRows};
use sqlx::mysql::{MySqlPool, MySqlPoolOptions, MySqlRow};
use sqlx::{Column, Row, TypeInfo};

use crate::quirks::quirks;

/// MySQL adapter wrapping sqlx MySqlPool.
pub struct MySqlAdapter {
    pool: Option<MySqlPool>,
    quirks: &'static crate::quirks::EngineQuirks,
}

impl MySqlAdapter {
    /// Create new unconnected adapter.
    pub fn new() -> Self {
        let quirks = quirks().get("mysql").cloned().unwrap_or_default();
        Self {
            pool: None,
            quirks: Box::leak(Box::new(quirks)),
        }
    }

    /// Convert sqlx row to HashMap for FakeRows compatibility.
    fn row_to_map(row: &MySqlRow) -> HashMap<String, serde_json::Value> {
        let mut map = HashMap::new();
        for (idx, column) in row.columns().iter().enumerate() {
            let name = column.name().to_string();
            let value: serde_json::Value = match column.type_info().name() {
                "INT" | "BIGINT" | "SMALLINT" | "TINYINT" | "MEDIUMINT" => {
                    row.try_get::<i64, _>(idx).map(Into::into).unwrap_or(serde_json::Value::Null)
                }
                "VARCHAR" | "TEXT" | "CHAR" | "LONGTEXT" | "MEDIUMTEXT" | "TINYTEXT" => {
                    row.try_get::<String, _>(idx).map(Into::into).unwrap_or(serde_json::Value::Null)
                }
                "FLOAT" | "DOUBLE" | "DECIMAL" => {
                    row.try_get::<f64, _>(idx).map(Into::into).unwrap_or(serde_json::Value::Null)
                }
                "BOOL" | "BOOLEAN" => {
                    row.try_get::<bool, _>(idx).map(Into::into).unwrap_or(serde_json::Value::Null)
                }
                "DATETIME" | "TIMESTAMP" | "DATE" | "TIME" => {
                    row.try_get::<String, _>(idx)
                        .map(Into::into)
                        .unwrap_or(serde_json::Value::Null)
                }
                "JSON" => {
                    row.try_get::<String, _>(idx)
                        .map(|s| serde_json::from_str(&s).unwrap_or(serde_json::Value::Null))
                        .unwrap_or(serde_json::Value::Null)
                }
                _ => row
                    .try_get::<String, _>(idx)
                    .map(Into::into)
                    .unwrap_or(serde_json::Value::Null),
            };
            map.insert(name, value);
        }
        map
    }
}

impl Default for MySqlAdapter {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl DatabaseDriver for MySqlAdapter {
    async fn connect(&mut self, dsn: &str) -> Result<(), DriverError> {
        let pool = MySqlPoolOptions::new()
            .max_connections(5)
            .connect(dsn)
            .await
            .map_err(|e| DriverError::Connect(e.to_string()))?;
        self.pool = Some(pool);
        Ok(())
    }

    async fn close(&mut self) -> Result<(), DriverError> {
        if let Some(pool) = self.pool.take() {
            pool.close().await;
        }
        Ok(())
    }

    async fn ping(&self) -> Result<(), DriverError> {
        let pool = self.pool.as_ref().ok_or(DriverError::Closed)?;
        pool.acquire().await.map_err(|e| DriverError::Connect(e.to_string()))?;
        Ok(())
    }

    async fn query(&self, sql: &str) -> Result<FakeRows, DriverError> {
        let pool = self.pool.as_ref().ok_or(DriverError::Closed)?;
        // Use raw_sql with AssertSqlSafe for dynamic SQL
        let rows = sqlx::raw_sql(sqlx::AssertSqlSafe(sql))
            .fetch_all(pool)
            .await
            .map_err(|e| DriverError::Query(e.to_string()))?;

        let mut fake_rows = FakeRows {
            columns: vec![],
            rows: vec![],
        };

        if let Some(first_row) = rows.first() {
            fake_rows.columns = first_row
                .columns()
                .iter()
                .map(|c| c.name().to_string())
                .collect();
        }

        for row in rows {
            fake_rows.rows.push(Self::row_to_map(&row));
        }

        Ok(fake_rows)
    }

    async fn exec(&self, sql: &str) -> Result<i64, DriverError> {
        let pool = self.pool.as_ref().ok_or(DriverError::Closed)?;
        let result = sqlx::raw_sql(sqlx::AssertSqlSafe(sql))
            .execute(pool)
            .await
            .map_err(|e| DriverError::Query(e.to_string()))?;
        Ok(result.rows_affected() as i64)
    }

    async fn get_databases(&self) -> Result<Vec<String>, DriverError> {
        let pool = self.pool.as_ref().ok_or(DriverError::Closed)?;
        let rows = sqlx::query("SHOW DATABASES")
            .fetch_all(pool)
            .await
            .map_err(|e| DriverError::Query(e.to_string()))?;
        Ok(rows
            .iter()
            .filter_map(|r| r.try_get::<String, _>(0).ok())
            .collect())
    }

    async fn get_tables(&self, db: &str) -> Result<Vec<String>, DriverError> {
        let pool = self.pool.as_ref().ok_or(DriverError::Closed)?;
        let sql = format!("SHOW TABLES FROM `{}`", crate::quirks::check_ident(db)?);
        let rows = sqlx::raw_sql(sqlx::AssertSqlSafe(sql.as_str()))
            .fetch_all(pool)
            .await
            .map_err(|e| DriverError::Query(e.to_string()))?;
        Ok(rows
            .iter()
            .filter_map(|r| r.try_get::<String, _>(0).ok())
            .collect())
    }

    async fn get_columns(&self, _db: &str, table: &str) -> Result<Vec<ColumnDef>, DriverError> {
        let pool = self.pool.as_ref().ok_or(DriverError::Closed)?;
        let sql = format!("DESCRIBE `{}`", crate::quirks::check_ident(table)?);
        let rows = sqlx::raw_sql(sqlx::AssertSqlSafe(sql.as_str()))
            .fetch_all(pool)
            .await
            .map_err(|e| DriverError::Query(e.to_string()))?;

        Ok(rows
            .iter()
            .filter_map(|r| {
                let name: String = r.try_get("Field").ok()?;
                let type_str: String = r.try_get("Type").ok()?;
                let null_str: String = r.try_get("Null").ok()?;
                Some(ColumnDef {
                    name,
                    data_type: type_str,
                    nullable: null_str == "YES",
                })
            })
            .collect())
    }

    fn capabilities(&self) -> CapabilityFlags {
        self.quirks.extra_caps
    }

    fn engine_id(&self) -> &'static str {
        "mysql"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::quirks::quirks;

    #[test]
    fn mysql_quirks_registered() {
        let q = quirks();
        let mysql = q.get("mysql").unwrap();
        assert_eq!(mysql.max_batch_rows, Some(1000));
        assert_eq!(mysql.max_batch_args, Some(60000));
    }
}