//! Cancel registry: `db_cancel` fires the token, pending query resolves to a
//! success:false envelope; unknown ids stay in-envelope too.

use irondb_facade::AppFacade;
use irondb_tauri::commands::connect::db_connect_inner;
use irondb_tauri::commands::query::{db_cancel_inner, db_query_with_cancel_inner};

fn assert_no_secrets(msg: &str) {
    let lower = msg.to_lowercase();
    assert!(!lower.contains("password="), "secret leak: {msg}");
    assert!(!msg.contains("://"), "possible userinfo leak: {msg}");
}

#[tokio::test]
async fn cancel_unknown_query_id_stays_in_envelope() {
    let res = db_cancel_inner("no-such-query").await;
    assert!(!res.success);
    assert!(!res.message.is_empty());
    assert_no_secrets(&res.message);
}

#[tokio::test]
async fn cancel_invalid_query_id_stays_in_envelope() {
    let res = db_cancel_inner("").await;
    assert!(!res.success);
}

#[tokio::test]
async fn query_with_cancel_succeeds_and_applies_max_rows() {
    let facade = AppFacade::new_for_test().await.expect("open facade");
    let res = db_connect_inner(&facade, "c1", "fake", "fake://dummy").await;
    assert!(res.success, "message: {}", res.message);

    let res = db_query_with_cancel_inner(
        &facade,
        "c1",
        "SELECT * FROM users",
        "q-ok-1",
        Some(1),
        Some(5000),
    )
    .await;
    assert!(res.success, "message: {}", res.message);
    let rows = res.data.as_ref().and_then(|d| d.get("rows")).and_then(|r| r.as_array());
    assert_eq!(rows.map(|r| r.len()), Some(1), "max_rows not applied");

    // Registry entry removed on completion: second cancel is unknown.
    let res = db_cancel_inner("q-ok-1").await;
    assert!(!res.success);
}

#[tokio::test]
async fn query_invalid_args_stay_in_envelope() {
    let facade = AppFacade::new_for_test().await.expect("open facade");
    let res = db_query_with_cancel_inner(&facade, "", "SELECT 1", "q1", None, None).await;
    assert!(!res.success);
    let res = db_query_with_cancel_inner(&facade, "c1", "   ", "q1", None, None).await;
    assert!(!res.success);
    let res = db_query_with_cancel_inner(&facade, "c1", "SELECT 1", "", None, None).await;
    assert!(!res.success);
}
