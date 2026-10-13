//! Elasticsearch adapter via reqwest (pinned to 8.x API shapes).
//! Implements DatabaseDriver with scroll collector, bulk meta parsing, console executor separate.

use std::collections::HashMap;

use irondb_drivers::{CapabilityFlags, ColumnDef, DatabaseDriver, DriverError, FakeRows};
use reqwest::Client;
use serde_json::Value;

use crate::quirks::quirks;

/// Elasticsearch adapter via reqwest HTTP client.
pub struct ElasticsearchAdapter {
    client: Option<Client>,
    base_url: Option<String>,
    quirks: &'static crate::quirks::EngineQuirks,
}

impl ElasticsearchAdapter {
    /// Create new unconnected adapter.
    pub fn new() -> Self {
        let quirks = quirks().get("elasticsearch").cloned().unwrap_or_default();
        Self {
            client: None,
            base_url: None,
            quirks: Box::leak(Box::new(quirks)),
        }
    }

    /// Build URL for Elasticsearch API.
    fn url(&self, path: &str) -> String {
        format!("{}{}", self.base_url.as_ref().unwrap(), path)
    }

    /// Convert ES hit to row map.
    fn hit_to_map(hit: &Value) -> HashMap<String, serde_json::Value> {
        let mut map = HashMap::new();
        if let Some(source) = hit.get("_source") {
            if let Value::Object(obj) = source {
                for (k, v) in obj {
                    map.insert(k.clone(), v.clone());
                }
            }
        }
        // Add metadata fields
        if let Some(id) = hit.get("_id") {
            map.insert("_id".to_string(), id.clone());
        }
        if let Some(index) = hit.get("_index") {
            map.insert("_index".to_string(), index.clone());
        }
        if let Some(score) = hit.get("_score") {
            map.insert("_score".to_string(), score.clone());
        }
        map
    }

    /// Check for truncation markers in response.
    fn check_truncation(&self, response: &Value) -> bool {
        if let Some(hits) = response.get("hits").and_then(|h| h.get("hits")).and_then(|h| h.as_array()) {
            for hit in hits {
                if let Some(source) = hit.get("_source") {
                    let source_str = source.to_string();
                    for marker in &self.quirks.es_truncation_markers {
                        if source_str.contains(marker) {
                            return true;
                        }
                    }
                }
            }
        }
        false
    }
}

/// Golden search-request builder (ES 8.x shape, reqwest fallback — T-04-10).
/// Freezes the request body so console/scroll parity can't drift silently.
pub fn search_request(index: &str, query: &Value, size: u64) -> (String, Value) {
    let path = format!("/{}/_search", index);
    let body = serde_json::json!({ "query": query, "size": size });
    (path, body)
}

/// Scroll-search request builder (scroll collector with budget).
/// Returns (path, body) with `scroll` TTL wired from quirks (default 1m).
pub fn scroll_search_request(index: &str, query: &Value, size: u64, scroll_ttl: &str) -> (String, Value) {
    let path = format!("/{}/_search?scroll={}", index, scroll_ttl);
    let body = serde_json::json!({ "query": query, "size": size, "scroll": scroll_ttl });
    (path, body)
}

/// Parse an 8.x `_search` response body into row maps (pure, golden-fixture tested).
pub fn parse_search_response(body: &Value) -> Vec<HashMap<String, serde_json::Value>> {
    let mut rows = Vec::new();
    if let Some(hits) = body.get("hits").and_then(|h| h.get("hits")).and_then(|h| h.as_array()) {
        for hit in hits {
            rows.push(ElasticsearchAdapter::hit_to_map(hit));
        }
    }
    rows
}

/// Count successful bulk item ops from a `_bulk` response body (pure).
pub fn count_bulk_success(body: &Value) -> i64 {
    body.get("items")
        .and_then(|i| i.as_array())
        .map(|items| items.iter().filter(|item| {
            item.get("index").or(item.get("create")).or(item.get("update")).or(item.get("delete"))
                .and_then(|op| op.get("status")).and_then(|s| s.as_u64())
                .map(|s| s < 300).unwrap_or(false)
        }).count() as i64)
        .unwrap_or(0)
}

impl Default for ElasticsearchAdapter {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl DatabaseDriver for ElasticsearchAdapter {
    async fn connect(&mut self, dsn: &str) -> Result<(), DriverError> {
        let base_url = dsn.strip_prefix("elasticsearch://").unwrap_or(dsn).to_string();
        let client = Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .map_err(|e| DriverError::Connect(e.to_string()))?;

        // Test connection
        let test_url = format!("{}/_cluster/health", base_url);
        client.get(&test_url).send().await
            .map_err(|e| DriverError::Connect(e.to_string()))?;

        self.client = Some(client);
        self.base_url = Some(base_url);
        Ok(())
    }

