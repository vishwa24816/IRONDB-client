//! Facade errors — DSN material is sanitized before it reaches `message`.

use thiserror::Error;

/// Strip password/userinfo from a DSN-ish string: `scheme://user:pass@host` → `scheme://***@host`.
pub fn sanitize_dsn(raw: &str) -> String {
    if let Some(at) = raw.rfind('@') {
        if let Some(scheme) = raw.find("://") {
            return format!("{}://***@{}", &raw[..scheme], &raw[at + 1..]);
        }
        return "***@redacted".to_string();
    }
    if raw.contains("password=") {
        return raw
            .split(';')
            .filter(|kv| !kv.trim_start().to_lowercase().starts_with("password="))
            .collect::<Vec<_>>()
            .join(";")
            + ";password=***";
    }
    raw.to_string()
}

#[derive(Debug, Error)]
pub enum FacadeError {
    #[error("unknown connection: {0}")]
    UnknownConnection(String),
    #[error("unknown engine: {0}")]
    UnknownEngine(String),
    #[error("store error: {0}")]
    Store(String),
    #[error("driver error: {0}")]
    Driver(String),
    #[error("connect failed for engine {engine}")]
    Connect { engine: String },
}
