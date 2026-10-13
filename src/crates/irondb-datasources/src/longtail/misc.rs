//! Misc SQL-family long-tail: wire-compat adapters over existing `sqlx`.
//!
//! mysql-wire (tidb native, goldendb mysql-compat, oceanbase/doris/starrocks
//! mysql-protocol, sphinx SphinxQL, duckdb analytical via mysql-side path is
//! NOT used — duckdb rides the sidecar, see `cn::SidecarAdapter`).
//! pg-wire (cockroach, timescale, mariadb-columnar via pg is NOT used —
//! mariadb rides the sidecar). No new crates.

use std::collections::HashMap;

use irondb_drivers::{ColumnDef, DatabaseDriver, DriverError, FakeRows};
use sqlx::{Column, Row, TypeInfo};

use super::cn::LONGTAIL_ROW_BUDGET;

/// Which existing sqlx pool backs this engine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WireKind {
    MySql,
    Postgres,
}

/// Generic wire-compat adapter: one struct, engine id + quirk note vary.
pub struct CompatAdapter {
    engine_id: &'static str,
    wire: WireKind,
    quirk: &'static str,
    mysql_pool: Option<sqlx::mysql::MySqlPool>,
    pg_pool: Option<sqlx::PgPool>,
}

impl CompatAdapter {
    pub fn new(engine_id: &'static str, wire: WireKind, quirk: &'static str) -> Self {
        Self { engine_id, wire, quirk, mysql_pool: None, pg_pool: None }
    }
    pub fn quirk(&self) -> &'static str {
        self.quirk
    }

    fn map_row(cols: &[String], get: &dyn Fn(usize) -> serde_json::Value) -> HashMap<String, serde_json::Value> {
        cols.iter().enumerate().map(|(i, c)| (c.clone(), get(i))).collect()
    }
}

#[async_trait::async_trait]
impl DatabaseDriver for CompatAdapter {
    async fn connect(&mut self, dsn: &str) -> Result<(), DriverError> {
        match self.wire {
            WireKind::MySql => {
                let pool = sqlx::mysql::MySqlPoolOptions::new()
                    .max_connections(5)
                    .connect(dsn)
                    .await
                    .map_err(|e| DriverError::Connect(super::cn::redact_secrets(&e.to_string())))?;
                self.mysql_pool = Some(pool);
            }
            WireKind::Postgres => {
                let pool = sqlx::postgres::PgPoolOptions::new()
                    .max_connections(5)
                    .connect(dsn)
                    .await
                    .map_err(|e| DriverError::Connect(super::cn::redact_secrets(&e.to_string())))?;
                self.pg_pool = Some(pool);
            }
        }
        Ok(())
    }

    async fn close(&mut self) -> Result<(), DriverError> {
        if let Some(p) = self.mysql_pool.take() {
            p.close().await;
        }
        if let Some(p) = self.pg_pool.take() {
            p.close().await;
        }
        Ok(())
    }

    async fn ping(&self) -> Result<(), DriverError> {
        match self.wire {
            WireKind::MySql => self.mysql_pool.as_ref().map(|_| ()).ok_or(DriverError::Closed),
            WireKind::Postgres => self.pg_pool.as_ref().map(|_| ()).ok_or(DriverError::Closed),
        }
    }

