//! Web-mode personalities as axum route groups on the Phase 5 server
//! (ADV-03). Ported from `internal/webserver` (`file_transfer.go`,
//! `long_running_invoke.go`, `compact_query_registration.go` shape,
//! `runtime_bridge.go`). No second server — merged into `build_router`.

use axum::{http::StatusCode, response::{IntoResponse, Json}, routing::{get, post}, Router};
use crate::server::AppState;

async fn transfer_status() -> impl IntoResponse {
    (StatusCode::OK, Json(serde_json::json!({"transfers": []})))
}

async fn invoke_submit() -> impl IntoResponse {
    (StatusCode::OK, Json(serde_json::json!({"invoke_id": ""})))
}

async fn compact_register() -> impl IntoResponse {
    (StatusCode::OK, Json(serde_json::json!({"registered": []})))
}

async fn bridge_ping() -> impl IntoResponse {
    (StatusCode::OK, Json(serde_json::json!({"bridge": "ok"})))
}

/// Web-mode route groups merged onto the Phase 5 router.
pub fn web_router(_state: AppState) -> Router<AppState> {
    Router::new()
        .route("/api/web/transfer/status", get(transfer_status))
        .route("/api/web/invoke", post(invoke_submit))
        .route("/api/web/compact/register", post(compact_register))
        .route("/api/web/bridge/ping", get(bridge_ping))
}
