//! Verbatim port of Go `optionalAgentRequest/Response` (optional_driver_agent_impl.go:54-101).
//! JSON field names frozen for compat with existing Go agents.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RowBudgetOptions {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_total_rows: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_total_bytes: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentRequest {
    pub id: i64,
    pub method: String,
    #[serde(rename = "sessionId", skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub query: Option<String>,
    /// Positional bind params — json-lines-v2 only; omitempty keeps old agents clean.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub args: Option<Vec<serde_json::Value>>,
    #[serde(rename = "timeoutMs", skip_serializing_if = "Option::is_none")]
    pub timeout_ms: Option<i64>,
    #[serde(rename = "rowBudget", skip_serializing_if = "Option::is_none")]
    pub row_budget: Option<RowBudgetOptions>,
    #[serde(rename = "dbName", skip_serializing_if = "Option::is_none")]
    pub db_name: Option<String>,
    #[serde(rename = "tableName", skip_serializing_if = "Option::is_none")]
    pub table_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub alias: Option<String>,
    /// Cancel notify only: points at the in-flight request id. Distinct from timeoutMs deadline.
    #[serde(rename = "targetId", skip_serializing_if = "Option::is_none")]
    pub target_id: Option<i64>,
    /// Opaque payload for methods carrying structured bodies (changes/attach/es-console).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentResponse {
    pub id: i64,
    pub success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(rename = "outcomeUnknown", skip_serializing_if = "Option::is_none")]
    pub outcome_unknown: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fields: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub messages: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rows_affected: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub truncated: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub partial_data: Option<bool>,
}

impl AgentRequest {
    pub fn to_line(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string(self)
    }
}

impl AgentResponse {
    pub fn from_line(line: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(line)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn golden_field_names_byte_match_go() {
        // Go golden: {"id":7,"method":"query","sessionId":"s1","query":"SELECT 1","timeoutMs":30000,"targetId":0-omitted}
        let req = AgentRequest {
            id: 7,
            method: "query".into(),
            session_id: Some("s1".into()),
            query: Some("SELECT 1".into()),
            args: None,
            timeout_ms: Some(30000),
            row_budget: None,
            db_name: None,
            table_name: None,
            alias: None,
            target_id: None,
            data: None,
        };
        let line = req.to_line().unwrap();
        assert!(line.contains(r#""sessionId":"s1""#), "{line}");
        assert!(line.contains(r#""timeoutMs":30000"#), "{line}");
        assert!(!line.contains("targetId"), "omitempty: {line}");
        assert!(!line.contains("args"), "omitempty: {line}");
        // round-trip
        let back: AgentRequest = serde_json::from_str(&line).unwrap();
        assert_eq!(back.id, 7);
    }

    #[test]
    fn args_v2_and_cancel_target_id() {
        let req = AgentRequest {
            id: 9,
            method: "query".into(),
            session_id: None,
            query: Some("SELECT ?".into()),
            args: Some(vec![serde_json::json!(1)]),
            timeout_ms: None,
            row_budget: Some(RowBudgetOptions { max_total_rows: Some(100), max_total_bytes: None }),
            db_name: None,
            table_name: None,
            alias: None,
            target_id: Some(8),
            data: None,
        };
        let line = req.to_line().unwrap();
        assert!(line.contains(r#""args":[1]"#), "{line}");
        assert!(line.contains(r#""targetId":8"#), "{line}");
        assert!(line.contains(r#""rowBudget":{"max_total_rows":100}"#), "{line}");
    }

    #[test]
    fn response_round_trip() {
        let raw = r#"{"id":7,"success":true,"fields":["a"],"truncated":true}"#;
        let r = AgentResponse::from_line(raw).unwrap();
        assert!(r.success && r.truncated == Some(true));
        assert_eq!(r.fields.unwrap(), vec!["a"]);
    }
}
