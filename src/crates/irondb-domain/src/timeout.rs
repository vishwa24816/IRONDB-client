//! Per-request deadline + explicit cancel registry.
//! Two mechanisms (never conflated): ctx-derived `timeout_ms` deadline
//! vs explicit user cancel via `cancel(id)`. Cancelled connections stay reusable.
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use thiserror::Error;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum TimeoutError {
    #[error("query timed out after {0}ms")]
    TimedOut(u64),
    #[error("query cancelled")]
    Cancelled,
    #[error("driver: {0}")]
    Driver(String),
}

#[derive(Debug, Clone, Copy)]
pub struct TimeoutConfig {
    pub timeout_ms: u64,
}

impl Default for TimeoutConfig {
    fn default() -> Self { Self { timeout_ms: 30_000 } }
}

/// Cancel registry keyed by query id. `cancel` flips the flag; running
/// queries poll via `is_cancelled`. Registry entry removed on completion
/// so the connection is reusable.
#[derive(Debug, Clone, Default)]
pub struct CancelRegistry {
    inner: Arc<Mutex<HashMap<String, bool>>>,
}

impl CancelRegistry {
    pub fn register(&self, id: &str) {
        self.inner.lock().unwrap().insert(id.to_string(), false);
    }
    pub fn cancel(&self, id: &str) {
        if let Some(v) = self.inner.lock().unwrap().get_mut(id) {
            *v = true;
        }
    }
    pub fn is_cancelled(&self, id: &str) -> bool {
        self.inner.lock().unwrap().get(id).copied().unwrap_or(false)
    }
    pub fn complete(&self, id: &str) {
        self.inner.lock().unwrap().remove(id);
    }
    /// Flip every registered in-flight query to cancelled (shutdown drain path).
    pub fn cancel_all(&self) {
        for v in self.inner.lock().unwrap().values_mut() {
            *v = true;
        }
    }
    pub fn len(&self) -> usize { self.inner.lock().unwrap().len() }
}

pub async fn with_deadline<T, F>(cfg: TimeoutConfig, fut: F) -> Result<T, TimeoutError>
where F: std::future::Future<Output = Result<T, TimeoutError>> {
    match tokio::time::timeout(Duration::from_millis(cfg.timeout_ms), fut).await {
        Ok(r) => r,
        Err(_) => Err(TimeoutError::TimedOut(cfg.timeout_ms)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn deadline_fires() {
        let cfg = TimeoutConfig { timeout_ms: 20 };
        let r: Result<(), TimeoutError> = with_deadline(cfg, async {
            tokio::time::sleep(Duration::from_millis(200)).await;
            Ok(())
        }).await;
        assert_eq!(r, Err(TimeoutError::TimedOut(20)));
    }

    #[tokio::test]
    async fn fast_query_passes() {
        let r = with_deadline(TimeoutConfig::default(), async { Ok::<_, TimeoutError>(42) }).await;
        assert_eq!(r.unwrap(), 42);
    }

    #[test]
    fn cancel_registry_reusable() {
        let reg = CancelRegistry::default();
        reg.register("q1");
        reg.cancel("q1");
        assert!(reg.is_cancelled("q1"));
        reg.complete("q1");
        assert_eq!(reg.len(), 0);
        // reuse same id afterwards
        reg.register("q1");
        assert!(!reg.is_cancelled("q1"));
    }
}
