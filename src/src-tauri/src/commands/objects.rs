//! `db_objects` + `db_table_ddl` bridge commands — lazy object-browser backend.
//!
//! Returns canned FakeDriver metadata inside the frozen `QueryResult`
//! envelope. Every level is gated by Phase 2 `CapabilityFlags` bits passed
//! as `caps` (u32): a level whose required bit is missing yields an empty
//! `children` list, so the frontend never renders unsupported nodes.
//! No locks, no awaits — nothing to hold across `.await`.

use irondb_contracts::QueryResult;
use irondb_facade::sanitize_dsn;

/// Phase 2 `CapabilityFlags` bit for schema-bearing engines (SCHEMAS = 0x04).
/// Kept as a local const: this crate does not depend on irondb-drivers.
const CAP_SCHEMAS: u32 = 0x04;

/// Levels the tree can expand. `schemas` needs SCHEMAS, the rest are core.
fn needs_schemas(level: &str) -> bool {
    level == "schemas"
}

fn failure(message: impl Into<String>) -> QueryResult {
    QueryResult {
        success: false,
        message: sanitize_dsn(&message.into()),
        ..Default::default()
    }
}

fn ok_children(children: Vec<serde_json::Value>) -> QueryResult {
    QueryResult {
        success: true,
        data: Some(serde_json::json!({ "children": children })),
        ..Default::default()
    }
}

fn node(kind: &str, name: &str) -> serde_json::Value {
    serde_json::json!({ "kind": kind, "name": name })
}

/// Testable core: children for `level` under `parent`, capability-gated.
pub fn db_objects_inner(
    connection_id: &str,
    level: &str,
    parent: Option<&str>,
    caps: u32,
) -> QueryResult {
    if connection_id.trim().is_empty() || connection_id.len() > 256 {
        return failure("invalid connection_id");
    }
    if needs_schemas(level) && caps & CAP_SCHEMAS == 0 {
        return ok_children(vec![]);
    }
    match level {
        "databases" => ok_children(vec![node("database", "main")]),
        "schemas" => ok_children(vec![node("schema", "public")]),
        "tables" => match parent {
            Some(_) => ok_children(vec![node("table", "users"), node("table", "orders")]),
            None => failure("tables level needs a parent database"),
        },
        "columns" => match parent {
            Some("users") => ok_children(vec![node("column", "id"), node("column", "name")]),
            Some("orders") => ok_children(vec![node("column", "id"), node("column", "user_id")]),
            Some(_) => ok_children(vec![]),
            None => failure("columns level needs a parent table"),
        },
        _ => failure("unknown level"),
    }
}

/// Testable core: read-only DDL preview for one table.
pub fn db_table_ddl_inner(connection_id: &str, table: &str) -> QueryResult {
    if connection_id.trim().is_empty() || connection_id.len() > 256 {
        return failure("invalid connection_id");
    }
    if table.trim().is_empty() || table.len() > 256 {
        return failure("invalid table");
    }
    let ddl = match table {
        "users" => "CREATE TABLE users (\n  id INTEGER PRIMARY KEY,\n  name TEXT NOT NULL\n);",
        "orders" => {
            "CREATE TABLE orders (\n  id INTEGER PRIMARY KEY,\n  user_id INTEGER NOT NULL\n);"
        }
        _ => return failure("unknown table"),
    };
    QueryResult {
        success: true,
        data: Some(serde_json::json!({ "table": table, "ddl": ddl })),
        ..Default::default()
    }
}

#[tauri::command]
pub async fn db_objects(
    connection_id: String,
    level: String,
    parent: Option<String>,
    caps: Option<u32>,
) -> Result<QueryResult, String> {
    Ok(db_objects_inner(
        &connection_id,
        &level,
        parent.as_deref(),
        caps.unwrap_or(0),
    ))
}

#[tauri::command]
pub async fn db_table_ddl(connection_id: String, table: String) -> Result<QueryResult, String> {
    Ok(db_table_ddl_inner(&connection_id, &table))
}
