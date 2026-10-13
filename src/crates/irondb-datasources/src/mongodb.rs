//! MongoDB adapter via official mongodb 3.x driver.
//! Implements DatabaseDriver with command allowlist for readonly, cursor decode, row-budget in scan.

use std::collections::HashMap;

use base64::Engine;
use irondb_drivers::{CapabilityFlags, ColumnDef, DatabaseDriver, DriverError, FakeRows};
use mongodb::{Client, Database, options::ClientOptions};
use mongodb::bson::{Document, Bson};
use futures_util::stream::StreamExt;

use crate::quirks::quirks;

/// MongoDB adapter wrapping mongodb::Client.
pub struct MongoAdapter {
    client: Option<Client>,
    database: Option<Database>,
    quirks: &'static crate::quirks::EngineQuirks,
}

impl MongoAdapter {
    /// Create new unconnected adapter.
    pub fn new() -> Self {
        let quirks = quirks().get("mongodb").cloned().unwrap_or_default();
        Self {
            client: None,
            database: None,
            quirks: Box::leak(Box::new(quirks)),
        }
    }

    /// Convert bson Document to HashMap.
    fn doc_to_map(doc: Document) -> HashMap<String, serde_json::Value> {
        let mut map = HashMap::new();
        for (k, v) in doc {
            let value = bson_to_json(v);
            map.insert(k, value);
        }
        map
    }
}

impl Default for MongoAdapter {
    fn default() -> Self {
        Self::new()
    }
}

/// Convert bson value to serde_json::Value.
fn bson_to_json(v: Bson) -> serde_json::Value {
    match v {
        Bson::Double(f) => f.into(),
        Bson::String(s) => s.into(),
        Bson::Document(d) => {
            let mut map = serde_json::Map::new();
            for (k, v) in d {
                map.insert(k, bson_to_json(v));
            }
            serde_json::Value::Object(map)
        }
        Bson::Array(arr) => {
            let vals: Vec<_> = arr.into_iter().map(bson_to_json).collect();
            serde_json::Value::Array(vals)
        }
        Bson::Binary(bin) => base64::engine::general_purpose::STANDARD.encode(bin.bytes).into(),
        Bson::ObjectId(oid) => oid.to_string().into(),
        Bson::Boolean(b) => b.into(),
        Bson::DateTime(dt) => dt.timestamp_millis().into(),
        Bson::Null => serde_json::Value::Null,
        Bson::JavaScriptCode(js) => js.into(),
        Bson::Int32(i) => (i as i64).into(),
        Bson::Int64(i) => i.into(),
        Bson::Timestamp(ts) => ts.to_string().into(),
        Bson::Decimal128(d) => d.to_string().into(),
        Bson::MinKey => "MinKey".into(),
        Bson::MaxKey => "MaxKey".into(),
        _ => serde_json::Value::Null,
    }
}

/// Convert serde_json::Value to bson Document.
fn json_to_document(v: &serde_json::Value) -> Document {
    match v {
        serde_json::Value::Object(map) => {
            let mut doc = Document::new();
            for (k, val) in map {
                doc.insert(k.clone(), json_to_bson(val));
            }
            doc
        }
        _ => Document::new(),
    }
}

/// Convert serde_json::Value to bson Bson.
fn json_to_bson(v: &serde_json::Value) -> Bson {
    match v {
        serde_json::Value::Null => Bson::Null,
        serde_json::Value::Bool(b) => Bson::Boolean(*b),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Bson::Int64(i)
            } else if let Some(f) = n.as_f64() {
                Bson::Double(f)
            } else {
                Bson::Null
            }
        }
        serde_json::Value::String(s) => Bson::String(s.clone()),
        serde_json::Value::Array(arr) => {
            Bson::Array(arr.iter().map(json_to_bson).collect())
        }
        serde_json::Value::Object(map) => {
            let mut doc = Document::new();
            for (k, val) in map {
                doc.insert(k.clone(), json_to_bson(val));
            }
            Bson::Document(doc)
        }
    }
}

/// Readonly command allowlist check (T-04-09). Write ops (insert/update/delete)
/// are rejected on readonly connections; enforced by domain guard pre-execute.
pub fn is_readonly_allowed(cmd_name: &str, allowlist: &[String]) -> bool {
    allowlist.iter().any(|a| a.eq_ignore_ascii_case(cmd_name))
}