    async fn query(&self, sql: &str) -> Result<FakeRows, DriverError> {
        let mut out = FakeRows { columns: vec![], rows: vec![] };
        match self.wire {
            WireKind::MySql => {
                let pool = self.mysql_pool.as_ref().ok_or(DriverError::Closed)?;
                let rows = sqlx::raw_sql(sqlx::AssertSqlSafe(sql))
                    .fetch_all(pool)
                    .await
                    .map_err(|e| DriverError::Query(e.to_string()))?;
                if let Some(first) = rows.first() {
                    out.columns = first.columns().iter().map(|c| c.name().to_string()).collect();
                }
                for row in rows.into_iter().take(LONGTAIL_ROW_BUDGET) {
                    let cols = out.columns.clone();
                    let types: Vec<String> = row.columns().iter().map(|c| c.type_info().name().to_string()).collect();
                    out.rows.push(Self::map_row(&cols, &|i| match types[i].as_str() {
                        "INT" | "BIGINT" | "SMALLINT" | "TINYINT" | "MEDIUMINT" => {
                            row.try_get::<i64, _>(i).map(Into::into).unwrap_or(serde_json::Value::Null)
                        }
                        "BOOL" | "BOOLEAN" => {
                            row.try_get::<bool, _>(i).map(Into::into).unwrap_or(serde_json::Value::Null)
                        }
                        "FLOAT" | "DOUBLE" | "DECIMAL" => {
                            row.try_get::<f64, _>(i).map(Into::into).unwrap_or(serde_json::Value::Null)
                        }
                        _ => row.try_get::<String, _>(i).map(Into::into).unwrap_or(serde_json::Value::Null),
                    }));
                }
            }
            WireKind::Postgres => {
                let pool = self.pg_pool.as_ref().ok_or(DriverError::Closed)?;
                let rows = sqlx::raw_sql(sqlx::AssertSqlSafe(sql))
                    .fetch_all(pool)
                    .await
                    .map_err(|e| DriverError::Query(e.to_string()))?;
                if let Some(first) = rows.first() {
                    out.columns = first.columns().iter().map(|c| c.name().to_string()).collect();
                }
                for row in rows.into_iter().take(LONGTAIL_ROW_BUDGET) {
                    let cols = out.columns.clone();
                    out.rows.push(Self::map_row(&cols, &|i| {
                        row.try_get::<String, _>(i).map(Into::into).unwrap_or(serde_json::Value::Null)
                    }));
                }
            }
        }
        Ok(out)
    }

    async fn exec(&self, sql: &str) -> Result<i64, DriverError> {
        match self.wire {
            WireKind::MySql => {
                let pool = self.mysql_pool.as_ref().ok_or(DriverError::Closed)?;
                Ok(sqlx::raw_sql(sqlx::AssertSqlSafe(sql)).execute(pool).await
                    .map_err(|e| DriverError::Exec(e.to_string()))?.rows_affected() as i64)
            }
            WireKind::Postgres => {
                let pool = self.pg_pool.as_ref().ok_or(DriverError::Closed)?;
                Ok(sqlx::raw_sql(sqlx::AssertSqlSafe(sql)).execute(pool).await
                    .map_err(|e| DriverError::Exec(e.to_string()))?.rows_affected() as i64)
            }
        }
    }

    async fn get_databases(&self) -> Result<Vec<String>, DriverError> {
        let sql = match self.wire {
            WireKind::MySql => "SHOW DATABASES",
            WireKind::Postgres => "SELECT datname FROM pg_database WHERE datistemplate = false",
        };
        Ok(self.query(sql).await?.rows.iter()
            .filter_map(|r| r.values().next()?.as_str().map(str::to_string)).collect())
    }

    async fn get_tables(&self, db: &str) -> Result<Vec<String>, DriverError> {
        crate::quirks::check_ident(db)?;
        let sql = match self.wire {
            WireKind::MySql => format!("SHOW TABLES FROM `{db}`"),
            WireKind::Postgres => format!("SELECT tablename FROM pg_tables WHERE schemaname NOT IN ('pg_catalog','information_schema')"),
        };
        Ok(self.query(&sql).await?.rows.iter()
            .filter_map(|r| r.values().next()?.as_str().map(str::to_string)).collect())
    }

    async fn get_columns(&self, _db: &str, table: &str) -> Result<Vec<ColumnDef>, DriverError> {
        crate::quirks::check_ident(table)?;
        let sql = match self.wire {
            WireKind::MySql => format!("DESCRIBE `{table}`"),
            WireKind::Postgres => format!("SELECT column_name, data_type FROM information_schema.columns WHERE table_name = '{table}'"),
        };
        Ok(self.query(&sql).await?.rows.iter().filter_map(|r| {
            let vals: Vec<&serde_json::Value> = r.values().collect();
            Some(ColumnDef {
                name: vals.first()?.as_str()?.to_string(),
                data_type: vals.get(1)?.as_str().unwrap_or("").to_string(),
                nullable: true,
            })
        }).collect())
    }

    fn capabilities(&self) -> irondb_drivers::CapabilityFlags {
        irondb_drivers::CapabilityFlags::empty()
    }

    fn engine_id(&self) -> &'static str {
        self.engine_id
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compat_adapters_fail_closed_unconnected() {
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        rt.block_on(async {
            let a = CompatAdapter::new("tidb", WireKind::MySql, "mysql-compat");
            assert!(a.query("SELECT 1").await.is_err());
            assert_eq!(a.quirk(), "mysql-compat");
        });
    }
}