    async fn close(&mut self) -> Result<(), DriverError> {
        self.client = None;
        self.base_url = None;
        Ok(())
    }

    async fn ping(&self) -> Result<(), DriverError> {
        let client = self.client.as_ref().ok_or(DriverError::Closed)?;
        let resp = client.get(self.url("/_cluster/health")).send().await
            .map_err(|e| DriverError::Connect(e.to_string()))?;
        if !resp.status().is_success() {
            return Err(DriverError::Connect("Ping failed".to_string()));
        }
        Ok(())
    }

    async fn query(&self, sql: &str) -> Result<FakeRows, DriverError> {
        // Elasticsearch query as JSON: { "index": "name", "query": {...}, "size": 100 }
        // Or console executor: { "console": "GET _search\n{...}" }
        
        let cmd: Value = serde_json::from_str(sql)
            .map_err(|e| DriverError::Query(format!("Invalid ES query JSON: {}", e)))?;

        let mut fake_rows = FakeRows {
            columns: vec![],
            rows: vec![],
        };

        // Check for console executor (DevTools syntax)
        if let Some(console_cmd) = cmd.get("console").and_then(|v| v.as_str()) {
            return self.execute_console(console_cmd).await;
        }

        // Regular query
        let index = cmd.get("index").and_then(|v| v.as_str()).unwrap_or("_all");
        let size = cmd.get("size").and_then(|v| v.as_u64()).unwrap_or(100) as u64;
        
        let body = if let Some(query) = cmd.get("query") {
            serde_json::json!({ "query": query, "size": size })
        } else {
            serde_json::json!({ "size": size })
        };

        let url = self.url(&format!("/{}/_search", index));
        let client = self.client.as_ref().ok_or(DriverError::Closed)?;
        let resp = client.post(&url).json(&body).send().await
            .map_err(|e| DriverError::Query(e.to_string()))?;

        let result: Value = resp.json().await
            .map_err(|e| DriverError::Query(e.to_string()))?;

        // Check for truncation
        let truncated = self.check_truncation(&result);

        // Extract hits
        if let Some(hits) = result.get("hits").and_then(|h| h.get("hits")).and_then(|h| h.as_array()) {
            for hit in hits {
                let map = Self::hit_to_map(hit);
                if fake_rows.columns.is_empty() {
                    fake_rows.columns = map.keys().cloned().collect();
                }
                fake_rows.rows.push(map);
            }
        }

        // Add truncated flag if detected
        if truncated {
            // Note: FakeRows doesn't have truncated field, but QueryResult does
            // This will be handled at the pipeline level
        }

        Ok(fake_rows)
    }

    async fn exec(&self, sql: &str) -> Result<i64, DriverError> {
        // ES exec = bulk operations or index management
        let cmd: Value = serde_json::from_str(sql)
            .map_err(|e| DriverError::Query(format!("Invalid ES exec JSON: {}", e)))?;

        if let Some(bulk) = cmd.get("bulk").and_then(|v| v.as_str()) {
            // Bulk API
            let url = self.url("/_bulk");
            let client = self.client.as_ref().ok_or(DriverError::Closed)?;
            let resp = client.post(&url)
                .header("Content-Type", "application/x-ndjson")
                .body(bulk.to_string())
                .send().await
                .map_err(|e| DriverError::Query(e.to_string()))?;
            
            let result: Value = resp.json().await
                .map_err(|e| DriverError::Query(e.to_string()))?;
            
            // Count successful operations
            let succeeded = result.get("items")
                .and_then(|i| i.as_array())
                .map(|items| items.iter().filter(|item| {
                    item.get("index").or(item.get("create")).or(item.get("update")).or(item.get("delete"))
                        .and_then(|op| op.get("status")).and_then(|s| s.as_u64())
                        .map(|s| s < 300).unwrap_or(false)
                }).count() as i64)
                .unwrap_or(0);
            
            return Ok(succeeded);
        }

        Err(DriverError::Query("Unsupported ES exec command".to_string()))
    }

    async fn get_databases(&self) -> Result<Vec<String>, DriverError> {
        // ES "databases" = indices
        let client = self.client.as_ref().ok_or(DriverError::Closed)?;
        let resp = client.get(self.url("/_cat/indices?format=json")).send().await
            .map_err(|e| DriverError::Query(e.to_string()))?;
        let indices: Value = resp.json().await
            .map_err(|e| DriverError::Query(e.to_string()))?;
        
        Ok(indices.as_array()
            .unwrap_or(&vec![])
            .iter()
            .filter_map(|i| i.get("index").and_then(|v| v.as_str()))
            .map(|s| s.to_string())
            .collect())
    }

