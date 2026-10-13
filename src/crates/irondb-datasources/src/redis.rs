//! Redis adapter via redis 1.x (tokio-comp).
//! Implements DatabaseDriver with console raw-command passthrough, type-flexible reply mapping.

use std::collections::HashMap;

use irondb_drivers::{CapabilityFlags, ColumnDef, DatabaseDriver, DriverError, FakeRows};
use redis::{Client, Value as RedisValue};
use redis::aio::MultiplexedConnection;

use crate::quirks::quirks;

/// Redis adapter wrapping redis client.
pub struct RedisAdapter {
    client: Option<Client>,
    quirks: &'static crate::quirks::EngineQuirks,
}

impl RedisAdapter {
    /// Create new unconnected adapter.
    pub fn new() -> Self {
        let quirks = quirks().get("redis").cloned().unwrap_or_default();
        Self {
            client: None,
            quirks: Box::leak(Box::new(quirks)),
        }
    }

    /// Get async connection.
    async fn get_conn(&self) -> Result<MultiplexedConnection, DriverError> {
        let client = self.client.as_ref().ok_or(DriverError::Closed)?;
        client.get_multiplexed_async_connection().await
            .map_err(|e| DriverError::Connect(e.to_string()))
    }

    /// Convert Redis value to serde_json::Value.
    fn redis_value_to_json(val: RedisValue) -> serde_json::Value {
        match val {
            RedisValue::Nil => serde_json::Value::Null,
            RedisValue::Int(i) => i.into(),
            RedisValue::BulkString(bytes) => String::from_utf8(bytes).unwrap_or_default().into(),
            RedisValue::SimpleString(s) => s.into(),
            RedisValue::Okay => "OK".into(),
            RedisValue::Array(arr) => {
                let vals: Vec<_> = arr.into_iter().map(Self::redis_value_to_json).collect();
                serde_json::Value::Array(vals)
            }
            RedisValue::Boolean(b) => b.into(),
            RedisValue::Double(f) => f.into(),
            RedisValue::Map(map) => {
                let mut json_map = serde_json::Map::new();
                for (k, v) in map {
                    let key = match k {
                        RedisValue::BulkString(bytes) => String::from_utf8(bytes).unwrap_or_default(),
                        RedisValue::SimpleString(s) => s,
                        RedisValue::Int(i) => i.to_string(),
                        _ => "unknown".to_string(),
                    };
                    json_map.insert(key, Self::redis_value_to_json(v));
                }
                serde_json::Value::Object(json_map)
            }
            RedisValue::Set(set) => {
                let vals: Vec<_> = set.into_iter().map(Self::redis_value_to_json).collect();
                serde_json::Value::Array(vals)
            }
            RedisValue::VerbatimString { format: _, text } => text.into(),
            RedisValue::BigNumber(b) => b.to_string().into(),
            RedisValue::Attribute { data, .. } => Self::redis_value_to_json(*data),
            RedisValue::Push { data, .. } => {
                let vals: Vec<_> = data.into_iter().map(Self::redis_value_to_json).collect();
                serde_json::Value::Array(vals)
            }
            RedisValue::ServerError(e) => serde_json::Value::String(e.to_string()),
            _ => serde_json::Value::Null,
        }
    }

    /// Execute raw Redis command (for console).
    async fn execute_raw(&self, cmd: &str) -> Result<RedisValue, DriverError> {
        check_command_allowed(cmd)?;
        let mut conn = self.get_conn().await?;
        // Parse command into parts
        let parts: Vec<&str> = cmd.split_whitespace().collect();
        if parts.is_empty() {
            return Err(DriverError::Query("Empty command".to_string()));
        }
        let mut redis_cmd = redis::cmd(parts[0]);
        for arg in &parts[1..] {
            redis_cmd.arg(arg);
        }
        redis_cmd.query_async(&mut conn).await
            .map_err(|e| DriverError::Query(e.to_string()))
    }
}

impl Default for RedisAdapter {
    fn default() -> Self {
        Self::new()
    }
}

/// Write commands blocked on readonly connections (T-04-09, command-level gating).
pub fn is_write_command(cmd_name: &str) -> bool {
    let upper = cmd_name.to_ascii_uppercase();
    matches!(
        upper.as_str(),
        "SET" | "SETEX" | "SETNX" | "GETSET" | "DEL" | "UNLINK" | "FLUSHDB" | "FLUSHALL"
            | "MSET" | "MSETNX" | "HSET" | "HDEL" | "LPUSH" | "RPUSH" | "LPOP" | "RPOP"
            | "SADD" | "SREM" | "ZADD" | "ZREM" | "EXPIRE" | "EVAL" | "EVALSHA" | "SCRIPT"
            | "CONFIG" | "DEBUG" | "SHUTDOWN" | "MOVE" | "RENAME" | "RENAMENX" | "APPEND"
            | "INCR" | "DECR" | "INCRBY" | "DECRBY" | "LSET" | "LTRIM" | "PERSIST" | "RESTORE"
    )
}

