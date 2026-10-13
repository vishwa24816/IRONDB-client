//! Axum HTTP shim over shared Arc<AppFacade> (HEAD-02).
//! Routes: GET /healthz (open), GET /api/connections, POST /api/query,
//! POST /api/export, POST /api/cancel/:id (all bearer-auth).
//! Ported limits: 1MiB body (=MaxRequestBodyBytes), TimeoutLayer 10s -> 408.

use std::{net::SocketAddr, time::Duration};

use axum::{
    extract::{Path, State},
    http::{header, StatusCode},
    middleware::{self, Next},
    response::{IntoResponse, Json},
    routing::{get, post},
    Router,
};
use irondb_facade::SharedFacade;
use serde::{Deserialize, Serialize};
use tower_http::{limit::RequestBodyLimitLayer, timeout::TimeoutLayer};

pub const MAX_BODY_BYTES: usize = 1024 * 1024;
pub const HANDLER_TIMEOUT_SECS: u64 = 10;
pub const READ_TIMEOUT_SECS: u64 = 30;
pub const WRITE_TIMEOUT_SECS: u64 = 60;
pub const IDLE_TIMEOUT_SECS: u64 = 120;
pub const DEFAULT_BIND: &str = "127.0.0.1:18789";
pub const TOKEN_ENV: &str = "irondb_HEADLESS_TOKEN";

#[derive(Clone)]
pub struct AppState {
    pub facade: SharedFacade,
    pub token: String,
}

impl AppState {
    pub fn from_env(facade: SharedFacade) -> Self {
        Self {
            facade,
            token: std::env::var(TOKEN_ENV).unwrap_or_default(),
        }
    }
}

/// Constant-time bearer compare; empty configured token denies everything.
fn token_ok(provided: &str, expected: &str) -> bool {
    if expected.is_empty() || provided.is_empty() {
        return false;
    }
    use std::hint::black_box;
    let a = provided.as_bytes();
    let b = expected.as_bytes();
    if a.len() != b.len() {
        // Still walk to avoid length oracle timing shape.
        let mut diff = a.len() ^ b.len();
        for i in 0..a.len().max(b.len()) {
            diff |= (a.get(i).copied().unwrap_or(0) ^ b.get(i).copied().unwrap_or(0)) as usize;
        }
        black_box(diff);
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    black_box(diff);
    diff == 0
}

async fn require_bearer(State(s): State<AppState>, req: axum::http::Request<axum::body::Body>, next: Next) -> impl IntoResponse {
    let got = req
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .unwrap_or("");
    if token_ok(got, &s.token) {
        Ok(next.run(req).await)
    } else {
        Err((StatusCode::UNAUTHORIZED, Json(serde_json::json!({"error":"unauthorized"}))))
    }
}

async fn healthz() -> impl IntoResponse {
    (StatusCode::OK, Json(serde_json::json!({"ok":true})))
}

async fn list_connections() -> impl IntoResponse {
    // Store-backed listing is Phase 3 scope; return empty envelope.
    (StatusCode::OK, Json(serde_json::json!([])))
}

#[derive(Debug, Deserialize)]
pub struct QueryReq {
    pub connection_id: Option<String>,
    pub conn: Option<String>,
    pub sql: String,
    #[serde(default = "default_max_rows")]
    pub max_rows: usize,
    #[serde(default)]
    pub timeout_ms: u64,
    #[serde(default = "default_qid")]
    pub query_id: String,
}
fn default_max_rows() -> usize {
    1000
}
fn default_qid() -> String {
    format!("http-{}", chrono_now())
}
fn chrono_now() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0)
}

async fn post_query(State(s): State<AppState>, Json(b): Json<QueryReq>) -> impl IntoResponse {
    let conn = b.connection_id.or(b.conn).unwrap_or_default();
    if conn.is_empty() || b.sql.is_empty() {
        return (StatusCode::BAD_REQUEST, Json(serde_json::json!({"error":"missing conn/sql"})));
    }
    let res = s.facade.execute_query(&conn, &b.sql, b.max_rows, b.timeout_ms, &b.query_id).await;
    (StatusCode::OK, Json(serde_json::to_value(&res).unwrap_or_default()))
}

