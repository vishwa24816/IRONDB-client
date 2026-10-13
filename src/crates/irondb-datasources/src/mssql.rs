//! MSSQL adapter via tiberius 0.13 (TDS driver).
//! Implements DatabaseDriver trait with @p placeholders, sqlServerAnalyzeMaxRows budget.

use std::collections::HashMap;

use irondb_drivers::{CapabilityFlags, ColumnDef, DatabaseDriver, DriverError, FakeRows};
use tiberius::{Client, Config, AuthMethod, EncryptionLevel};
use tokio::net::TcpStream;
use tokio_util::compat::TokioAsyncWriteCompatExt;

use crate::quirks::quirks;

/// MSSQL adapter wrapping tiberius Client.
pub struct MssqlAdapter {
    config: Option<Config>,
    host: Option<String>,
    port: Option<u16>,
    quirks: &'static crate::quirks::EngineQuirks,
}

impl MssqlAdapter {
    /// Create new unconnected adapter.
    pub fn new() -> Self {
        let quirks = quirks().get("sqlserver").cloned().unwrap_or_default();
        Self {
            config: None,
            host: None,
            port: None,
            quirks: Box::leak(Box::new(quirks)),
        }
    }

    /// Create a new client connection.
    async fn connect_client(&self) -> Result<Client<tokio_util::compat::Compat<TcpStream>>, DriverError> {
        let config = self.config.as_ref().ok_or(DriverError::Closed)?;
        let host = self.host.as_ref().ok_or(DriverError::Closed)?;
        let port = self.port.ok_or(DriverError::Closed)?;
        let tcp = TcpStream::connect((host.as_str(), port))
            .await
            .map_err(|e| DriverError::Connect(e.to_string()))?;

        let client = Client::connect(config.clone(), tcp.compat_write())
            .await
            .map_err(|e| DriverError::Connect(e.to_string()))?;

        Ok(client)
    }

    /// Convert tiberius Row to HashMap.
    fn row_to_map(row: &tiberius::Row) -> HashMap<String, serde_json::Value> {
        let mut map = HashMap::new();
        for (idx, col) in row.columns().iter().enumerate() {
            let name = col.name().to_string();
            let value: serde_json::Value = if let Ok(Some(v)) = row.try_get::<&str, _>(idx) {
                v.to_string().into()
            } else if let Ok(Some(v)) = row.try_get::<i32, _>(idx) {
                (v as i64).into()
            } else if let Ok(Some(v)) = row.try_get::<i64, _>(idx) {
                v.into()
            } else if let Ok(Some(v)) = row.try_get::<f64, _>(idx) {
                v.into()
            } else if let Ok(Some(v)) = row.try_get::<bool, _>(idx) {
                v.into()
            } else {
                serde_json::Value::Null
            };
            map.insert(name, value);
        }
        map
    }
}

impl Default for MssqlAdapter {
    fn default() -> Self {
        Self::new()
    }
}

/// Parse DSN into tiberius Config.
fn parse_dsn(dsn: &str) -> Result<(Config, String, u16), DriverError> {
    // Simple DSN parsing - in production use a proper URL parser
    let dsn = dsn.strip_prefix("sqlserver://").unwrap_or(dsn);
    let mut config = Config::new();

    // Parse host, port, user, password, database
    // Format: user:pass@host:port/db?encrypt=true&trustServerCertificate=true
    let (auth_part, rest) = dsn.split_once('@').ok_or_else(|| DriverError::Connect("Invalid DSN: missing @".to_string()))?;
    let (user, pass) = auth_part.split_once(':').ok_or_else(|| DriverError::Connect("Invalid DSN: missing password".to_string()))?;

    let (host_port_db, _params) = rest.split_once('?').unwrap_or((rest, ""));
    let (host_port, db) = host_port_db.split_once('/').ok_or_else(|| DriverError::Connect("Invalid DSN: missing database".to_string()))?;
    let (host, port) = host_port.split_once(':').ok_or_else(|| DriverError::Connect("Invalid DSN: missing port".to_string()))?;

    let port_num: u16 = port.parse().map_err(|_| DriverError::Connect("Invalid port".to_string()))?;

    config.host(host);
    config.port(port_num);
    config.authentication(AuthMethod::sql_server(user, pass));
    config.database(db);
    config.trust_cert(); // For dev; production should use proper certs
    config.encryption(EncryptionLevel::Required);

    Ok((config, host.to_string(), port_num))
}

#[async_trait::async_trait]
impl DatabaseDriver for MssqlAdapter {
    async fn connect(&mut self, dsn: &str) -> Result<(), DriverError> {
        let (config, host, port) = parse_dsn(dsn)?;
        self.config = Some(config);
        self.host = Some(host);
        self.port = Some(port);
        // Test connection
        let _client = self.connect_client().await?;
        Ok(())
    }

