//! JMX helper client (ADV-04): thin typed wrapper over the sidecar harness.
//!
//! The original ships `tools/jmx-helper` (Java, `JmxHelperMain`) whose
//! stdout carries only convention JSON (`ok`/`error` + payload). We never
//! embed a JVM: the prebuilt helper binary (or the Go agent, when no Java
//! toolchain is present per the env probe) is spawned through the Phase-4
//! `irondb-sidecar` harness and its stdout-JSON parsed here.
//! T-06-09: passwords/DSNs never logged; [`redacted_summary`] only emits
//! host/port/mbean-attribute names.

use serde::{Deserialize, Serialize};

use crate::protocol::{AgentRequest, AgentResponse};

/// Sidecar method names (frozen).
pub const METHOD_JMX_QUERY: &str = "jmx.query";
pub const METHOD_JMX_LIST: &str = "jmx.list";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JmxTarget {
    pub host: String,
    #[serde(default = "default_jmx_port")]
    pub port: u16,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub username: Option<String>,
    /// Secret is carried in `data` only; never included in Display/logs.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub password: Option<String>,
}

fn default_jmx_port() -> u16 {
    9999
}

impl JmxTarget {
    pub fn validate(&self) -> Result<(), String> {
        if self.host.trim().is_empty() {
            return Err("jmx host must not be empty".into());
        }
        if self.port == 0 {
            return Err("jmx port must be non-zero".into());
        }
        Ok(())
    }

    /// One-line summary safe for logs: no credentials.
    pub fn redacted_summary(&self) -> String {
        format!("{}:{}", self.host, self.port)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct JmxQuery {
    pub target: JmxTarget,
    pub mbean: String,
    pub attribute: String,
}

impl JmxQuery {
    pub fn into_request(self, id: i64) -> AgentRequest {
        AgentRequest {
            id,
            method: METHOD_JMX_QUERY.into(),
            session_id: None,
            query: None,
            args: None,
            timeout_ms: None,
            row_budget: None,
            db_name: None,
            table_name: None,
            alias: None,
            target_id: None,
            data: Some(serde_json::to_value(&self).unwrap_or_default()),
        }
    }
}

/// Convention stdout-JSON payload from the helper: `{ok, value?, error?}`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct JmxValue {
    #[serde(default)]
    pub ok: bool,
    #[serde(default)]
    pub value: Option<serde_json::Value>,
    #[serde(default)]
    pub error: Option<String>,
}

pub fn parse_helper_response(resp: &AgentResponse) -> Result<JmxValue, String> {
    if !resp.success {
        return Err(resp.error.clone().unwrap_or_else(|| "jmx helper failed".into()));
    }
    let data = resp.data.clone().unwrap_or(serde_json::Value::Null);
    serde_json::from_value::<JmxValue>(data).map_err(|e| format!("bad jmx stdout JSON: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_carries_structured_data() {
        let q = JmxQuery {
            target: JmxTarget { host: "db1".into(), port: 9999, username: None, password: Some("s3cret".into()) },
            mbean: "java.lang:type=Memory".into(),
            attribute: "HeapMemoryUsage".into(),
        };
        let req = q.into_request(3);
        assert_eq!(req.method, "jmx.query");
        let line = req.to_line().unwrap();
        assert!(line.contains("HeapMemoryUsage"), "{line}");
    }

    #[test]
    fn redacted_summary_hides_password() {
        let t = JmxTarget { host: "db1".into(), port: 9999, username: Some("u".into()), password: Some("s3cret".into()) };
        let s = t.redacted_summary();
        assert_eq!(s, "db1:9999");
        assert!(!s.contains("s3cret"));
    }

    #[test]
    fn stdout_json_parsed_and_errors_surfaced() {
        let ok = AgentResponse { id: 3, success: true, error: None, outcome_unknown: None, data: Some(serde_json::json!({"ok": true, "value": 42})), fields: None, messages: None, rows_affected: None, truncated: None, partial_data: None };
        let v = parse_helper_response(&ok).unwrap();
        assert!(v.ok && v.value == Some(serde_json::json!(42)));
        let fail = AgentResponse { id: 3, success: false, error: Some("conn refused".into()), outcome_unknown: None, data: None, fields: None, messages: None, rows_affected: None, truncated: None, partial_data: None };
        assert!(parse_helper_response(&fail).is_err());
    }

    #[test]
    fn target_validated() {
        let bad = JmxTarget { host: "".into(), port: 9999, username: None, password: None };
        assert!(bad.validate().is_err());
    }
}
