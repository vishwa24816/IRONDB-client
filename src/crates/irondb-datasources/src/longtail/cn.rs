//! Long-tail engine adapters (DRV-04) — CN + misc SQL family.
//!
//! Vehicle resolution (human-approved 2026-10-13): `odbc-api` 29.x is the ONLY
//! new crate. Everything else reuses existing `sqlx` (pg/mysql wire compat),
//! `reqwest` (HTTP-native), or the Phase-4 `irondb-sidecar` harness (Go agent
//! reuse). NO qdrant-client / meilisearch-sdk / typesense / rdkafka.
//!
//! Mirrors Go original: `gbase8s_odbc_conn.go` (ODBC bridge), pg/mysql-compat
//! CN engines, and the Phase-4 quirk matrix.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use irondb_drivers::{ColumnDef, DatabaseDriver, DriverError, FakeRows};
use irondb_sidecar::{AgentRequest, RowBudgetOptions, SidecarClient};
use odbc_api::{ConnectionOptions, Cursor, ResultSetMetadata};

/// Max rows enforced on every long-tail read path (row-budget parity).
pub const LONGTAIL_ROW_BUDGET: usize = 5000;
/// Default sidecar call timeout.
pub const SIDECAR_TIMEOUT: Duration = Duration::from_secs(30);

fn empty_rows() -> FakeRows {
    FakeRows { columns: vec![], rows: vec![] }
}

/// Redact `password`/`token`/`secret` substrings from anything logged or
/// persisted (CDC positions, DSNs, sidecar errors). T-06-10 mitigation.
pub fn redact_secrets(s: &str) -> String {
    let mut out = s.to_string();
    // URI userinfo form: scheme://user:pass@host -> scheme://user:***@host
    for marker in ["://"] {
        let mut start = 0;
        while let Some(i) = out[start..].find(marker) {
            let abs = start + i + marker.len();
            if let Some(at) = out[abs..].find('@') {
                let host = abs + at;
                if let Some(colon) = out[abs..host].find(':') {
                    let pass_at = abs + colon + 1;
                    out.replace_range(pass_at..host, "***");
                    start = pass_at + 3;
                    continue;
                }
            }
            start = abs;
            if start >= out.len() {
                break;
            }
        }
    }
    for key in ["password", "passwd", "pwd", "token", "secret"] {
        let mut start = 0;
        while let Some(i) = out[start..].to_lowercase().find(key) {
            let abs = start + i + key.len();
            // blank the following `=value` / `:value` run up to a delimiter
            let rest = &out[abs..];
            let skip = rest.chars().take_while(|c| *c == '=' || *c == ':' || c.is_whitespace()).count();
            let val_start = abs + skip;
            let val_len = out[val_start..]
                .chars()
                .take_while(|c| !matches!(*c, ';' | '&' | ',' | ' ' | '"' | '\''))
                .map(|c| c.len_utf8())
                .sum::<usize>();
            if val_len > 0 {
                out.replace_range(val_start..val_start + val_len, "***");
            }
            start = val_start + 3;
            if start >= out.len() {
                break;
            }
        }
    }
    out
}

/// ODBC-bridge adapter for gbase8a / gbase8s (Informix kernel via CSDK ODBC
/// driver). Port of `gbase8s_odbc_conn.go`: DSN names a system ODBC source,
/// never embeds creds in logs.
pub struct OdbcBridgeAdapter {
    engine_id: &'static str,
    dsn: Option<String>,
}

impl OdbcBridgeAdapter {
    pub fn gbase8a() -> Self {
        Self { engine_id: "gbase8a", dsn: None }
    }
    pub fn gbase8s() -> Self {
        Self { engine_id: "gbase8s", dsn: None }
    }

    fn exec_blocking(dsn: &str, sql: &str) -> Result<FakeRows, DriverError> {
        let env = odbc_api::Environment::new().map_err(|e| DriverError::Connect(redact_secrets(&e.to_string())))?;
        let conn = env
            .connect_with_connection_string(dsn, ConnectionOptions::default())
            .map_err(|e| DriverError::Connect(redact_secrets(&e.to_string())))?;
        let out = match conn.execute(sql, (), None).map_err(|e| DriverError::Query(redact_secrets(&e.to_string())))? {
            Some(mut cursor) => {
                let ncols = cursor.num_result_cols().map_err(|e| DriverError::Query(e.to_string()))? as usize;
                let mut names: Vec<String> = Vec::with_capacity(ncols);
                for i in 1..=ncols as u16 {
                    names.push(cursor.col_name(i).map_err(|e| DriverError::Query(e.to_string()))?);
                }
                let mut out = FakeRows { columns: names.clone(), rows: vec![] };
                while let Some(mut row) = cursor.next_row().map_err(|e| DriverError::Query(e.to_string()))? {
                    let mut map = HashMap::new();
                    for (idx, name) in names.iter().enumerate() {
                        let mut buf = Vec::new();
                        let present = row.get_text((idx + 1) as u16, &mut buf).unwrap_or(false);
                        let val = if present {
                            serde_json::Value::String(String::from_utf8_lossy(&buf).into_owned())
                        } else {
                            serde_json::Value::Null
                        };
                        map.insert(name.clone(), val);
                    }
                    out.rows.push(map);
                    if out.rows.len() >= LONGTAIL_ROW_BUDGET {
                        break;
                    }
                }
                Ok(out)
            }
            None => Ok(empty_rows()),
        };
        out
    }
}

