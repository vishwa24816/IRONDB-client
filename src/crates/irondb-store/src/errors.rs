use thiserror::Error;

/// Typed store errors. Never carries secret material (no DSN/password fields).
#[derive(Debug, Error)]
pub enum StoreError {
    #[error("failed to open store: {0}")]
    Open(String),
    #[error("failed to apply pragmas: {0}")]
    Pragma(String),
    #[error("query failed: {0}")]
    Query(String),
}
