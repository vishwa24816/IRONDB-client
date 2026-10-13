use serde::{Deserialize, Serialize};

/// A saved connection profile (non-secret payload only; secrets live in keyring per Phase 3).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SavedConnection {
    pub id: String,
    pub engine: String,
    pub payload_json: String,
}

/// A simple key-value setting row.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Setting {
    pub key: String,
    pub value: String,
}