#[derive(Debug, Deserialize)]
pub struct ExportReq {
    #[serde(flatten)]
    pub q: QueryReq,
    #[serde(default = "default_fmt")]
    pub format: String,
}
fn default_fmt() -> String {
    "csv".into()
}

#[derive(Debug, Serialize)]
struct ExportResp {
    payload: String,
    format: String,
}

async fn post_export(State(s): State<AppState>, Json(b): Json<ExportReq>) -> impl IntoResponse {
    let conn = b.q.connection_id.clone().or(b.q.conn.clone()).unwrap_or_default();
    if conn.is_empty() || b.q.sql.is_empty() {
        return (StatusCode::BAD_REQUEST, Json(serde_json::json!({"error":"missing conn/sql"})));
    }
    let res = s.facade.execute_query(&conn, &b.q.sql, b.q.max_rows, b.q.timeout_ms, &b.q.query_id).await;
    if !res.success {
        return (StatusCode::OK, Json(serde_json::to_value(&res).unwrap_or_default()));
    }
    let payload = res.data.clone().unwrap_or_default().to_string();
    (StatusCode::OK, Json(serde_json::to_value(&ExportResp { payload, format: b.format }).unwrap_or_default()))
}

async fn post_cancel(State(s): State<AppState>, Path(id): Path<String>) -> impl IntoResponse {
    s.facade.cancel_query(&id).await;
    (StatusCode::OK, Json(serde_json::json!({"cancelled": id})))
}

pub fn build_router(state: AppState) -> Router {
    let api = Router::new()
        .route("/api/connections", get(list_connections))
        .route("/api/query", post(post_query))
        .route("/api/export", post(post_export))
        .route("/api/cancel/{id}", post(post_cancel))
        .route_layer(middleware::from_fn_with_state(state.clone(), require_bearer));
    Router::new()
        .route("/healthz", get(healthz))
        .merge(api)
        .merge(crate::web::web_router(state.clone()))
        .layer(RequestBodyLimitLayer::new(MAX_BODY_BYTES))
        .layer(TimeoutLayer::with_status_code(StatusCode::REQUEST_TIMEOUT, Duration::from_secs(HANDLER_TIMEOUT_SECS)))
        .layer(tower_sessions::SessionManagerLayer::new(tower_sessions::MemoryStore::default()))
        .with_state(state)
}

/// Default bind is localhost-only; wildcard requires explicit --bind.
pub fn resolve_bind(explicit: Option<&str>) -> String {
    explicit.unwrap_or(DEFAULT_BIND).to_string()
}

pub fn is_wildcard_bind(bind: &str) -> bool {
    bind.starts_with("0.0.0.0") || bind.starts_with("[::]")
}

pub const SHUTDOWN_DRAIN_SECS: u64 = 10;

pub async fn shutdown_signal() {
    let ctrl_c = async { tokio::signal::ctrl_c().await.expect("ctrl+c handler") };
    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("sigterm handler")
            .recv()
            .await;
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();
    tokio::select! { _ = ctrl_c => {}, _ = terminate => {}, }
}

pub async fn serve(addr: &str, state: AppState) -> anyhow::Result<()> {
    let listener = tokio::net::TcpListener::bind(addr).await?;
    // Drain-then-teardown (mirrors Go runHTTP/shutdownHTTP order): stop the
    // listener, cancel in-flight query IDs, bounded drain, then checkpoint.
    axum::serve(listener, build_router(state.clone()).into_make_service_with_connect_info::<SocketAddr>())
        .with_graceful_shutdown(shutdown_signal())
        .await?;
    let _ = tokio::time::timeout(
        Duration::from_secs(SHUTDOWN_DRAIN_SECS),
        state.facade.shutdown(),
    )
    .await;
    Ok(())
}