#[async_trait::async_trait]
impl DatabaseDriver for OdbcBridgeAdapter {
    async fn connect(&mut self, dsn: &str) -> Result<(), DriverError> {
        if dsn.trim().is_empty() {
            return Err(DriverError::Connect(format!("{}: empty ODBC connection string", self.engine_id)));
        }
        // Validate the DSN now (open + close); per-call connections after.
        let dsn_owned = dsn.to_string();
        tokio::task::spawn_blocking(move || {
            let env = odbc_api::Environment::new().map_err(|e| DriverError::Connect(redact_secrets(&e.to_string())))?;
            env.connect_with_connection_string(&dsn_owned, ConnectionOptions::default())
                .map_err(|e| DriverError::Connect(redact_secrets(&e.to_string())))?;
            Ok::<_, DriverError>(())
        })
        .await
        .map_err(|e| DriverError::Connect(e.to_string()))??;
        self.dsn = Some(dsn.to_string());
        Ok(())
    }

    async fn close(&mut self) -> Result<(), DriverError> {
        self.dsn = None;
        Ok(())
    }

    async fn ping(&self) -> Result<(), DriverError> {
        self.dsn.as_ref().map(|_| ()).ok_or(DriverError::Closed)
    }

    async fn query(&self, sql: &str) -> Result<FakeRows, DriverError> {
        let dsn = self.dsn.clone().ok_or(DriverError::Closed)?;
        let sql_owned = sql.to_string();
        tokio::task::spawn_blocking(move || Self::exec_blocking(&dsn, &sql_owned))
            .await
            .map_err(|e| DriverError::Query(e.to_string()))?
    }

    async fn exec(&self, sql: &str) -> Result<i64, DriverError> {
        let rows = self.query(sql).await?;
        Ok(rows.rows.len() as i64)
    }

    async fn get_databases(&self) -> Result<Vec<String>, DriverError> {
        Ok(self.query("SELECT name FROM sysdatabases").await?.rows.iter()
            .filter_map(|r| r.values().next()?.as_str().map(str::to_string)).collect())
    }

    async fn get_tables(&self, db: &str) -> Result<Vec<String>, DriverError> {
        crate::quirks::check_ident(db)?;
        Ok(self.query(&format!("SELECT tabname FROM `{db}`.systables")).await?.rows.iter()
            .filter_map(|r| r.values().next()?.as_str().map(str::to_string)).collect())
    }

    async fn get_columns(&self, db: &str, table: &str) -> Result<Vec<ColumnDef>, DriverError> {
        crate::quirks::check_ident(db)?;
        crate::quirks::check_ident(table)?;
        Ok(self.query(&format!("SELECT colname, coltype FROM `{db}`.syscolumns WHERE tabid = (SELECT tabid FROM `{db}`.systables WHERE tabname = '{table}')"))
            .await?.rows.iter().map(|r| ColumnDef {
                name: r.get("colname").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                data_type: r.get("coltype").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                nullable: true,
            }).collect())
    }

    fn capabilities(&self) -> irondb_drivers::CapabilityFlags {
        irondb_drivers::CapabilityFlags::empty()
    }

    fn engine_id(&self) -> &'static str {
        self.engine_id
    }
}

/// Sidecar-reuse adapter: delegates to the original Go driver agent through
/// the Phase-4 harness (`SidecarClient::spawn`). Binary comes from
/// `irondb_SIDECAR_BIN`; without it every op fails closed with a hint.
pub struct SidecarAdapter {
    engine_id: &'static str,
    client: Option<Arc<SidecarClient>>,
}

impl SidecarAdapter {
    pub fn new(engine_id: &'static str) -> Self {
        Self { engine_id, client: None }
    }

    fn req(&self, method: &str, query: Option<String>) -> Result<AgentRequest, DriverError> {
        let client = self.client.as_ref().ok_or(DriverError::Closed)?;
        Ok(AgentRequest {
            id: client.alloc_id(),
            method: method.into(),
            session_id: None,
            query,
            args: None,
            timeout_ms: Some(30_000),
            row_budget: Some(RowBudgetOptions { max_total_rows: Some(LONGTAIL_ROW_BUDGET as i64), max_total_bytes: None }),
            db_name: None,
            table_name: None,
            alias: Some(self.engine_id.into()),
            target_id: None,
            data: None,
        })
    }

