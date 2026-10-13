//! Append-only audit log. Redacted: never records DSN/password.
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AuditEntry {
    pub seq: u64,
    pub principal: String,
    pub engine: String,
    /// Redacted SQL (literals + secrets stripped).
    pub sql: String,
    pub allowed: bool,
    pub note: String,
}

/// Redact string literals and `password=...`-style secrets.
pub fn redact_sql(sql: &str) -> String {
    let mut out = String::with_capacity(sql.len());
    let mut chars = sql.chars().peekable();
    let mut in_str = false;
    while let Some(c) = chars.next() {
        if in_str {
            if c == '\'' {
                in_str = false;
                out.push('\'');
            }
            continue;
        }
        if c == '\'' {
            in_str = true;
            out.push_str("'?'");
            continue;
        }
        out.push(c);
    }
    // Mask password/secret/dsn assignments.
    let mut redacted = out;
    for key in ["password", "passwd", "secret", "dsn", "token"] {
        let mut search = String::new();
        let _ = &mut search;
        let lower = redacted.to_lowercase();
        if let Some(pos) = lower.find(key) {
            let mut end = pos + key.len();
            let bytes = redacted.as_bytes();
            while end < bytes.len() && (bytes[end] == b' ' || bytes[end] == b'=' || bytes[end] == b':') {
                end += 1;
            }
            let mut val_end = end;
            while val_end < bytes.len() && !bytes[val_end].is_ascii_whitespace() && bytes[val_end] != b';' {
                val_end += 1;
            }
            if val_end > end {
                redacted.replace_range(end..val_end, "***");
            }
        }
    }
    redacted
}

#[derive(Debug, Default)]
pub struct AuditLog {
    entries: Vec<AuditEntry>,
}

impl AuditLog {
    pub fn append(&mut self, principal: &str, engine: &str, sql: &str, allowed: bool, note: &str) -> &AuditEntry {
        let seq = self.entries.len() as u64 + 1;
        self.entries.push(AuditEntry {
            seq,
            principal: principal.to_string(),
            engine: engine.to_string(),
            sql: redact_sql(sql),
            allowed,
            note: note.to_string(),
        });
        self.entries.last().unwrap()
    }

    pub fn entries(&self) -> &[AuditEntry] { &self.entries }
    pub fn len(&self) -> usize { self.entries.len() }
    pub fn is_empty(&self) -> bool { self.entries.is_empty() }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn append_is_sequential_and_redacted() {
        let mut log = AuditLog::default();
        log.append("alice", "mysql", "select * from t where pw='hunter2'", true, "ok");
        log.append("bob", "pg", "select 1", false, "blocked");
        assert_eq!(log.len(), 2);
        assert_eq!(log.entries()[0].seq, 1);
        assert!(!log.entries()[0].sql.contains("hunter2"));
        // append-only: no mutation API exists; len only grows
        assert!(log.entries()[1].seq > log.entries()[0].seq);
    }

    #[test]
    fn secrets_masked() {
        let r = redact_sql("connect password=s3cr3t;");
        assert!(!r.contains("s3cr3t"));
    }
}
