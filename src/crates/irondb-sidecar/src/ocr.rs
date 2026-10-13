//! OCR daemon client (ADV-04): thin typed wrapper over the sidecar harness.
//!
//! Mirrors Go `internal/ocr` (`manager` + `server` shape): the Rust binary
//! carries no CV dependencies. Image bytes / provisioning flow go through
//! the local sidecar/daemon; this module only builds typed requests and
//! parses convention stdout-JSON.
//! T-06-09: image bytes travel in `data`, never in logs; errors are typed.

use serde::{Deserialize, Serialize};

use crate::protocol::{AgentRequest, AgentResponse};

/// Sidecar method names (frozen; mirror the Go OCR manager surface).
pub const METHOD_OCR_RECOGNIZE: &str = "ocr.recognize";
pub const METHOD_OCR_PROVISION: &str = "ocr.provision";
pub const METHOD_OCR_STATUS: &str = "ocr.status";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OcrRequest {
    /// Base64 image payload (opaque to this crate).
    pub image_b64: String,
    #[serde(default = "default_lang")]
    pub lang: String,
}

fn default_lang() -> String {
    "eng".into()
}

impl OcrRequest {
    pub fn validate(&self) -> Result<(), String> {
        if self.image_b64.trim().is_empty() {
            return Err("ocr image payload must not be empty".into());
        }
        Ok(())
    }

    pub fn into_request(self, id: i64) -> AgentRequest {
        AgentRequest {
            id,
            method: METHOD_OCR_RECOGNIZE.into(),
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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OcrResult {
    #[serde(default)]
    pub ok: bool,
    #[serde(default)]
    pub text: String,
    #[serde(default)]
    pub confidence: f32,
    #[serde(default)]
    pub error: Option<String>,
}

pub fn parse_daemon_response(resp: &AgentResponse) -> Result<OcrResult, String> {
    if !resp.success {
        return Err(resp.error.clone().unwrap_or_else(|| "ocr daemon failed".into()));
    }
    let data = resp.data.clone().unwrap_or(serde_json::Value::Null);
    serde_json::from_value::<OcrResult>(data).map_err(|e| format!("bad ocr stdout JSON: {e}"))
}

/// Provisioning flow state (mirrors Go manager download/provision shape).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProvisionState {
    Ready,
    Downloading,
    Missing,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognize_request_and_response_round_trip() {
        let req = OcrRequest { image_b64: "aGVsbG8=".into(), lang: "eng".into() };
        assert!(req.validate().is_ok());
        let line = req.into_request(5).to_line().unwrap();
        assert!(line.contains("ocr.recognize"), "{line}");
        let resp = AgentResponse { id: 5, success: true, error: None, outcome_unknown: None, data: Some(serde_json::json!({"ok": true, "text": "hello", "confidence": 0.97})), fields: None, messages: None, rows_affected: None, truncated: None, partial_data: None };
        let out = parse_daemon_response(&resp).unwrap();
        assert_eq!(out.text, "hello");
        assert!(out.confidence > 0.9);
    }

    #[test]
    fn daemon_failure_surfaced_without_payload() {
        let resp = AgentResponse { id: 5, success: false, error: Some("no engine".into()), outcome_unknown: None, data: None, fields: None, messages: None, rows_affected: None, truncated: None, partial_data: None };
        assert!(parse_daemon_response(&resp).is_err());
        let empty = OcrRequest { image_b64: "".into(), lang: "eng".into() };
        assert!(empty.validate().is_err());
    }
}
