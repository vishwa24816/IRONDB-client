//! PostgreSQL adapter via sqlx 0.9 (postgres feature).
//! Implements DatabaseDriver trait with $n placeholders, LISTEN/NOTIFY support.

use std::collections::HashMap;

use irondb_drivers::{CapabilityFlags, ColumnDef, DatabaseDriver, DriverError, FakeRows};
use sqlx::postgres::{PgPool, PgPoolOptions, PgRow};
use sqlx::{Column, Row, TypeInfo};

use crate::quirks::quirks;

/// PostgreSQL adapter wrapping sqlx PgPool.
pub struct PostgresAdapter {
    pool: Option<PgPool>,
    quirks: &'static crate::quirks::EngineQuirks,
}

impl PostgresAdapter {
    /// Create new unconnected adapter.
    pub fn new() -> Self {
        let quirks = quirks().get("postgres").cloned().unwrap_or_default();
        Self {
            pool: None,
            quirks: Box::leak(Box::new(quirks)),
        }
    }

    /// Convert sqlx row to HashMap.
    fn row_to_map(row: &PgRow) -> HashMap<String, serde_json::Value> {
        let mut map = HashMap::new();
        for (idx, column) in row.columns().iter().enumerate() {
            let name = column.name().to_string();
            let value: serde_json::Value = match column.type_info().name() {
                "INT2" | "INT4" | "INT8" => {
                    row.try_get::<i64, _>(idx).map(Into::into).unwrap_or(serde_json::Value::Null)
                }
                "VARCHAR" | "TEXT" | "CHAR" | "BPCHAR" | "NAME" => {
                    row.try_get::<String, _>(idx).map(Into::into).unwrap_or(serde_json::Value::Null)
                }
                "FLOAT4" | "FLOAT8" | "NUMERIC" => {
                    row.try_get::<f64, _>(idx).map(Into::into).unwrap_or(serde_json::Value::Null)
                }
                "BOOL" => {
                    row.try_get::<bool, _>(idx).map(Into::into).unwrap_or(serde_json::Value::Null)
                }
                "TIMESTAMP" | "TIMESTAMPTZ" | "DATE" | "TIME" | "TIMETZ" => {
                    row.try_get::<String, _>(idx)
                        .map(Into::into)
                        .unwrap_or(serde_json::Value::Null)
                }
                "JSON" | "JSONB" => {
                    row.try_get::<String, _>(idx)
                        .map(|s| serde_json::from_str(&s).unwrap_or(serde_json::Value::Null))
                        .unwrap_or(serde_json::Value::Null)
                }
                "UUID" => {
                    row.try_get::<String, _>(idx)
                        .map(Into::into)
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

impl Default for PostgresAdapter {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl DatabaseDriver for PostgresAdapter {
    async fn connect(&mut self, dsn: &str) -> Result<(), DriverError> {
        let pool = PgPoolOptions::new()
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
        let rows = sqlx::query("SELECT datname FROM pg_database WHERE datistemplate = false")
            .fetch_all(pool)
            .await
            .map_err(|e| DriverError::Query(e.to_string()))?;
        Ok(rows
            .iter()
            .filter_map(|r| r.try_get::<String, _>("datname").ok())
            .collect())
    }

    async fn get_tables(&self, _db: &str) -> Result<Vec<String>, DriverError> {
        let pool = self.pool.as_ref().ok_or(DriverError::Closed)?;
        let rows = sqlx::query(
            "SELECT tablename FROM pg_tables WHERE schemaname = 'public'",
        )
        .fetch_all(pool)
        .await
        .map_err(|e| DriverError::Query(e.to_string()))?;
        Ok(rows
            .iter()
            .filter_map(|r| r.try_get::<String, _>("tablename").ok())
            .collect())
    }

    async fn get_columns(&self, _db: &str, table: &str) -> Result<Vec<ColumnDef>, DriverError> {
        let pool = self.pool.as_ref().ok_or(DriverError::Closed)?;
        let sql = r#"
            SELECT column_name, data_type, is_nullable
            FROM information_schema.columns
            WHERE table_name = $1 AND table_schema = 'public'
            ORDER BY ordinal_position
        "#;
        let rows = sqlx::query(sql)
            .bind(table)
            .fetch_all(pool)
            .await
            .map_err(|e| DriverError::Query(e.to_string()))?;

        Ok(rows
            .iter()
            .filter_map(|r| {
                let name: String = r.try_get("column_name").ok()?;
                let type_str: String = r.try_get("data_type").ok()?;
                let nullable: String = r.try_get("is_nullable").ok()?;
                Some(ColumnDef {
                    name,
                    data_type: type_str,
                    nullable: nullable == "YES",
                })
            })
            .collect())
    }

    fn capabilities(&self) -> CapabilityFlags {
        self.quirks.extra_caps
    }

    fn engine_id(&self) -> &'static str {
        "postgres"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::quirks::quirks;

    #[test]
    fn postgres_quirks_registered() {
        let q = quirks();
        let pg = q.get("postgres").unwrap();
        assert_eq!(pg.placeholder_style, crate::quirks::PlaceholderStyle::DollarNumber);
        assert!(pg.supports_listen_notify);
    }
}