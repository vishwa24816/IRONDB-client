//! HEAD-02: HTTP query/export/cancel/connections over same facade+state.

use std::net::SocketAddr;

use irondb_headless::server::{build_router, AppState};

const TOKEN: &str = "test-token-123";

async fn spawn_server() -> (String, reqwest::Client) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("app.db");
    std::mem::forget(dir); // keep tempdir alive for test duration is best-effort; db file persists
    let facade = irondb_facade::AppFacade::open(&path).await.unwrap();
    facade.connect("c1", "fake", "").await.unwrap();
    let state = AppState { facade, token: TOKEN.into() };
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr: SocketAddr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, build_router(state).into_make_service_with_connect_info::<SocketAddr>())
            .await
            .unwrap();
    });
    (format!("http://{addr}"), reqwest::Client::new())
}

fn auth(r: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
    r.bearer_auth(TOKEN)
}

#[tokio::test]
async fn healthz_open_no_auth() {
    let (base, c) = spawn_server().await;
    let r = c.get(format!("{base}/healthz")).send().await.unwrap();
    assert_eq!(r.status(), 200);
}

#[tokio::test]
async fn api_requires_token() {
    let (base, c) = spawn_server().await;
    let r = c.get(format!("{base}/api/connections")).send().await.unwrap();
    assert_eq!(r.status(), 401);
}

#[tokio::test]
async fn query_success_shape() {
    let (base, c) = spawn_server().await;
    let r = auth(c.post(format!("{base}/api/query")))
        .json(&serde_json::json!({"conn":"c1","sql":"SELECT 1","query_id":"q1"}))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 200);
    let v: serde_json::Value = r.json().await.unwrap();
    assert_eq!(v["success"], true);
    assert_eq!(v["queryId"], "q1");
}

#[tokio::test]
async fn export_returns_payload() {
    let (base, c) = spawn_server().await;
    let r = auth(c.post(format!("{base}/api/export")))
        .json(&serde_json::json!({"conn":"c1","sql":"SELECT 1","query_id":"e1","format":"csv"}))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 200);
    let v: serde_json::Value = r.json().await.unwrap();
    assert!(v.get("payload").is_some(), "export payload missing: {v}");
}

#[tokio::test]
async fn cancel_hits_registry() {
    let (base, c) = spawn_server().await;
    let r = auth(c.post(format!("{base}/api/cancel/q9"))).send().await.unwrap();
    assert_eq!(r.status(), 200);
    let v: serde_json::Value = r.json().await.unwrap();
    assert_eq!(v["cancelled"], "q9");
}

#[tokio::test]
async fn oversized_body_rejected() {
    let (base, c) = spawn_server().await;
    let big = "x".repeat(1024 * 1024 + 16);
    let r = auth(c.post(format!("{base}/api/query")))
        .json(&serde_json::json!({"conn":"c1","sql":big,"query_id":"big"}))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 413);
}