    async fn close(&mut self) -> Result<(), DriverError> {
        self.config = None;
        self.host = None;
        self.port = None;
        Ok(())
    }

    async fn ping(&self) -> Result<(), DriverError> {
        let mut client = self.connect_client().await?;
        client.simple_query("SELECT 1").await.map_err(|e| DriverError::Connect(e.to_string()))?;
        Ok(())
    }

    async fn query(&self, sql: &str) -> Result<FakeRows, DriverError> {
        let mut client = self.connect_client().await?;

        let stream = client.query(sql, &[]).await.map_err(|e| DriverError::Query(e.to_string()))?;
        let results: Vec<Vec<tiberius::Row>> = stream.into_results().await.map_err(|e| DriverError::Query(e.to_string()))?;

        let mut fake_rows = FakeRows {
            columns: vec![],
            rows: vec![],
        };

        // Use first result set
        if let Some(first_result) = results.first() {
            if let Some(first_row) = first_result.first() {
                fake_rows.columns = first_row.columns().iter().map(|c| c.name().to_string()).collect();
            }
            for row in first_result {
                fake_rows.rows.push(Self::row_to_map(row));
            }
        }

        Ok(fake_rows)
    }

    async fn exec(&self, sql: &str) -> Result<i64, DriverError> {
        let mut client = self.connect_client().await?;
        let result = client.execute(sql, &[]).await.map_err(|e| DriverError::Query(e.to_string()))?;
        Ok(result.total() as i64)
    }

    async fn get_databases(&self) -> Result<Vec<String>, DriverError> {
        let mut client = self.connect_client().await?;
        let stream = client.query("SELECT name FROM sys.databases WHERE database_id > 4", &[]).await
            .map_err(|e| DriverError::Query(e.to_string()))?;
        let results = stream.into_results().await.map_err(|e| DriverError::Query(e.to_string()))?;

        let mut dbs = Vec::new();
        for result in results {
            for row in result {
                if let Ok(Some(name)) = row.try_get::<&str, _>("name") {
                    dbs.push(name.to_string());
                }
            }
        }
        Ok(dbs)
    }

    async fn get_tables(&self, _db: &str) -> Result<Vec<String>, DriverError> {
        let mut client = self.connect_client().await?;
        let stream = client.query("SELECT TABLE_NAME FROM INFORMATION_SCHEMA.TABLES WHERE TABLE_TYPE = 'BASE TABLE'", &[]).await
            .map_err(|e| DriverError::Query(e.to_string()))?;
        let results = stream.into_results().await.map_err(|e| DriverError::Query(e.to_string()))?;

        let mut tables = Vec::new();
        for result in results {
            for row in result {
                if let Ok(Some(name)) = row.try_get::<&str, _>("TABLE_NAME") {
                    tables.push(name.to_string());
                }
            }
        }
        Ok(tables)
    }

    async fn get_columns(&self, _db: &str, table: &str) -> Result<Vec<ColumnDef>, DriverError> {
        let mut client = self.connect_client().await?;
        let sql = format!(
            "SELECT COLUMN_NAME, DATA_TYPE, IS_NULLABLE FROM INFORMATION_SCHEMA.COLUMNS WHERE TABLE_NAME = '{}'",
            crate::quirks::check_ident(table)?
        );
        let stream = client.query(&sql, &[]).await.map_err(|e| DriverError::Query(e.to_string()))?;
        let results = stream.into_results().await.map_err(|e| DriverError::Query(e.to_string()))?;

        let mut columns = Vec::new();
        for result in results {
            for row in result {
                let name = row.try_get::<&str, _>("COLUMN_NAME").ok().flatten()
                    .ok_or_else(|| DriverError::Query("Missing COLUMN_NAME".to_string()))?;
                let type_str = row.try_get::<&str, _>("DATA_TYPE").ok().flatten()
                    .ok_or_else(|| DriverError::Query("Missing DATA_TYPE".to_string()))?;
                let nullable = row.try_get::<&str, _>("IS_NULLABLE").ok().flatten()
                    .ok_or_else(|| DriverError::Query("Missing IS_NULLABLE".to_string()))?;
                columns.push(ColumnDef {
                    name: name.to_string(),
                    data_type: type_str.to_string(),
                    nullable: nullable == "YES",
                });
            }
        }
        Ok(columns)
    }

    fn capabilities(&self) -> CapabilityFlags {
        self.quirks.extra_caps
    }

    fn engine_id(&self) -> &'static str {
        "sqlserver"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::quirks::quirks;

    #[test]
    fn mssql_quirks_registered() {
        let q = quirks();
        let mssql = q.get("sqlserver").unwrap();
        assert_eq!(mssql.mssql_analyze_max_rows, Some(100_000));
        assert_eq!(mssql.placeholder_style, crate::quirks::PlaceholderStyle::AtSymbol);
    }
}