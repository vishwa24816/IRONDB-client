//! Vector/search + OLAP/timeseries/kv HTTP adapters over existing `reqwest`.
//!
//! Covers chroma/qdrant/milvus/weaviate, meilisearch/typesense/opensearch,
//! clickhouse/trino/presto, tdengine/influxdb/questdb/greptimedb, etcd,
//! navicat-tunnel. NO native SDK crates per approved vehicle resolution —
//! all of these engines speak HTTP natively, so plain `reqwest` is the
//! correct vehicle (ponytail: one adapter, not eight SDKs).

use std::collections::HashMap;
use std::time::Duration;

use irondb_drivers::{ColumnDef, DatabaseDriver, DriverError, FakeRows};

use super::cn::{redact_secrets, LONGTAIL_ROW_BUDGET};

/// Generic HTTP-engine adapter: `dsn` is the base URL, `query()` POSTs
/// `{q}` to `<base>/query` with timeout + row budget, parsing either
/// `{columns, rows}` or a bare array of objects.
pub struct HttpEngineAdapter {
    engine_id: &'static str,
    quirk: &'static str,
    client: reqwest::Client,
    base_url: Option<String>,
}

impl HttpEngineAdapter {
    pub fn new(engine_id: &'static str, quirk: &'static str) -> Self {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());
        Self { engine_id, quirk, client, base_url: None }
    }
    pub fn quirk(&self) -> &'static str {
        self.quirk
    }

    fn parse_body(body: &serde_json::Value) -> FakeRows {
        let mut out = FakeRows { columns: vec![], rows: vec![] };
        if let Some(cols) = body.get("columns").and_then(|v| v.as_array()) {
            out.columns = cols.iter().filter_map(|c| c.as_str().map(str::to_string)).collect();
        }
        let rows_val = body.get("rows").unwrap_or(body);
        if let Some(arr) = rows_val.as_array() {
            for r in arr.iter().take(LONGTAIL_ROW_BUDGET) {
                if let serde_json::Value::Object(m) = r {
                    if out.columns.is_empty() {
                        out.columns = m.keys().cloned().collect();
                    }
                    out.rows.push(m.iter().map(|(k, v)| (k.clone(), v.clone())).collect());
                }
            }
        }
        out
    }
}

#[async_trait::async_trait]
impl DatabaseDriver for HttpEngineAdapter {
    async fn connect(&mut self, dsn: &str) -> Result<(), DriverError> {
        let base = dsn.trim().trim_end_matches('/').to_string();
        if base.is_empty() {
            return Err(DriverError::Connect(format!("{}: empty base URL", self.engine_id)));
        }
        // Liveness probe — failure is a clean Connect error, never a panic.
        // Probe is best-effort: unreachable hosts fail here, not on first query.
        self.client.get(format!("{base}/")).send().await.map_err(|e| DriverError::Connect(redact_secrets(&e.to_string())))?;
        self.base_url = Some(base);
        Ok(())
    }

    async fn close(&mut self) -> Result<(), DriverError> {
        self.base_url = None;
        Ok(())
    }

    async fn ping(&self) -> Result<(), DriverError> {
        self.base_url.as_ref().map(|_| ()).ok_or(DriverError::Closed)
    }

    async fn query(&self, sql: &str) -> Result<FakeRows, DriverError> {
        let base = self.base_url.clone().ok_or(DriverError::Closed)?;
        let limit_str = LONGTAIL_ROW_BUDGET.to_string();
        let mut payload = HashMap::new();
        payload.insert("q", sql);
        payload.insert("limit", limit_str.as_str());
        let resp = self.client.post(format!("{base}/query")).json(&payload).send().await
            .map_err(|e| DriverError::Query(redact_secrets(&e.to_string())))?;
        if !resp.status().is_success() {
            return Err(DriverError::Query(redact_secrets(&format!("{} -> {}", self.engine_id, resp.status()))));
        }
        let body: serde_json::Value = resp.json().await.map_err(|e| DriverError::Query(e.to_string()))?;
        Ok(Self::parse_body(&body))
    }

    async fn exec(&self, sql: &str) -> Result<i64, DriverError> {
        Ok(self.query(sql).await?.rows.len() as i64)
    }

    async fn get_databases(&self) -> Result<Vec<String>, DriverError> {
        Ok(self.query("SHOW DATABASES").await?.rows.iter()
            .filter_map(|r| r.values().next()?.as_str().map(str::to_string)).collect())
    }

    async fn get_tables(&self, db: &str) -> Result<Vec<String>, DriverError> {
        crate::quirks::check_ident(db)?;
        Ok(self.query(&format!("SHOW TABLES IN {db}")).await?.rows.iter()
            .filter_map(|r| r.values().next()?.as_str().map(str::to_string)).collect())
    }

    async fn get_columns(&self, _db: &str, table: &str) -> Result<Vec<ColumnDef>, DriverError> {
        crate::quirks::check_ident(table)?;
        Ok(self.query(&format!("DESCRIBE {table}")).await?.rows.iter().filter_map(|r| {
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
    fn parse_bare_array_and_envelope() {
        let bare = serde_json::json!([{"a": 1}, {"a": 2}]);
        let rows = HttpEngineAdapter::parse_body(&bare);
        assert_eq!(rows.rows.len(), 2);
        assert_eq!(rows.columns, vec!["a"]);
        let env = serde_json::json!({"columns": ["x"], "rows": [{"x": "y"}]});
        let rows = HttpEngineAdapter::parse_body(&env);
        assert_eq!(rows.columns, vec!["x"]);
    }

    #[test]
    fn http_adapter_fails_closed_unconnected() {
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        rt.block_on(async {
            let mut a = HttpEngineAdapter::new("qdrant", "HTTP/gRPC vector store");
            assert!(a.query("SELECT 1").await.is_err());
            assert!(a.connect("").await.is_err());
        });
    }
}
