//! Datasource adapters — wraps driver row types. Never imports facade/bridge.
pub use irondb_drivers::{CapabilityFlags, ColumnDef, DriverError, FakeRows};

mod quirks;
pub mod longtail;
mod mysql;
mod postgres;
mod mssql;
mod sqlite;
mod mongodb;
mod redis;
mod elasticsearch;

pub use quirks::{check_ident, quirks, EngineQuirks, PlaceholderStyle};
pub use longtail::{register_longtail, LONGTAIL_MATRIX};
pub use mysql::MySqlAdapter;
pub use postgres::PostgresAdapter;
pub use mssql::MssqlAdapter;
pub use sqlite::SqliteAdapter;
pub use mongodb::MongoAdapter;
pub use redis::RedisAdapter;
pub use elasticsearch::ElasticsearchAdapter;

use std::collections::HashMap;
use irondb_drivers::DriverRegistry;

/// Row type kept behind a struct so Phase 4 real rows slot in.
#[derive(Debug, Clone, Default)]
pub struct Row {
    pub values: HashMap<String, serde_json::Value>,
}

impl Row {
    pub fn from_fake(row: &HashMap<String, serde_json::Value>) -> Self {
        Self { values: row.clone() }
    }
}

/// Adapter skeleton wrapping a FakeDriver-compatible source.
pub struct FakeAdapter {
    pub engine_id: &'static str,
    pub caps: CapabilityFlags,
}

impl FakeAdapter {
    pub fn new() -> Self {
        Self { engine_id: irondb_drivers::FakeDriver::ENGINE_ID, caps: irondb_drivers::FakeDriver::CAPS }
    }

    pub fn rows(&self, fake: &FakeRows) -> Vec<Row> {
        fake.rows.iter().map(Row::from_fake).collect()
    }
}

impl Default for FakeAdapter {
    fn default() -> Self {
        Self::new()
    }
}

/// Register all native datasource adapters in the driver registry.
pub fn register_datasources(registry: &mut DriverRegistry) {
    // SQL family
    registry.register("mysql", CapabilityFlags::empty(), || {
        Box::new(MySqlAdapter::new())
    });
    registry.register("postgres", CapabilityFlags::empty(), || {
        Box::new(PostgresAdapter::new())
    });
    registry.register("sqlserver", CapabilityFlags::empty(), || {
        Box::new(MssqlAdapter::new())
    });
    registry.register("sqlite", CapabilityFlags::IN_MEMORY, || {
        Box::new(SqliteAdapter::new())
    });

    // NoSQL family
    registry.register("mongodb", CapabilityFlags::empty(), || {
        Box::new(MongoAdapter::new())
    });
    registry.register("redis", CapabilityFlags::empty(), || {
        Box::new(RedisAdapter::new())
    });
    registry.register("elasticsearch", CapabilityFlags::empty(), || {
        Box::new(ElasticsearchAdapter::new())
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use irondb_drivers::DriverRegistry;

    #[test]
    fn all_engines_registered() {
        let mut registry = DriverRegistry::new();
        register_datasources(&mut registry);
        let engines = registry.engines();
        assert!(engines.contains(&"mysql"));
        assert!(engines.contains(&"postgres"));
        assert!(engines.contains(&"sqlserver"));
        assert!(engines.contains(&"sqlite"));
        assert!(engines.contains(&"mongodb"));
        assert!(engines.contains(&"redis"));
        assert!(engines.contains(&"elasticsearch"));
    }
}