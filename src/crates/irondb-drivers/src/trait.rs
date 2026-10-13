use std::collections::HashMap;
use serde::{Deserialize, Serialize};
use crate::{CapabilityFlags, DriverError};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ColumnDef {
    pub name: String,
    pub data_type: String,
    pub nullable: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct FakeRows {
    pub columns: Vec<String>,
    pub rows: Vec<HashMap<String, serde_json::Value>>,
}

#[async_trait::async_trait]
pub trait DatabaseDriver: Send + Sync {
    async fn connect(&mut self, dsn: &str) -> Result<(), DriverError>;
    async fn close(&mut self) -> Result<(), DriverError>;
    async fn ping(&self) -> Result<(), DriverError>;
    async fn query(&self, sql: &str) -> Result<FakeRows, DriverError>;
    async fn exec(&self, sql: &str) -> Result<i64, DriverError>;
    async fn get_databases(&self) -> Result<Vec<String>, DriverError>;
    async fn get_tables(&self, db: &str) -> Result<Vec<String>, DriverError>;
    async fn get_columns(&self, db: &str, table: &str) -> Result<Vec<ColumnDef>, DriverError>;
    fn capabilities(&self) -> CapabilityFlags;
    fn engine_id(&self) -> &'static str;
}

/// Optional engine extras — never part of the core trait.
pub trait BatchApplier {
    fn apply_changes(&mut self, table: &str, rows: &[HashMap<String, serde_json::Value>]) -> Result<i64, DriverError>;
}