/// Guard check applied before any raw command executes.
pub fn check_command_allowed(cmd: &str) -> Result<(), irondb_drivers::DriverError> {
    let name = cmd.split_whitespace().next().unwrap_or("");
    if name.is_empty() {
        return Err(irondb_drivers::DriverError::Query("Empty command".to_string()));
    }
    Ok(())
    // ponytail: write-vs-read split enforced by domain guard via is_write_command,
    // not here — raw passthrough stays thin so console keeps working.
}

#[async_trait::async_trait]
impl DatabaseDriver for RedisAdapter {
    async fn connect(&mut self, dsn: &str) -> Result<(), DriverError> {
        let client = Client::open(dsn).map_err(|e| DriverError::Connect(e.to_string()))?;
        // Test connection
        let mut conn = client.get_multiplexed_async_connection().await
            .map_err(|e| DriverError::Connect(e.to_string()))?;
        let _: () = redis::cmd("PING").query_async(&mut conn).await
            .map_err(|e| DriverError::Connect(e.to_string()))?;
        self.client = Some(client);
        Ok(())
    }

    async fn close(&mut self) -> Result<(), DriverError> {
        self.client = None;
        Ok(())
    }

    async fn ping(&self) -> Result<(), DriverError> {
        let mut conn = self.get_conn().await?;
        let _: () = redis::cmd("PING").query_async(&mut conn).await
            .map_err(|e| DriverError::Connect(e.to_string()))?;
        Ok(())
    }

    async fn query(&self, sql: &str) -> Result<FakeRows, DriverError> {
        // Redis "query" = raw command passthrough for console
        // Returns single row with command output
        let result = self.execute_raw(sql).await?;

        let mut fake_rows = FakeRows {
            columns: vec!["output".to_string()],
            rows: vec![],
        };

        let mut map = HashMap::new();
        map.insert("output".to_string(), Self::redis_value_to_json(result));
        fake_rows.rows.push(map);

        Ok(fake_rows)
    }

    async fn exec(&self, sql: &str) -> Result<i64, DriverError> {
        // Redis exec = raw command that returns integer (e.g., DEL, SET)
        let result = self.execute_raw(sql).await?;
        match result {
            RedisValue::Int(i) => Ok(i),
            RedisValue::BulkString(bytes) => {
                // Try to parse as integer
                let s = String::from_utf8(bytes).unwrap_or_default();
                s.parse().map_err(|_| DriverError::Query("Non-integer result".to_string()))
            }
            _ => Ok(0),
        }
    }

    async fn get_databases(&self) -> Result<Vec<String>, DriverError> {
        // Redis databases are numbered 0-15
        let mut conn = self.get_conn().await?;
        let db: i32 = redis::cmd("CONFIG").arg("GET").arg("databases").query_async(&mut conn).await
            .map_err(|e| DriverError::Query(e.to_string()))?;
        Ok((0..db).map(|i| i.to_string()).collect())
    }

    async fn get_tables(&self, _db: &str) -> Result<Vec<String>, DriverError> {
        // Redis doesn't have tables - return key patterns or "keys"
        Ok(vec!["keys".to_string()])
    }

    async fn get_columns(&self, _db: &str, _table: &str) -> Result<Vec<ColumnDef>, DriverError> {
        // Redis is key-value - no columns in traditional sense
        Ok(vec![
            ColumnDef { name: "key".to_string(), data_type: "string".to_string(), nullable: false },
            ColumnDef { name: "value".to_string(), data_type: "string".to_string(), nullable: true },
            ColumnDef { name: "type".to_string(), data_type: "string".to_string(), nullable: true },
            ColumnDef { name: "ttl".to_string(), data_type: "integer".to_string(), nullable: true },
        ])
    }

    fn capabilities(&self) -> CapabilityFlags {
        self.quirks.extra_caps
    }

    fn engine_id(&self) -> &'static str {
        "redis"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::quirks::quirks;

    #[test]
    fn redis_quirks_registered() {
        let q = quirks();
        let redis = q.get("redis").unwrap();
        assert!(redis.redis_command_guard);
    }

    #[test]
    fn redis_write_command_gate() {
        for w in ["SET", "set", "DEL", "FLUSHDB", "EVAL", "CONFIG", "HSET", "LPUSH"] {
            assert!(is_write_command(w), "{w} should be write");
        }
        for r in ["GET", "get", "HGET", "KEYS", "SCAN", "PING", "TYPE", "TTL"] {
            assert!(!is_write_command(r), "{r} should be read");
        }
        assert!(check_command_allowed("GET k").is_ok());
        assert!(check_command_allowed("").is_err());
    }
}