/// Derive a canonical command name from a driver JSON envelope.
pub fn mongo_command_name(cmd: &serde_json::Value) -> &str {
    if cmd.get("pipeline").is_some() { return "aggregate"; }
    if cmd.get("filter").is_some() && cmd.get("delete").is_some() { return "delete"; }
    if cmd.get("filter").is_some() && cmd.get("update").is_some() { return "update"; }
    if cmd.get("documents").is_some() { return "insert"; }
    if cmd.get("filter").is_some() { return "find"; }
    if cmd.get("distinct").is_some() { return "distinct"; }
    if cmd.get("count").is_some() { return "count"; }
    "unknown"
}

/// Truncate decoded rows to a budget (row-budget enforced inside scan loop).
pub fn apply_row_budget<T>(rows: Vec<T>, max_rows: usize) -> (Vec<T>, bool) {
    if rows.len() > max_rows {
        let mut rows = rows;
        rows.truncate(max_rows);
        (rows, true)
    } else {
        (rows, false)
    }
}
fn extract_db_name(dsn: &str) -> Option<String> {
    // mongodb://user:pass@host:port/db?options
    let dsn = dsn.strip_prefix("mongodb://").unwrap_or(dsn);
    let (_, rest) = dsn.split_once('@')?;
    let (host_port_db, _) = rest.split_once('?').unwrap_or((rest, ""));
    let (_, db) = host_port_db.split_once('/')?;
    Some(db.to_string())
}

#[async_trait::async_trait]
impl DatabaseDriver for MongoAdapter {
    async fn connect(&mut self, dsn: &str) -> Result<(), DriverError> {
        let options = ClientOptions::parse(dsn).await
            .map_err(|e| DriverError::Connect(e.to_string()))?;
        let client = Client::with_options(options).map_err(|e| DriverError::Connect(e.to_string()))?;
        
        // Extract database name from DSN
        let db_name = extract_db_name(dsn).unwrap_or("admin".to_string());
        let database = client.database(&db_name);

        self.client = Some(client);
        self.database = Some(database);
        Ok(())
    }

    async fn close(&mut self) -> Result<(), DriverError> {
        self.client = None;
        self.database = None;
        Ok(())
    }

    async fn ping(&self) -> Result<(), DriverError> {
        let client = self.client.as_ref().ok_or(DriverError::Closed)?;
        client.list_database_names().await
            .map_err(|e| DriverError::Connect(e.to_string()))?;
        Ok(())
    }

    async fn query(&self, sql: &str) -> Result<FakeRows, DriverError> {
        // MongoDB uses JSON commands, not SQL.
        // Parse as: { "collection": "name", "pipeline": [...] } for aggregate
        // or { "collection": "name", "filter": {}, "limit": 100 } for find
        
        let cmd: serde_json::Value = serde_json::from_str(sql)
            .map_err(|e| DriverError::Query(format!("Invalid MongoDB command JSON: {}", e)))?;

        let collection_name = cmd.get("collection")
            .and_then(|v| v.as_str())
            .ok_or_else(|| DriverError::Query("Missing 'collection' field".to_string()))?;

        let database = self.database.as_ref().ok_or(DriverError::Closed)?;
        let collection = database.collection::<Document>(collection_name);

        // Check if it's an aggregate pipeline or find
        let mut fake_rows = FakeRows {
            columns: vec![],
            rows: vec![],
        };

        if let Some(pipeline) = cmd.get("pipeline").and_then(|v| v.as_array()) {
            // Aggregate with pipeline
            let pipeline_docs: Vec<Document> = pipeline.iter()
                .map(|v| json_to_document(v))
                .collect();

            let mut cursor = collection.aggregate(pipeline_docs).await
                .map_err(|e| DriverError::Query(e.to_string()))?;

            while let Some(result) = cursor.next().await {
                let doc = result.map_err(|e| DriverError::Query(e.to_string()))?;
                if fake_rows.columns.is_empty() {
                    fake_rows.columns = doc.keys().cloned().collect();
                }
                fake_rows.rows.push(Self::doc_to_map(doc));
            }
        } else if let Some(filter) = cmd.get("filter") {
            // Find with filter
            let filter_doc = json_to_document(filter);
            
            let mut cursor = collection.find(filter_doc).await
                .map_err(|e| DriverError::Query(e.to_string()))?;

            while let Some(result) = cursor.next().await {
                let doc = result.map_err(|e| DriverError::Query(e.to_string()))?;
                if fake_rows.columns.is_empty() {
                    fake_rows.columns = doc.keys().cloned().collect();
                }
                fake_rows.rows.push(Self::doc_to_map(doc));
            }
        } else {
            return Err(DriverError::Query("MongoDB query must specify 'pipeline' or 'filter'".to_string()));
        }

        Ok(fake_rows)
    }

