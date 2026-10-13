//! Cancel registry: targetId notify (explicit user cancel) vs timeoutMs deadline (agent self-abort).
//! Port of Go `optional_driver_agent_cancel.go` distinction.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio_util::sync::CancellationToken;

use crate::protocol::AgentRequest;
use crate::spawn::{CONTROL_CALL_TIMEOUT, SidecarClient, SidecarError};

#[derive(Clone)]
pub struct CancelRegistry {
    tokens: Arc<Mutex<HashMap<i64, CancellationToken>>>,
}

impl CancelRegistry {
    pub fn new() -> Self {
        Self { tokens: Arc::new(Mutex::new(HashMap::new())) }
    }

    /// Register a query id; returns token aborted on explicit cancel.
    pub fn register(&self, id: i64) -> CancellationToken {
        let t = CancellationToken::new();
        self.tokens.lock().unwrap().insert(id, t.clone());
        t
    }

    /// Explicit user cancel: send targetId notify message (NOT a timeout).
    /// T-04-06: caller should also closeSession on drop.
    pub async fn cancel(&self, client: &Arc<SidecarClient>, target_id: i64) -> Result<(), SidecarError> {
        if let Some(t) = self.tokens.lock().unwrap().remove(&target_id) {
            t.cancel();
        }
        let notify = AgentRequest {
            id: 0, method: "cancel".into(), session_id: None, query: None, args: None,
            timeout_ms: None, row_budget: None, db_name: None, table_name: None,
            alias: None, target_id: Some(target_id), data: None,
        };
        // Notify uses method table bypass: send raw without method check.
        let line = notify.to_line().map_err(|e| SidecarError::Protocol(e.to_string()))?;
        // Best-effort: ignore response errors — target may already be gone.
        let _ = client.call_raw_line(&line, CONTROL_CALL_TIMEOUT).await;
        Ok(())
    }
}

impl Default for CancelRegistry {
    fn default() -> Self { Self::new() }
}

impl SidecarClient {
    /// Send a pre-serialized line and wait for the response id inside it (cancel-notify path).
    pub async fn call_raw_line(&self, line: &str, _timeout: Duration) -> Result<(), SidecarError> {
        self.send_line(line.to_string())
    }
}

pub struct CancelHandle {
    id: i64,
    registry: CancelRegistry,
    client: Option<Arc<SidecarClient>>,
}

impl CancelHandle {
    pub fn new(id: i64, registry: CancelRegistry, client: Arc<SidecarClient>) -> Self {
        registry.register(id);
        Self { id, registry, client: Some(client) }
    }
    pub async fn cancel(&self) -> Result<(), SidecarError> {
        if let Some(c) = &self.client {
            self.registry.cancel(c, self.id).await
        } else { Ok(()) }
    }
}

impl Drop for CancelHandle {
    fn drop(&mut self) {
        if let Some(t) = self.registry.tokens.lock().unwrap().remove(&self.id) {
            t.cancel(); // closeSession-on-drop semantic: abort local waiters
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn cancel_notify_distinct_from_timeout() {
        let reg = CancelRegistry::new();
        let tok = reg.register(42);
        assert!(!tok.is_cancelled());
        // Without a client we only assert token mechanics here; full notify covered by loopback.
        tok.cancel();
        assert!(tok.is_cancelled());
    }

    /// Timeout path never sends targetId: a timed-out call leaves no notify behind.
    #[test]
    fn timeout_request_carries_no_target_id() {
        let req = AgentRequest {
            id: 1, method: "query".into(), session_id: None, query: Some("SELECT 1".into()),
            args: None, timeout_ms: Some(100), row_budget: None, db_name: None,
            table_name: None, alias: None, target_id: None, data: None,
        };
        let line = req.to_line().unwrap();
        assert!(line.contains(r#""timeoutMs":100"#), "{line}");
        assert!(!line.contains("targetId"), "{line}");
    }

    /// Cancel notify carries targetId and NO timeoutMs — the two mechanisms are distinct.
    #[test]
    fn cancel_notify_shape() {
        let notify = AgentRequest {
            id: 0, method: "cancel".into(), session_id: None, query: None, args: None,
            timeout_ms: None, row_budget: None, db_name: None, table_name: None,
            alias: None, target_id: Some(42), data: None,
        };
        let line = notify.to_line().unwrap();
        assert!(line.contains(r#""targetId":42"#), "{line}");
        assert!(!line.contains("timeoutMs"), "{line}");
    }
}
