use std::collections::HashMap;
use crate::{CapabilityFlags, ColumnDef, DatabaseDriver, DriverError, FakeRows};

fn row(pairs: &[(&str, serde_json::Value)]) -> HashMap<String, serde_json::Value> {
    pairs.iter().map(|(k, v)| (k.to_string(), v.clone())).collect()
}

/// In-memory fake driver for facade E2E. Dummy DSN only — no secrets.
pub struct FakeDriver {
    connected: bool,
    readonly: bool,
}

impl FakeDriver {
    pub fn new() -> Self {
        Self { connected: false, readonly: false }
    }

    pub fn new_readonly() -> Self {
        Self { connected: false, readonly: true }
    }

    pub const ENGINE_ID: &'static str = "fake";

    pub const CAPS: CapabilityFlags = CapabilityFlags::IN_MEMORY;

    fn canned_users() -> FakeRows {
        FakeRows {
            columns: vec!["id".to_string(), "name".to_string()],
            rows: vec![
                row(&[("id", 1.into()), ("name", "alice".into())]),
                row(&[("id", 2.into()), ("name", "bob".into())]),
            ],
        }
    }
}

impl Default for FakeDriver {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl DatabaseDriver for FakeDriver {
    async fn connect(&mut self, _dsn: &str) -> Result<(), DriverError> {
        self.connected = true;
        Ok(())
    }

    async fn close(&mut self) -> Result<(), DriverError> {
        self.connected = false;
        Ok(())
    }

    async fn ping(&self) -> Result<(), DriverError> {
        if self.connected {
            Ok(())
        } else {
            Err(DriverError::Closed)
        }
    }

    async fn query(&self, sql: &str) -> Result<FakeRows, DriverError> {
        if !self.connected {
            return Err(DriverError::Closed);
        }
        let q = sql.trim().to_lowercase();
        if q.contains("users") {
            Ok(Self::canned_users())
        } else if q.contains("boom") {
            Err(DriverError::Query("synthetic scripted failure".to_string()))
        } else {
            Ok(FakeRows { columns: vec![], rows: vec![] })
        }
    }

    async fn exec(&self, sql: &str) -> Result<i64, DriverError> {
        if !self.connected {
            return Err(DriverError::Closed);
        }
        if self.readonly {
            return Err(DriverError::Denied("readonly connection: writes denied".to_string()));
        }
        let q = sql.trim().to_lowercase();
        if q.starts_with("insert") {
            Ok(1)
        } else if q.starts_with("update") || q.starts_with("delete") {
            Ok(1)
        } else {
            Ok(0)
        }
    }

    async fn get_databases(&self) -> Result<Vec<String>, DriverError> {
        Ok(vec!["fakedb".to_string()])
    }

    async fn get_tables(&self, _db: &str) -> Result<Vec<String>, DriverError> {
        Ok(vec!["users".to_string(), "orders".to_string()])
    }

    async fn get_columns(&self, _db: &str, table: &str) -> Result<Vec<ColumnDef>, DriverError> {
        if table == "users" {
            Ok(vec![
                ColumnDef { name: "id".to_string(), data_type: "INTEGER".to_string(), nullable: false },
                ColumnDef { name: "name".to_string(), data_type: "TEXT".to_string(), nullable: true },
            ])
        } else {
            Ok(vec![])
        }
    }

    fn capabilities(&self) -> CapabilityFlags {
        Self::CAPS
    }

    fn engine_id(&self) -> &'static str {
        Self::ENGINE_ID
    }
}
