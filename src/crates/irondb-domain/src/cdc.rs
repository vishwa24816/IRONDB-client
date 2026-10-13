//! CDC registry/adapters with credential hygiene (T-06-03).
//! Never persist credentials; `mongo_safe_endpoint` parity; redacted logs.
use std::collections::HashMap;

use serde::{Deserialize, Serialize};

/// Strip credentials from a Mongo endpoint for positions/logs.
pub fn mongo_safe_endpoint(endpoint: &str) -> String {
    // mongodb://user:pass@host/db -> mongodb://host/db
    if let Some((scheme, rest)) = endpoint.split_once("://") {
        let after_auth = rest.rsplit('@').next().unwrap_or(rest);
        return format!("{scheme}://{after_auth}");
    }
    endpoint.to_string()
}

/// Redact credential-ish keys from a log/position map.
pub fn redact(map: &HashMap<String, String>) -> HashMap<String, String> {
    map.iter()
        .map(|(k, v)| {
            let kl = k.to_lowercase();
            let val = if kl.contains("password") || kl.contains("passwd") || kl.contains("token") || kl.contains("secret") {
                "***".to_string()
            } else {
                v.to_owned()
            };
            (k.to_owned(), val)
        })
        .collect()
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum CdcKind {
    Mongo,
    Mysql,
    Postgres,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CdcPosition {
    pub kind: CdcKind,
    /// Safe endpoint only — never credentials.
    pub endpoint: String,
    pub resume_token: String,
}

impl CdcPosition {
    pub fn new(kind: CdcKind, endpoint: &str, resume_token: &str) -> Self {
        Self { kind, endpoint: mongo_safe_endpoint(endpoint), resume_token: resume_token.into() }
    }

    pub fn serialized(&self) -> String {
        serde_json::to_string(self).unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn safe_endpoint_strips_creds() {
        let s = mongo_safe_endpoint("mongodb://u:p@host:27017/db");
        assert!(!s.contains("p"), "{}", s);
        assert!(s.contains("host"));
    }

    #[test]
    fn redact_hides_secrets() {
        let m: HashMap<String, String> =
            [("password".into(), "x".into()), ("host".into(), "h".into())].into();
        let r = redact(&m);
        assert_eq!(r["password"], "***");
        assert_eq!(r["host"], "h");
    }

    #[test]
    fn position_scan_clean() {
        let p = CdcPosition::new(CdcKind::Mongo, "mongodb://u:secret@h/db", "tok123");
        let s = p.serialized().to_lowercase();
        assert!(!s.contains("password") && !s.contains("secret@"));
        assert!(!p.endpoint.contains('@') || !p.endpoint.contains(':'));
    }
}
