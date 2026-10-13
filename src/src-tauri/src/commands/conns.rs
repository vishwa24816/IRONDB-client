//! `conn_list` / `conn_save` / `conn_delete` bridge commands.
//!
//! Non-secret payloads only (T-03-12): list/save responses carry
//! `{ keyringRef: true }` plus a `••••••` mask — never a password or DSN.
//! Secrets travel write-only via `secret_store`. All failures stay inside
//! the frozen `QueryResult` envelope.

use irondb_contracts::QueryResult;
use irondb_facade::sanitize_dsn;

/// Mask shown wherever a secret would appear. Never a real credential.
pub const MASKED: &str = "••••••";

fn failure(message: impl Into<String>) -> QueryResult {
    QueryResult {
        success: false,
        message: sanitize_dsn(&message.into()),
        ..Default::default()
    }
}

fn validate_id(id: &str) -> Result<(), QueryResult> {
    if id.trim().is_empty() || id.len() > 256 {
        return Err(failure("invalid connection id"));
    }
    Ok(())
}

/// Masked, non-secret view of a connection for list payloads.
pub fn masked_ref(id: &str, engine: &str, name: &str) -> serde_json::Value {
    serde_json::json!({
        "id": id,
        "engine": engine,
        "name": name,
        "password": MASKED,
        "keyringRef": true,
    })
}

/// Testable core: list payload shape (empty store for now — no secrets).
pub fn conn_list_inner() -> QueryResult {
    QueryResult {
        success: true,
        data: Some(serde_json::json!({ "connections": [] })),
        ..Default::default()
    }
}

/// Testable core: validate + echo the masked payload (persistence deferred).
pub fn conn_save_inner(id: &str, engine: &str, name: &str) -> QueryResult {
    if let Err(e) = validate_id(id) {
        return e;
    }
    if engine.trim().is_empty() || engine.len() > 64 {
        return failure("invalid engine");
    }
    if name.trim().is_empty() || name.len() > 256 {
        return failure("invalid name");
    }
    QueryResult {
        success: true,
        data: Some(serde_json::json!({ "connection": masked_ref(id, engine, name) })),
        ..Default::default()
    }
}

/// Testable core: validate id; secret cleanup happens via `secret_delete`.
pub fn conn_delete_inner(id: &str) -> QueryResult {
    if let Err(e) = validate_id(id) {
        return e;
    }
    QueryResult {
        success: true,
        ..Default::default()
    }
}

#[tauri::command]
pub async fn conn_list() -> Result<QueryResult, String> {
    Ok(conn_list_inner())
}

#[tauri::command]
pub async fn conn_save(id: String, engine: String, name: String) -> Result<QueryResult, String> {
    Ok(conn_save_inner(&id, &engine, &name))
}

#[tauri::command]
pub async fn conn_delete(id: String) -> Result<QueryResult, String> {
    Ok(conn_delete_inner(&id))
}
