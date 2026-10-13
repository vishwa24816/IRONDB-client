//! OS-keyring secret seam: `secret_store` / `secret_get` / `secret_delete`.
//!
//! Secrets cross the JS→Rust bridge exactly once (write-only). List payloads
//! and logs only ever carry `keyringRef: true` + a `••••••` placeholder.
//! Failures stay inside the `QueryResult` envelope (success:false), sanitized.

use irondb_contracts::QueryResult;
use irondb_facade::sanitize_dsn;

/// Service name for all keyring entries; keyed by `conn_id`.
pub const KEYRING_SERVICE: &str = "irondb-rs";
/// Masked placeholder shown in list payloads instead of the secret.
#[allow(dead_code)]
pub const MASKED: &str = "••••••";

fn failure(message: impl Into<String>) -> QueryResult {
    QueryResult {
        success: false,
        message: sanitize_dsn(&message.into()),
        ..Default::default()
    }
}

fn ok(message: impl Into<String>) -> QueryResult {
    QueryResult {
        success: true,
        message: message.into(),
        ..Default::default()
    }
}

fn validate_conn_id(conn_id: &str) -> Result<(), QueryResult> {
    if conn_id.trim().is_empty() || conn_id.len() > 256 {
        return Err(failure("invalid conn_id"));
    }
    Ok(())
}

/// Testable core: store `secret` for `conn_id` in the OS keyring.
pub fn secret_store_inner(conn_id: &str, secret: &str) -> QueryResult {
    if let Err(e) = validate_conn_id(conn_id) {
        return e;
    }
    if secret.is_empty() || secret.len() > 16 * 1024 {
        return failure("invalid secret length");
    }
    match keyring::Entry::new(KEYRING_SERVICE, conn_id) {
        Ok(entry) => match entry.set_password(secret) {
            Ok(()) => ok("stored"),
            Err(e) => failure(format!("keyring store failed: {}", e)),
        },
        Err(e) => failure(format!("keyring store failed: {}", e)),
    }
}

/// Testable core: read back the secret (connect-time use inside Rust only).
/// Never called from list payloads.
#[allow(dead_code)]
pub fn secret_get_inner(conn_id: &str) -> Result<String, QueryResult> {
    validate_conn_id(conn_id)?;
    match keyring::Entry::new(KEYRING_SERVICE, conn_id) {
        Ok(entry) => match entry.get_password() {
            Ok(pw) => Ok(pw),
            Err(e) => Err(failure(format!("keyring read failed: {}", e))),
        },
        Err(e) => Err(failure(format!("keyring read failed: {}", e))),
    }
}

/// Testable core: delete the secret (connection delete path).
pub fn secret_delete_inner(conn_id: &str) -> QueryResult {
    if let Err(e) = validate_conn_id(conn_id) {
        return e;
    }
    match keyring::Entry::new(KEYRING_SERVICE, conn_id) {
        Ok(entry) => match entry.delete_credential() {
            Ok(()) => ok("deleted"),
            Err(e) => failure(format!("keyring delete failed: {}", e)),
        },
        Err(e) => failure(format!("keyring delete failed: {}", e)),
    }
}

/// Masked list entry: the only shape list payloads may carry.
#[allow(dead_code)]
pub fn masked_ref(conn_id: &str) -> serde_json::Value {
    serde_json::json!({
        "conn_id": conn_id,
        "keyringRef": true,
        "secret": MASKED,
    })
}

#[tauri::command]
pub async fn secret_store(conn_id: String, secret: String) -> Result<QueryResult, String> {
    Ok(secret_store_inner(&conn_id, &secret))
}

#[tauri::command]
pub async fn secret_delete(conn_id: String) -> Result<QueryResult, String> {
    Ok(secret_delete_inner(&conn_id))
}
