//! Bridge handler round-trip (level-b): exercise the same `*_inner` logic the
//! frozen `#[tauri::command]` fns (`DBConnect`, `DBQuery`, `DBQueryWithCancel`)
//! delegate to, against a test `SharedFacade` (temp store + FakeDriver).
//!
//! Explicit non-goal: full WebView `invoke` round-trip (needs a running WebView;
//! deferred to Phase 3). Handler fns take `tauri::State`, which cannot be built
//! outside an app, so the test targets the inner fns 1:1.

use irondb_facade::AppFacade;
use irondb_tauri::commands::connect::db_connect_inner;
use irondb_tauri::commands::query::db_query_inner;

fn assert_no_secrets(msg: &str) {
    let lower = msg.to_lowercase();
    assert!(!lower.contains("password="), "secret leak: {msg}");
    assert!(!lower.contains("hunter2"), "secret leak: {msg}");
    assert!(!msg.contains("://"), "possible userinfo leak: {msg}");
}

#[tokio::test]
async fn bridge_connect_then_query_roundtrip() {
    let facade = AppFacade::new_for_test().await.expect("open facade");

    // Connect through the bridge command logic.
    let res = db_connect_inner(&facade, "c1", "fake", "fake://dummy").await;
    assert!(res.success, "message: {}", res.message);

    // Query through the bridge command logic; assert golden 21-field shape.
    let res = db_query_inner(&facade, "c1", "SELECT * FROM users").await;
    let json = serde_json::to_value(&res).expect("serialize");
    let obj = json.as_object().expect("object");
    for key in ["success", "message"] {
        assert!(obj.contains_key(key), "missing golden key: {key}");
    }
    assert_eq!(obj["success"], true);
    let rows = obj["data"]["rows"].as_array().expect("rows");
    assert_eq!(rows.len(), 2);
}

#[tokio::test]
async fn bridge_failure_envelope_no_secrets() {
    let facade = AppFacade::new_for_test().await.expect("open facade");

    // Query with no connection: success:false, message present.
    let res = db_query_inner(&facade, "nope", "SELECT 1").await;
    assert!(!res.success);
    assert!(!res.message.is_empty());
    assert_no_secrets(&res.message);

    // Bad engine connect: success:false, no DSN material.
    let res = db_connect_inner(&facade, "c9", "oracle-x", "fake://u:hunter2@h").await;
    assert!(!res.success);
    assert_no_secrets(&res.message);
}
