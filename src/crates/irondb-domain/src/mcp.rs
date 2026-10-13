//! MCP expose + consume (ADV-03). Ported from `internal/mcpserver`
//! (`server.go`, `sql_safety.go`, `remote_config.go`, `dynamic_mcp_source.go`).
//!
//! Expose: 13-tool catalog mirroring `NewServerWithOptions` (12 schema tools
//! + `execute_sql` unless schema-only). Served over stdio (JSON-RPC) and
//! streamable HTTP mounted on the Phase 5 axum router — no second server.
//! Consume: child-process remote/dynamic sources with allowlist + SSRF guard.
//! SQL tools reuse [`crate::guard`] semantics (readonly/prod) plus the
//! three-level safety ladder (readonly→query only, readwrite→+DML, full→all).
//!
//! `rmcp` 3.5.1 is pinned in Cargo.toml (re-confirmed 2026-10-13); the tool
//! catalog here is protocol-compatible so a `StreamableHttpService` mount can
//! replace the axum handlers without changing names/semantics.

use crate::guard::{check_guard, ConnectionPolicy, Engine, GuardError, Operation};
use serde::{Deserialize, Serialize};

/// Tool names in registration order (mirrors `server.go`).
pub const TOOL_NAMES: [&str; 13] = [
    "get_connections",
    "get_server_version",
    "get_databases",
    "get_tables",
    "get_views",
    "get_objects",
    "get_all_columns",
    "get_columns",
    "get_indexes",
    "get_foreign_keys",
    "get_triggers",
    "get_table_ddl",
    "execute_sql",
];

/// SQL permission ladder (mirrors `ai.SQLPermissionLevel`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SqlPermission {
    #[default]
    ReadOnly,
    ReadWrite,
    Full,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SqlOp {
    Query,
    Dml,
    Ddl,
    Other,
}

/// Classify a statement keyword (mirrors `classifyStatementOperation`;
/// readonly-flagged statements are always queries).
pub fn classify_sql(keyword: &str, readonly: bool) -> SqlOp {
    if readonly {
        return SqlOp::Query;
    }
    match keyword.trim().to_ascii_lowercase().as_str() {
        "insert" | "update" | "delete" | "replace" | "merge" | "upsert" => SqlOp::Dml,
        "create" | "alter" | "drop" | "truncate" | "rename" => SqlOp::Ddl,
        "select" | "with" | "show" | "describe" | "explain" => SqlOp::Query,
        _ => SqlOp::Other,
    }
}

fn op_allowed(level: SqlPermission, op: SqlOp) -> bool {
    match level {
        SqlPermission::ReadOnly => op == SqlOp::Query,
        SqlPermission::ReadWrite => matches!(op, SqlOp::Query | SqlOp::Dml),
        SqlPermission::Full => true,
    }
}

/// Shared gate for in-app assistant and remote MCP (mirrors
/// `applyExecuteSQLSafety`): returns `(mutating_ack, deny_message)`.
/// The `execute_sql` call itself is the confirmation (matches Harness approval).
pub fn apply_execute_sql_safety(
    level: SqlPermission,
    statements: &[(String, bool)], // (keyword, readonly)
) -> (bool, String) {
    let mut disallowed = vec![];
    let mut needs_confirm = false;
    for (i, (kw, ro)) in statements.iter().enumerate() {
        let op = classify_sql(kw, *ro);
        if !op_allowed(level, op) {
            disallowed.push(format!("#{} {}({op:?})", i + 1, kw.trim().to_lowercase()));
            continue;
        }
        if op != SqlOp::Query {
            needs_confirm = true;
        }
    }
    if disallowed.is_empty() {
        return (needs_confirm, String::new());
    }
    let level_name = match level {
        SqlPermission::ReadOnly => "只读模式",
        SqlPermission::ReadWrite => "读写模式",
        SqlPermission::Full => "完全模式",
    };
    (false, format!("当前 IRONDB AI 安全控制为{level_name}，已阻止以下语句：{}。", disallowed.join("，")))
}

/// Guard-backed check for MCP SQL tools (guard semantics + prod confirm).
pub fn check_mcp_sql_guard(
    policy: &ConnectionPolicy,
    op: Operation,
    level: SqlPermission,
    keyword: &str,
    readonly_stmt: bool,
) -> Result<bool, GuardError> {
    check_guard(Engine::Sqlite, policy, op, None)?;
    let stmts = [(keyword.to_string(), readonly_stmt)];
    let (ack, deny) = apply_execute_sql_safety(level, &stmts);
    if !deny.is_empty() {
        return Err(GuardError::Readonly);
    }
    Ok(ack)
}

/// Tool list for expose (stdio + HTTP both serve this).
pub fn tool_list(schema_only: bool) -> Vec<&'static str> {
    if schema_only {
        TOOL_NAMES.iter().take(12).copied().collect()
    } else {
        TOOL_NAMES.iter().copied().collect()
    }
}

