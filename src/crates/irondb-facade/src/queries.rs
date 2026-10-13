//! Query path: cached conn → driver → frozen QueryResult envelope.
//! All failures serialize inside QueryResult { success: false } — never Err-with-secrets, never panic.

use irondb_contracts::QueryResult;

use crate::errors::FacadeError;
use crate::facade::AppFacade;

fn failure(message: impl Into<String>) -> QueryResult {
    QueryResult {
        success: false,
        message: message.into(),
        data: None,
        ..Default::default()
    }
}

fn from_fake(rows: irondb_drivers::FakeRows) -> QueryResult {
    QueryResult {
        success: true,
        message: String::new(),
        data: Some(serde_json::json!({"rows": rows.rows})),
        fields: Some(rows.columns),
        ..Default::default()
    }
}

impl AppFacade {
    /// Guarded pipeline entry: timeout deadline + cancel registry + maxRows budget.
    /// Never calls drivers directly outside the deadline; cancel leaves conn reusable.
    pub async fn execute_query(
        &self,
        connection_id: &str,
        sql: &str,
        max_rows: usize,
        timeout_ms: u64,
        query_id: &str,
    ) -> QueryResult {
        use irondb_domain::row_budget::{apply_budget, estimate_bytes, RowBudget};
        use irondb_domain::timeout::{with_deadline, TimeoutConfig, TimeoutError};
        let started = std::time::Instant::now();
        self.cancels.register(query_id);
        let res: Result<QueryResult, TimeoutError> = with_deadline(
            TimeoutConfig { timeout_ms },
            async {
                if self.cancels.is_cancelled(query_id) {
                    return Err(TimeoutError::Cancelled);
                }
                let guard = self.connections.read().await;
                let cell = match guard.get(connection_id) {
                    Some(c) => c,
                    None => {
                        return Err(TimeoutError::Driver(
                            FacadeError::UnknownConnection(connection_id.to_string()).to_string(),
                        ))
                    }
                };
                let driver = cell.lock().await;
                let rows = driver.query(sql).await.map_err(|e| TimeoutError::Driver(e.to_string()))?;
                drop(driver);
                if self.cancels.is_cancelled(query_id) {
                    return Err(TimeoutError::Cancelled);
                }
                let outcome = apply_budget(
                    rows.rows.into_iter().map(|r| serde_json::to_value(&r).unwrap_or_default()),
                    RowBudget { max_rows, max_bytes: 8 * 1024 * 1024 },
                    estimate_bytes,
                );
                Ok(QueryResult {
                    success: true,
                    message: String::new(),
                    data: Some(serde_json::json!({"rows": outcome.rows})),
                    truncated: if outcome.truncated { Some(true) } else { None },
                    scanned_count: Some(outcome.scanned as i64),
                    duration_ms: Some(started.elapsed().as_millis() as i64),
                    query_id: Some(query_id.to_string()),
                    ..Default::default()
                })
            },
        )
        .await;
        self.cancels.complete(query_id);
        match res {
            Ok(qr) => qr,
            Err(TimeoutError::Cancelled) => QueryResult {
                success: false,
                message: "query cancelled".into(),
                cancellation_state: Some("cancelled".into()),
                query_id: Some(query_id.to_string()),
                ..Default::default()
            },
            Err(TimeoutError::TimedOut(ms)) => QueryResult {
                success: false,
                message: format!("query timed out after {ms}ms"),
                query_id: Some(query_id.to_string()),
                ..Default::default()
            },
            Err(TimeoutError::Driver(e)) => failure(e),
        }
    }

    /// Explicit user cancel (targetId-notify analogue): flips registry flag.
    pub async fn cancel_query(&self, query_id: &str) {
        self.cancels.cancel(query_id);
    }

    pub async fn query(&self, connection_id: &str, sql: &str) -> QueryResult {
        let guard = self.connections.read().await;
        let cell = match guard.get(connection_id) {
            Some(c) => c,
            None => {
                return failure(FacadeError::UnknownConnection(connection_id.to_string()).to_string())
            }
        };
        let driver = cell.lock().await;
        // Clone handle pattern N/A for Mutex guard: drop guard before heavy await is
        // inherent here — driver query is the await while holding per-conn lock only.
        match driver.query(sql).await {
            Ok(rows) => from_fake(rows),
            Err(e) => failure(e.to_string()),
        }
    }
}
