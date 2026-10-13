//! Frozen `DBQuery` + `DBQueryWithCancel` bridge commands — thin wrappers over
//! `AppFacade::query`, plus the `db_cancel` CancellationToken registry.
//!
//! Cancel protocol: each `DBQueryWithCancel` run registers
//! `query_id → CancellationToken` in a `Mutex<HashMap>`; `db_cancel` removes
//! and fires the token. The token is dropped (lock released) before awaiting.
//! Timeout is enforced facade-side via `tokio::time::timeout`. Every failure
//! stays inside the `QueryResult` envelope (success:false) — never `Err`.

use std::collections::HashMap;
use std::sync::LazyLock;
use std::time::Duration;

use irondb_contracts::QueryResult;
use irondb_facade::{sanitize_dsn, SharedFacade};
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

/// Default caps when the caller passes none.
pub const DEFAULT_MAX_ROWS: usize = 1000;
pub const DEFAULT_TIMEOUT_MS: u64 = 30_000;

static CANCEL_REGISTRY: LazyLock<Mutex<HashMap<String, CancellationToken>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

fn failure(message: impl Into<String>) -> QueryResult {
    QueryResult {
        success: false,
        message: sanitize_dsn(&message.into()),
        ..Default::default()
    }
}

fn validate_query(connection_id: &str, sql: &str, query_id: &str) -> Result<(), QueryResult> {
    if connection_id.trim().is_empty() || connection_id.len() > 256 {
        return Err(failure("invalid connection_id"));
    }
    if query_id.trim().is_empty() || query_id.len() > 128 {
        return Err(failure("invalid query_id"));
    }
    if sql.trim().is_empty() || sql.len() > 1024 * 1024 {
        return Err(failure("invalid sql"));
    }
    Ok(())
}

fn truncate_rows(mut res: QueryResult, max_rows: usize) -> QueryResult {
    if max_rows == 0 {
        return res;
    }
    if let Some(data) = res.data.as_mut().and_then(|d| d.as_object_mut()) {
        if let Some(rows) = data.get_mut("rows").and_then(|r| r.as_array_mut()) {
            if rows.len() > max_rows {
                rows.truncate(max_rows);
            }
        }
    }
    res
}

/// Testable core shared by the plain query command (no cancel/timeout).
pub async fn db_query_inner(
    facade: &SharedFacade,
    connection_id: &str,
    sql: &str,
) -> QueryResult {
    facade.query(connection_id, sql).await
}

/// Testable core: cancellable query with facade-side timeout + maxRows.
/// Registers the token, drops the lock, then awaits.
pub async fn db_query_with_cancel_inner(
    facade: &SharedFacade,
    connection_id: &str,
    sql: &str,
    query_id: &str,
    max_rows: Option<usize>,
    timeout_ms: Option<u64>,
) -> QueryResult {
    if let Err(e) = validate_query(connection_id, sql, query_id) {
        return e;
    }
    let token = CancellationToken::new();
    {
        let mut reg = CANCEL_REGISTRY.lock().await;
        reg.insert(query_id.to_string(), token.clone());
    } // lock dropped before await

    let timeout = Duration::from_millis(timeout_ms.unwrap_or(DEFAULT_TIMEOUT_MS));
    let query = facade.query(connection_id, sql);
    let result = tokio::select! {
        _ = token.cancelled() => failure("query cancelled"),
        r = tokio::time::timeout(timeout, query) => match r {
            Ok(res) => res,
            Err(_) => failure("query timeout"),
        },
    };

    {
        let mut reg = CANCEL_REGISTRY.lock().await;
        reg.remove(query_id);
    }
    truncate_rows(result, max_rows.unwrap_or(DEFAULT_MAX_ROWS))
}

/// Testable core: fire the token for `query_id`. Idempotent — unknown ids
/// still return success:false envelope, never `Err`.
pub async fn db_cancel_inner(query_id: &str) -> QueryResult {
    if query_id.trim().is_empty() || query_id.len() > 128 {
        return failure("invalid query_id");
    }
    let token = {
        let mut reg = CANCEL_REGISTRY.lock().await;
        reg.remove(query_id)
    }; // lock dropped; no await while held
    match token {
        Some(t) => {
            t.cancel();
            QueryResult {
                success: true,
                ..Default::default()
            }
        }
        None => failure("unknown query_id"),
    }
}

#[allow(non_snake_case)]
#[tauri::command]
pub async fn DBQuery(
    facade: tauri::State<'_, SharedFacade>,
    connection_id: String,
    sql: String,
) -> Result<QueryResult, String> {
    Ok(db_query_inner(&facade, &connection_id, &sql).await)
}

#[allow(non_snake_case)]
#[tauri::command]
pub async fn DBQueryWithCancel(
    facade: tauri::State<'_, SharedFacade>,
    connection_id: String,
    sql: String,
    query_id: Option<String>,
    max_rows: Option<usize>,
    timeout_ms: Option<u64>,
) -> Result<QueryResult, String> {
    let qid = query_id.unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    Ok(
        db_query_with_cancel_inner(&facade, &connection_id, &sql, &qid, max_rows, timeout_ms)
            .await,
    )
}

#[tauri::command]
pub async fn db_cancel(query_id: String) -> Result<QueryResult, String> {
    Ok(db_cancel_inner(&query_id).await)
}
