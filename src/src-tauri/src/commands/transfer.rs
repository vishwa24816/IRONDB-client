//! `import_run` / `export_run` bridge commands over FakeDriver.
//!
//! Stubs returning a `{ job_id, status }` envelope inside the frozen
//! `QueryResult`; the frontend polls for progress. Export excludes secrets
//! by default (`include_secrets` must be explicitly true). No locks held.

use irondb_contracts::QueryResult;
use irondb_facade::sanitize_dsn;

fn failure(message: impl Into<String>) -> QueryResult {
    QueryResult {
        success: false,
        message: sanitize_dsn(&message.into()),
        ..Default::default()
    }
}

fn job_envelope(job_id: &str, status: &str, extra: serde_json::Value) -> QueryResult {
    let mut job = serde_json::json!({ "job_id": job_id, "status": status });
    if let (Some(o), Some(e)) = (job.as_object_mut(), extra.as_object()) {
        for (k, v) in e {
            o.insert(k.clone(), v.clone());
        }
    }
    QueryResult {
        success: true,
        data: Some(job),
        ..Default::default()
    }
}

fn validate_job(connection_id: &str, format: &str) -> Result<(), QueryResult> {
    if connection_id.trim().is_empty() || connection_id.len() > 256 {
        return Err(failure("invalid connection_id"));
    }
    match format {
        "csv" | "json" | "sql" => Ok(()),
        _ => Err(failure("unsupported format")),
    }
}

/// Testable core: start an import job; returns immediately pollable status.
pub fn import_run_inner(connection_id: &str, table: &str, format: &str) -> QueryResult {
    if let Err(e) = validate_job(connection_id, format) {
        return e;
    }
    if table.trim().is_empty() || table.len() > 256 {
        return failure("invalid table");
    }
    job_envelope(
        &format!("import-{}-{}", connection_id, table),
        "queued",
        serde_json::json!({ "table": table, "format": format }),
    )
}

/// Testable core: start an export job. Secrets are excluded unless the
/// caller explicitly passes `include_secrets: true` (T-03-13).
pub fn export_run_inner(
    connection_id: &str,
    table: &str,
    format: &str,
    include_secrets: bool,
) -> QueryResult {
    if let Err(e) = validate_job(connection_id, format) {
        return e;
    }
    if table.trim().is_empty() || table.len() > 256 {
        return failure("invalid table");
    }
    job_envelope(
        &format!("export-{}-{}", connection_id, table),
        "queued",
        serde_json::json!({
            "table": table,
            "format": format,
            "secretsExcluded": !include_secrets,
        }),
    )
}

#[tauri::command]
pub async fn import_run(
    connection_id: String,
    table: String,
    format: String,
) -> Result<QueryResult, String> {
    Ok(import_run_inner(&connection_id, &table, &format))
}

#[tauri::command]
pub async fn export_run(
    connection_id: String,
    table: String,
    format: String,
    include_secrets: Option<bool>,
) -> Result<QueryResult, String> {
    Ok(export_run_inner(
        &connection_id,
        &table,
        &format,
        include_secrets.unwrap_or(false),
    ))
}