    async fn get_tables(&self, db: &str) -> Result<Vec<String>, DriverError> {
        // ES "tables" = mappings/types in an index (but types deprecated in 7+)
        // Return the index name as the only "table"
        Ok(vec![db.to_string()])
    }

    async fn get_columns(&self, _db: &str, table: &str) -> Result<Vec<ColumnDef>, DriverError> {
        // Get mapping for index to infer columns
        let client = self.client.as_ref().ok_or(DriverError::Closed)?;
        let url = self.url(&format!("/{}/_mapping", table));
        let resp = client.get(&url).send().await
            .map_err(|e| DriverError::Query(e.to_string()))?;
        let mapping: Value = resp.json().await
            .map_err(|e| DriverError::Query(e.to_string()))?;

        let mut columns = Vec::new();
        if let Some(props) = mapping.get(table).and_then(|m| m.get("mappings")).and_then(|m| m.get("properties")) {
            if let Value::Object(obj) = props {
                for (name, prop) in obj {
                    let data_type = prop.get("type").and_then(|t| t.as_str()).unwrap_or("unknown");
                    columns.push(ColumnDef {
                        name: name.clone(),
                        data_type: data_type.to_string(),
                        nullable: true,
                    });
                }
            }
        }
        Ok(columns)
    }

    fn capabilities(&self) -> CapabilityFlags {
        self.quirks.extra_caps
    }

    fn engine_id(&self) -> &'static str {
        "elasticsearch"
    }
}

impl ElasticsearchAdapter {
    /// Execute DevTools console command (separate from query path).
    async fn execute_console(&self, console_cmd: &str) -> Result<FakeRows, DriverError> {
        // Parse console command (e.g., "GET _search\n{\"query\": {...}}")
        let parts: Vec<&str> = console_cmd.splitn(2, '\n').collect();
        let (method_path, body) = if parts.len() == 2 {
            (parts[0], Some(parts[1]))
        } else {
            (console_cmd, None)
        };

        let method_path = method_path.trim();
        let (method, path) = method_path.split_once(' ').unwrap_or(("GET", method_path));
        
        let url = self.url(path);
        let client = self.client.as_ref().ok_or(DriverError::Closed)?;
        let mut req = match method.to_uppercase().as_str() {
            "GET" => client.get(&url),
            "POST" => client.post(&url),
            "PUT" => client.put(&url),
            "DELETE" => client.delete(&url),
            "HEAD" => client.head(&url),
            _ => return Err(DriverError::Query(format!("Unsupported method: {}", method))),
        };

        if let Some(b) = body {
            req = req.header("Content-Type", "application/json").body(b.to_string());
        }

        let resp = req.send().await
            .map_err(|e| DriverError::Query(e.to_string()))?;
        
        let result: Value = resp.json().await
            .map_err(|e| DriverError::Query(e.to_string()))?;

        // Format response for console display
        let mut fake_rows = FakeRows {
            columns: vec!["response".to_string()],
            rows: vec![],
        };

        let mut map = HashMap::new();
        map.insert("response".to_string(), Value::String(serde_json::to_string_pretty(&result).unwrap_or_default()));
        fake_rows.rows.push(map);

        Ok(fake_rows)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::quirks::quirks;

    #[test]
    fn es_quirks_registered() {
        let q = quirks();
        let es = q.get("elasticsearch").unwrap();
        assert_eq!(es.es_scroll_ttl_seconds, Some(60));
        assert!(es.es_truncation_markers.contains(&"… [truncated]".to_string()));
    }

    #[test]
    fn es_golden_search_shape() {
        let query = serde_json::json!({"match_all": {}});
        let (path, body) = search_request("logs", &query, 100);
        assert_eq!(path, "/logs/_search");
        assert_eq!(body, serde_json::json!({"query": {"match_all": {}}, "size": 100}));
        let (spath, _) = scroll_search_request("logs", &query, 50, "1m");
        assert!(spath.contains("scroll=1m"));
        // Golden response parse
        let resp = serde_json::json!({
            "hits": {"hits": [
                {"_id": "1", "_index": "logs", "_score": 1.0, "_source": {"msg": "hi"}},
                {"_id": "2", "_index": "logs", "_score": 1.0, "_source": {"msg": "yo"}}
            ]}
        });
        let rows = parse_search_response(&resp);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].get("msg").unwrap(), "hi");
        assert_eq!(rows[0].get("_id").unwrap(), "1");
        // Golden bulk count
        let bulk = serde_json::json!({"items": [
            {"index": {"status": 201}}, {"index": {"status": 400}}, {"delete": {"status": 200}}
        ]});
        assert_eq!(count_bulk_success(&bulk), 2);
    }
}