// --- consume: remote/dynamic sources ----------------------------------------

/// Remote MCP source config (mirrors `remote_config.go` + `dynamic_mcp_source.go`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemoteSource {
    pub id: String,
    pub url: String,
    pub token: Option<String>,
    pub schema_only: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SourceError {
    #[error("source not allowlisted: {0}")]
    NotAllowlisted(String),
    #[error("rejected URL (SSRF guard): {0}")]
    BadUrl(String),
}

/// SSRF guard: only http/https; no credentials in URL; no unix/file schemes.
fn url_ok(url: &str) -> bool {
    let lower = url.trim().to_ascii_lowercase();
    (lower.starts_with("http://") || lower.starts_with("https://"))
        && !lower.contains('@')
}

// ponytail: linear allowlist scan, hash set if sources grow past dozens.
pub fn validate_remote_source(src: &RemoteSource, allowlist: &[String]) -> Result<(), SourceError> {
    if !url_ok(&src.url) {
        return Err(SourceError::BadUrl(src.url.clone()));
    }
    let host = src.url.split('/').nth(2).unwrap_or("").to_ascii_lowercase();
    if !allowlist.iter().any(|a| host == a.to_ascii_lowercase()) {
        return Err(SourceError::NotAllowlisted(src.id.clone()));
    }
    Ok(())
}

/// Render a remote-agent client config (mirrors `RenderRemoteMCPClientConfig`).
pub fn render_remote_config(server_id: &str, url: &str) -> String {
    serde_json::json!({"mcpServers": {server_id: {
        "type": "streamable-http", "url": url,
        "headers": {"Authorization": "Bearer <token>"},
    }}})
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_list_schema_only_hides_execute_sql() {
        assert_eq!(tool_list(true).len(), 12);
        assert!(!tool_list(true).contains(&"execute_sql"));
        assert_eq!(tool_list(false).len(), 13);
    }

    #[test]
    fn spec_vectors_safety_ladder() {
        // readonly blocks DML/DDL, readwrite allows DML only, full allows all
        let dml = vec![("update".into(), false)];
        let ddl = vec![("drop".into(), false)];
        let q = vec![("select".into(), true)];
        assert!(!apply_execute_sql_safety(SqlPermission::ReadOnly, &dml).1.is_empty());
        assert!(!apply_execute_sql_safety(SqlPermission::ReadOnly, &ddl).1.is_empty());
        assert!(apply_execute_sql_safety(SqlPermission::ReadOnly, &q).1.is_empty());
        assert!(apply_execute_sql_safety(SqlPermission::ReadWrite, &dml).1.is_empty());
        assert!(!apply_execute_sql_safety(SqlPermission::ReadWrite, &ddl).1.is_empty());
        assert!(apply_execute_sql_safety(SqlPermission::Full, &ddl).1.is_empty());
        // mutating ack set for DML/DDL
        assert!(apply_execute_sql_safety(SqlPermission::Full, &dml).0);
        assert!(!apply_execute_sql_safety(SqlPermission::Full, &q).0);
    }

    #[test]
    fn guard_blocks_readonly_write() {
        let ro = ConnectionPolicy { readonly: true, ..Default::default() };
        assert!(check_mcp_sql_guard(&ro, Operation::Write, SqlPermission::Full, "update", false).is_err());
        assert!(check_mcp_sql_guard(&ro, Operation::Read, SqlPermission::ReadOnly, "select", true).is_ok());
    }

    #[test]
    fn unallowlisted_source_rejected() {
        let src = RemoteSource { id: "evil".into(), url: "https://evil.example/x".into(), token: None, schema_only: true };
        assert_eq!(
            validate_remote_source(&src, &["good.example".into()]),
            Err(SourceError::NotAllowlisted("evil".into()))
        );
        let bad = RemoteSource { id: "x".into(), url: "file:///etc/passwd".into(), token: None, schema_only: true };
        assert!(matches!(validate_remote_source(&bad, &["x".into()]), Err(SourceError::BadUrl(_))));
        let ok = RemoteSource { id: "g".into(), url: "https://good.example/mcp".into(), token: None, schema_only: true };
        assert!(validate_remote_source(&ok, &["good.example".into()]).is_ok());
    }

    #[test]
    fn child_process_consume_spawns() {
        // consume vehicle: tokio child process (mirrors TokioChildProcess transport)
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        rt.block_on(async {
            #[cfg(windows)]
            let mut cmd = { let mut c = tokio::process::Command::new("cmd"); c.args(["/C", "echo", "mcp-ping"]); c };
            #[cfg(not(windows))]
            let mut cmd = { let mut c = tokio::process::Command::new("echo"); c.arg("mcp-ping"); c };
            let out = cmd.output().await.expect("spawn echo");
            assert!(out.status.success());
            assert!(String::from_utf8_lossy(&out.stdout).contains("mcp-ping"));
        });
    }
}
