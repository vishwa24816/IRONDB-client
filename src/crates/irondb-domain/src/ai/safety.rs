//! Safety: SQL gating + result masking + redacted logging (T-06-05, AI-SPEC-R §6).
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SqlOp { Query, Dml, Ddl, Other }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SafetyResult {
    pub allowed: bool,
    pub operation_type: SqlOp,
    pub requires_confirm: bool,
    pub warning: String,
}

pub fn classify_sql(sql: &str) -> SqlOp {
    let s = sql.trim_start().to_uppercase();
    let w = s.split_whitespace().next().unwrap_or("");
    match w {
        "SELECT" | "WITH" | "EXPLAIN" | "SHOW" | "DESCRIBE" | "DESC" => SqlOp::Query,
        "INSERT" | "UPDATE" | "DELETE" | "MERGE" | "REPLACE" => SqlOp::Dml,
        "CREATE" | "ALTER" | "DROP" | "TRUNCATE" => SqlOp::Ddl,
        _ => SqlOp::Other,
    }
}

/// Never execute AI output blindly: queries pass, DML needs confirm, DDL blocked by default.
pub fn gate(sql: &str, readonly: bool) -> SafetyResult {
    match classify_sql(sql) {
        SqlOp::Query => SafetyResult { allowed: true, operation_type: SqlOp::Query, requires_confirm: false, warning: String::new() },
        SqlOp::Dml if !readonly => SafetyResult { allowed: true, operation_type: SqlOp::Dml, requires_confirm: true, warning: "AI-generated write needs confirmation".into() },
        SqlOp::Dml => SafetyResult { allowed: false, operation_type: SqlOp::Dml, requires_confirm: true, warning: "read-only connection blocks writes".into() },
        op => SafetyResult { allowed: false, operation_type: op, requires_confirm: true, warning: "AI output must not execute blindly".into() },
    }
}

/// Mask secrets before display/log: redact key-like tokens + long base64 blobs.
pub fn mask_text(s: &str, secrets: &[&str]) -> String {
    let mut out = s.to_string();
    for sec in secrets {
        if !sec.is_empty() { out = out.replace(sec, "***"); }
    }
    // sk-/xox-/AKIA-shaped tokens
    let re = regex_lite(out.clone());
    re
}

fn regex_lite(s: String) -> String {
    // ponytail: hand-rolled scan, no regex dep for one pattern class.
    let mut out = s;
    for pat in ["sk-", "xox-", "AKIA", "ghp_", "gsk_"] {
        loop {
            let Some(i) = out.find(pat) else { break };
            let end = out[i..].char_indices().take_while(|(k, c)| *k < pat.len() + 40 && (c.is_alphanumeric() || *c == '_' || *c == '-')).map(|(k, c)| i + k + c.len_utf8()).last().unwrap_or(i + pat.len());
            out.replace_range(i..end, "***");
        }
    }
    out
}

/// Hosted-plan limits (hosted_limits.go port): max output chars per call.
pub const MAX_OUTPUT_CHARS: usize = 32_000;
pub fn enforce_output_limit(content: &str) -> (String, bool) {
    if content.len() <= MAX_OUTPUT_CHARS { return (content.to_string(), false); }
    let mut end = MAX_OUTPUT_CHARS;
    while !content.is_char_boundary(end) { end -= 1; }
    (format!("{}…[truncated at hosted limit]", &content[..end]), true)
}

/// Redacted log line: URL with ?key=... and secret substrings scrubbed.
pub fn redact_for_log(s: &str, secrets: &[&str]) -> String {
    let mut out = mask_text(s, secrets);
    let mut from = 0;
    while let Some(rel) = out[from..].find("key=") {
        let start = from + rel + 4;
        let end = out[start..].find(|c: char| c.is_whitespace() || c == '&').map(|k| start + k).unwrap_or(out.len());
        out.replace_range(start..end, "REDACTED");
        from = start + "REDACTED".len();
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn gating_matrix() {
        assert!(gate("SELECT 1", false).allowed);
        let d = gate("DELETE FROM t", false);
        assert!(d.allowed && d.requires_confirm);
        assert!(!gate("DELETE FROM t", true).allowed);
        assert!(!gate("DROP TABLE t", false).allowed);
    }
    #[test] fn masking_no_secret_leak() {
        let secret = "sk-test-abc123XYZ";
        let out = mask_text(&format!("key is {secret} done"), &[secret]);
        assert!(!out.contains(secret));
        let out2 = mask_text("token AKIAIOSFODNN7EXAMPLE here", &[]);
        assert!(!out2.contains("AKIA"));
        let log = redact_for_log("POST https://x/v1beta/models/m:generateContent?key=REALKEY123 ok", &["REALKEY123"]);
        assert!(!log.contains("REALKEY123") && log.contains("REDACTED"));
    }
    #[test] fn hosted_limit_truncates() {
        let big = "x".repeat(MAX_OUTPUT_CHARS + 10);
        let (out, trunc) = enforce_output_limit(&big);
        assert!(trunc && out.contains("truncated at hosted limit"));
        let (small, t2) = enforce_output_limit("ok");
        assert!(!t2 && small == "ok");
    }
}