    async fn call(&self, req: AgentRequest) -> Result<irondb_sidecar::AgentResponse, DriverError> {
        let client = self.client.as_ref().ok_or(DriverError::Closed)?.clone();
        client.call(req, SIDECAR_TIMEOUT).await.map_err(|e| DriverError::Query(redact_secrets(&e.to_string())))
    }

    fn rows_from(resp: &irondb_sidecar::AgentResponse) -> Result<FakeRows, DriverError> {
        if !resp.success {
            return Err(DriverError::Query(redact_secrets(resp.error.as_deref().unwrap_or("sidecar error"))));
        }
        let columns = resp.fields.clone().unwrap_or_default();
        let mut out = FakeRows { columns, rows: vec![] };
        if let Some(serde_json::Value::Array(rows)) = resp.data.as_ref().and_then(|d| d.get("rows")).cloned() {
            for r in rows.into_iter().take(LONGTAIL_ROW_BUDGET) {
                if let serde_json::Value::Object(m) = r {
                    out.rows.push(m.into_iter().collect());
                }
            }
        }
        Ok(out)
    }
}

#[async_trait::async_trait]
impl DatabaseDriver for SidecarAdapter {
    async fn connect(&mut self, _dsn: &str) -> Result<(), DriverError> {
        let bin = std::env::var("irondb_SIDECAR_BIN")
            .map_err(|_| DriverError::Connect(format!("{}: set irondb_SIDECAR_BIN to the Go driver-agent binary", self.engine_id)))?;
        let client = SidecarClient::spawn(std::path::Path::new(&bin), None)
            .await
            .map_err(|e| DriverError::Connect(redact_secrets(&e.to_string())))?;
        self.client = Some(client);
        Ok(())
    }

    async fn close(&mut self) -> Result<(), DriverError> {
        if let Some(c) = self.client.take() {
            c.shutdown().await;
        }
        Ok(())
    }

    async fn ping(&self) -> Result<(), DriverError> {
        let client = self.client.as_ref().ok_or(DriverError::Closed)?.clone();
        client.probe_metadata().await.map_err(|e| DriverError::Connect(redact_secrets(&e.to_string())))?;
        Ok(())
    }

    async fn query(&self, sql: &str) -> Result<FakeRows, DriverError> {
        let resp = self.call(self.req("query", Some(sql.into()))?).await?;
        Self::rows_from(&resp)
    }

    async fn exec(&self, sql: &str) -> Result<i64, DriverError> {
        let resp = self.call(self.req("exec", Some(sql.into()))?).await?;
        if !resp.success {
            return Err(DriverError::Exec(redact_secrets(resp.error.as_deref().unwrap_or("sidecar error"))));
        }
        Ok(resp.rows_affected.unwrap_or(0))
    }

    async fn get_databases(&self) -> Result<Vec<String>, DriverError> {
        let resp = self.call(self.req("databases", None)?).await?;
        Ok(resp.messages.unwrap_or_default())
    }

    async fn get_tables(&self, db: &str) -> Result<Vec<String>, DriverError> {
        crate::quirks::check_ident(db)?;
        let mut r = self.req("tables", None)?;
        r.db_name = Some(db.into());
        Ok(self.call(r).await?.messages.unwrap_or_default())
    }

    async fn get_columns(&self, db: &str, table: &str) -> Result<Vec<ColumnDef>, DriverError> {
        crate::quirks::check_ident(db)?;
        crate::quirks::check_ident(table)?;
        let mut r = self.req("columns", None)?;
        r.db_name = Some(db.into());
        r.table_name = Some(table.into());
        let resp = self.call(r).await?;
        if !resp.success {
            return Err(DriverError::Query(redact_secrets(resp.error.as_deref().unwrap_or("sidecar error"))));
        }
        Ok(resp.fields.unwrap_or_default().into_iter().map(|name| ColumnDef { name, data_type: String::new(), nullable: true }).collect())
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
    fn redact_secrets_masks_passwords() {
        let s = redact_secrets("dsn=host;password=hunter2;user=a");
        assert!(!s.contains("hunter2"), "{s}");
        assert!(s.contains("***"), "{s}");
    }

    #[test]
    fn sidecar_adapter_fails_closed_without_bin() {
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        rt.block_on(async {
            std::env::remove_var("irondb_SIDECAR_BIN");
            let mut a = SidecarAdapter::new("kingbase");
            assert!(a.connect("dsn").await.is_err());
            assert!(a.query("SELECT 1").await.is_err());
        });
    }

    #[test]
    fn odbc_rejects_empty_dsn() {
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        rt.block_on(async {
            let mut a = OdbcBridgeAdapter::gbase8s();
            assert!(a.connect("").await.is_err());
        });
    }
}
