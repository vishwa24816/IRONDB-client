use thiserror::Error;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum DriverError {
    #[error("unknown engine: {0}")]
    UnknownEngine(String),
    #[error("connection failed: {0}")]
    Connect(String),
    #[error("query failed: {0}")]
    Query(String),
    #[error("exec failed: {0}")]
    Exec(String),
    #[error("operation denied: {0}")]
    Denied(String),
    #[error("closed")]
    Closed,
}