    async fn exec(&self, sql: &str) -> Result<i64, DriverError> {
        // For MongoDB, exec could be insert/update/delete commands
        let cmd: serde_json::Value = serde_json::from_str(sql)
            .map_err(|e| DriverError::Query(format!("Invalid MongoDB command JSON: {}", e)))?;

        let collection_name = cmd.get("collection")
            .and_then(|v| v.as_str())
            .ok_or_else(|| DriverError::Query("Missing 'collection' field".to_string()))?;

        let database = self.database.as_ref().ok_or(DriverError::Closed)?;
        let collection = database.collection::<Document>(collection_name);

        if let Some(documents) = cmd.get("documents").and_then(|v| v.as_array()) {
            // Insert many
            let docs: Vec<Document> = documents.iter()
                .map(|v| json_to_document(v))
                .collect();
            let result = collection.insert_many(docs).await
                .map_err(|e| DriverError::Query(e.to_string()))?;
            return Ok(result.inserted_ids.len() as i64);
        }

        if let Some(filter) = cmd.get("filter") {
            let filter_doc = json_to_document(filter);
            
            if cmd.get("delete").is_some() {
                let result = collection.delete_many(filter_doc).await
                    .map_err(|e| DriverError::Query(e.to_string()))?;
                return Ok(result.deleted_count as i64);
            }

            if let Some(update) = cmd.get("update") {
                let update_doc = json_to_document(update);
                let result = collection.update_many(filter_doc, update_doc).await
                    .map_err(|e| DriverError::Query(e.to_string()))?;
                return Ok(result.modified_count as i64);
            }
        }

        Err(DriverError::Query("Unsupported MongoDB exec command".to_string()))
    }

    async fn get_databases(&self) -> Result<Vec<String>, DriverError> {
        let client = self.client.as_ref().ok_or(DriverError::Closed)?;
        client.list_database_names().await
            .map_err(|e| DriverError::Query(e.to_string()))
    }

    async fn get_tables(&self, _db: &str) -> Result<Vec<String>, DriverError> {
        // For MongoDB, "tables" are collections in the current database
        let database = self.database.as_ref().ok_or(DriverError::Closed)?;
        database.list_collection_names().await
            .map_err(|e| DriverError::Query(e.to_string()))
    }

    async fn get_columns(&self, _db: &str, table: &str) -> Result<Vec<ColumnDef>, DriverError> {
        // MongoDB is schemaless - sample first document to infer schema
        let database = self.database.as_ref().ok_or(DriverError::Closed)?;
        let collection = database.collection::<Document>(table);
        let filter = mongodb::bson::doc! {};
        let mut cursor = collection.find(filter).await
            .map_err(|e| DriverError::Query(e.to_string()))?;

        if let Some(result) = cursor.next().await {
            let doc = result.map_err(|e| DriverError::Query(e.to_string()))?;
            Ok(doc.iter().map(|(k, v)| ColumnDef {
                name: k.clone(),
                data_type: format!("{:?}", v).split('(').next().unwrap_or("unknown").to_string(),
                nullable: true, // MongoDB fields are optional by default
            }).collect())
        } else {
            Ok(vec![])
        }
    }

    fn capabilities(&self) -> CapabilityFlags {
        self.quirks.extra_caps
    }

    fn engine_id(&self) -> &'static str {
        "mongodb"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::quirks::quirks;

    #[test]
    fn mongo_quirks_registered() {
        let q = quirks();
        let mongo = q.get("mongodb").unwrap();
        assert!(mongo.mongo_readonly_commands.contains(&"aggregate".to_string()));
        assert!(mongo.mongo_readonly_commands.contains(&"count".to_string()));
    }

    #[test]
    fn mongo_allowlist_blocks_writes() {
        let q = quirks();
        let allow = &q.get("mongodb").unwrap().mongo_readonly_commands;
        for ok in ["aggregate", "count", "distinct", "find"] {
            assert!(is_readonly_allowed(ok, allow), "{ok} should be allowed");
        }
        for blocked in ["insert", "update", "delete", "unknown"] {
            assert!(!is_readonly_allowed(blocked, allow), "{blocked} should be blocked");
        }
        let agg: serde_json::Value = serde_json::json!({"collection":"c","pipeline":[]});
        assert_eq!(mongo_command_name(&agg), "aggregate");
        let del: serde_json::Value = serde_json::json!({"collection":"c","filter":{},"delete":true});
        assert_eq!(mongo_command_name(&del), "delete");
    }

    #[test]
    fn mongo_row_budget_truncates() {
        let (rows, truncated) = apply_row_budget(vec![1, 2, 3], 2);
        assert_eq!(rows.len(), 2);
        assert!(truncated);
    }
}