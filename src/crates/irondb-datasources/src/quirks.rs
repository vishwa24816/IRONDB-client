//! Per-engine quirk matrix as code — single source of truth for driver behavior.
//! Mirrors IRONDB's quirk matrix from internal/db/database.go and driver impls.

use irondb_drivers::CapabilityFlags;
use std::collections::HashMap;

/// Engine-specific capability quirks and limits.
#[derive(Debug, Clone)]
pub struct EngineQuirks {
    pub engine_id: &'static str,
    /// Max rows per batch insert (MySQL 1000, others vary)
    pub max_batch_rows: Option<u32>,
    /// Max args per batch insert (MySQL 60000)
    pub max_batch_args: Option<u32>,
    /// Placeholder style: "?" for MySQL/SQLite, "$n" for Postgres, "@p" for MSSQL
    pub placeholder_style: PlaceholderStyle,
    /// Whether engine supports LISTEN/NOTIFY (PG only)
    pub supports_listen_notify: bool,
    /// MSSQL analyze max rows budget
    pub mssql_analyze_max_rows: Option<u32>,
    /// MongoDB readonly command allowlist
    pub mongo_readonly_commands: Vec<String>,
    /// Redis command-level guard (no type-level readonly)
    pub redis_command_guard: bool,
    /// ES scroll context TTL seconds
    pub es_scroll_ttl_seconds: Option<u32>,
    /// ES truncation marker strings
    pub es_truncation_markers: Vec<String>,
    /// SQLite single-writer discipline enforced
    pub sqlite_single_writer: bool,
    /// SQLite ATTACH/DETACH external spec support
    pub sqlite_attach_external: bool,
    /// Additional capability flags beyond base
    pub extra_caps: CapabilityFlags,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlaceholderStyle {
    QuestionMark, // MySQL, SQLite
    DollarNumber, // PostgreSQL
    AtSymbol,     // MSSQL
}

impl Default for EngineQuirks {
    fn default() -> Self {
        Self {
            engine_id: "unknown",
            max_batch_rows: None,
            max_batch_args: None,
            placeholder_style: PlaceholderStyle::QuestionMark,
            supports_listen_notify: false,
            mssql_analyze_max_rows: None,
            mongo_readonly_commands: vec![],
            redis_command_guard: false,
            es_scroll_ttl_seconds: None,
            es_truncation_markers: vec![],
            sqlite_single_writer: false,
            sqlite_attach_external: false,
            extra_caps: CapabilityFlags::empty(),
        }
    }
}

/// Validate a SQL identifier (db/table/alias) before interpolation.
/// Values always use positional binds; identifiers cannot — reject anything
/// outside `[A-Za-z0-9_.$]` (T-04-07). Returns the identifier unchanged.
pub fn check_ident(name: &str) -> Result<&str, irondb_drivers::DriverError> {
    if !name.is_empty()
        && name.len() <= 128
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '.' || c == '$')
    {
        Ok(name)
    } else {
        Err(irondb_drivers::DriverError::Query(
            "invalid identifier".to_string(),
        ))
    }
}
/// Global quirk registry — populated at startup.
pub fn quirks() -> HashMap<&'static str, EngineQuirks> {
    let mut map = HashMap::new();

    // MySQL
    map.insert(
        "mysql",
        EngineQuirks {
            engine_id: "mysql",
            max_batch_rows: Some(1000),
            max_batch_args: Some(60000),
            placeholder_style: PlaceholderStyle::QuestionMark,
            supports_listen_notify: false,
            mssql_analyze_max_rows: None,
            mongo_readonly_commands: vec![],
            redis_command_guard: false,
            es_scroll_ttl_seconds: None,
            es_truncation_markers: vec![],
            sqlite_single_writer: false,
            sqlite_attach_external: false,
            extra_caps: CapabilityFlags::empty(),
        },
    );

    // PostgreSQL
    map.insert(
        "postgres",
        EngineQuirks {
            engine_id: "postgres",
            max_batch_rows: None,
            max_batch_args: None,
            placeholder_style: PlaceholderStyle::DollarNumber,
            supports_listen_notify: true,
            mssql_analyze_max_rows: None,
            mongo_readonly_commands: vec![],
            redis_command_guard: false,
            es_scroll_ttl_seconds: None,
            es_truncation_markers: vec![],
            sqlite_single_writer: false,
            sqlite_attach_external: false,
            extra_caps: CapabilityFlags::empty(),
        },
    );

    // MSSQL (tiberius)
    map.insert(
        "sqlserver",
        EngineQuirks {
            engine_id: "sqlserver",
            max_batch_rows: None,
            max_batch_args: None,
            placeholder_style: PlaceholderStyle::AtSymbol,
            supports_listen_notify: false,
            mssql_analyze_max_rows: Some(100_000),
            mongo_readonly_commands: vec![],
            redis_command_guard: false,
            es_scroll_ttl_seconds: None,
            es_truncation_markers: vec![],
            sqlite_single_writer: false,
            sqlite_attach_external: false,
            extra_caps: CapabilityFlags::empty(),
        },
    );

    // SQLite (via irondb-store)
    map.insert(
        "sqlite",
        EngineQuirks {
            engine_id: "sqlite",
            max_batch_rows: None,
            max_batch_args: None,
            placeholder_style: PlaceholderStyle::QuestionMark,
            supports_listen_notify: false,
            mssql_analyze_max_rows: None,
            mongo_readonly_commands: vec![],
            redis_command_guard: false,
            es_scroll_ttl_seconds: None,
            es_truncation_markers: vec![],
            sqlite_single_writer: true,
            sqlite_attach_external: true,
            extra_caps: CapabilityFlags::empty(),
        },
    );

    // MongoDB
    map.insert(
        "mongodb",
        EngineQuirks {
            engine_id: "mongodb",
            max_batch_rows: None,
            max_batch_args: None,
            placeholder_style: PlaceholderStyle::QuestionMark, // N/A for Mongo
            supports_listen_notify: false,
            mssql_analyze_max_rows: None,
            mongo_readonly_commands: vec![
                "aggregate".to_string(),
                "count".to_string(),
                "distinct".to_string(),
                "find".to_string(),
                "findOne".to_string(),
            ],
            redis_command_guard: false,
            es_scroll_ttl_seconds: None,
            es_truncation_markers: vec![],
            sqlite_single_writer: false,
            sqlite_attach_external: false,
            extra_caps: CapabilityFlags::empty(),
        },
    );

    // Redis
    map.insert(
        "redis",
        EngineQuirks {
            engine_id: "redis",
            max_batch_rows: None,
            max_batch_args: None,
            placeholder_style: PlaceholderStyle::QuestionMark, // N/A for Redis
            supports_listen_notify: false,
            mssql_analyze_max_rows: None,
            mongo_readonly_commands: vec![],
            redis_command_guard: true,
            es_scroll_ttl_seconds: None,
            es_truncation_markers: vec![],
            sqlite_single_writer: false,
            sqlite_attach_external: false,
            extra_caps: CapabilityFlags::empty(),
        },
    );

    // Elasticsearch
    map.insert(
        "elasticsearch",
        EngineQuirks {
            engine_id: "elasticsearch",
            max_batch_rows: None,
            max_batch_args: None,
            placeholder_style: PlaceholderStyle::QuestionMark, // N/A for ES
            supports_listen_notify: false,
            mssql_analyze_max_rows: None,
            mongo_readonly_commands: vec![],
            redis_command_guard: false,
            es_scroll_ttl_seconds: Some(60),
            es_truncation_markers: vec!["… [truncated]".to_string()],
            sqlite_single_writer: false,
            sqlite_attach_external: false,
            extra_caps: CapabilityFlags::empty(),
        },
    );

    // Long-tail (DRV-04): mysql-wire compat family shares MySQL limits/placeholders.
    for engine in ["tidb", "goldendb", "oceanbase", "doris", "starrocks", "sphinx"] {
        map.insert(
            engine,
            EngineQuirks {
                engine_id: engine,
                max_batch_rows: Some(1000),
                max_batch_args: Some(60000),
                placeholder_style: PlaceholderStyle::QuestionMark,
                supports_listen_notify: false,
                mssql_analyze_max_rows: None,
                mongo_readonly_commands: vec![],
                redis_command_guard: false,
                es_scroll_ttl_seconds: None,
                es_truncation_markers: vec![],
                sqlite_single_writer: false,
                sqlite_attach_external: false,
                extra_caps: CapabilityFlags::empty(),
            },
        );
    }

    // Long-tail: pg-wire compat family shares Postgres placeholders.
    for engine in ["cockroach", "timescale", "kingbase", "highgo", "vastbase", "opengauss", "gaussdb", "gbase8c"] {
        map.insert(
            engine,
            EngineQuirks {
                engine_id: engine,
                max_batch_rows: None,
                max_batch_args: None,
                placeholder_style: PlaceholderStyle::DollarNumber,
                supports_listen_notify: false,
                mssql_analyze_max_rows: None,
                mongo_readonly_commands: vec![],
                redis_command_guard: false,
                es_scroll_ttl_seconds: None,
                es_truncation_markers: vec![],
                sqlite_single_writer: false,
                sqlite_attach_external: false,
                extra_caps: CapabilityFlags::empty(),
            },
        );
    }

    map
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mysql_quirk_limits() {
        let q = quirks();
        let mysql = q.get("mysql").unwrap();
        assert_eq!(mysql.max_batch_rows, Some(1000));
        assert_eq!(mysql.max_batch_args, Some(60000));
        assert_eq!(mysql.placeholder_style, PlaceholderStyle::QuestionMark);
    }

    #[test]
    fn postgres_dollar_placeholders() {
        let q = quirks();
        let pg = q.get("postgres").unwrap();
        assert_eq!(pg.placeholder_style, PlaceholderStyle::DollarNumber);
        assert!(pg.supports_listen_notify);
    }

    #[test]
    fn mssql_analyze_budget() {
        let q = quirks();
        let mssql = q.get("sqlserver").unwrap();
        assert_eq!(mssql.mssql_analyze_max_rows, Some(100_000));
        assert_eq!(mssql.placeholder_style, PlaceholderStyle::AtSymbol);
    }

    #[test]
    fn sqlite_single_writer() {
        let q = quirks();
        let sqlite = q.get("sqlite").unwrap();
        assert!(sqlite.sqlite_single_writer);
        assert!(sqlite.sqlite_attach_external);
    }

    #[test]
    fn mongo_readonly_allowlist() {
        let q = quirks();
        let mongo = q.get("mongodb").unwrap();
        assert!(mongo.mongo_readonly_commands.contains(&"aggregate".to_string()));
        assert!(mongo.mongo_readonly_commands.contains(&"count".to_string()));
    }

    #[test]
    fn redis_command_guard() {
        let q = quirks();
        let redis = q.get("redis").unwrap();
        assert!(redis.redis_command_guard);
    }

    #[test]
    fn check_ident_rejects_injection() {
        assert!(check_ident("users").is_ok());
        assert!(check_ident("main.users").is_ok());
        assert!(check_ident("users; DROP TABLE users--").is_err());
        assert!(check_ident("x' OR '1'='1").is_err());
        assert!(check_ident("").is_err());
    }

    #[test]
    fn es_truncation_markers() {
        let q = quirks();
        let es = q.get("elasticsearch").unwrap();
        assert!(es.es_truncation_markers.contains(&"… [truncated]".to_string()));
        assert_eq!(es.es_scroll_ttl_seconds, Some(60));
    }
}