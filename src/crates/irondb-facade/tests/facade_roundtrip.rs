//! Facade round-trip: connect FakeDriver then query canned table; failure shape + no-leak asserts.

use irondb_facade::AppFacade;

fn assert_no_secrets(msg: &str) {
    let lower = msg.to_lowercase();
    assert!(!lower.contains("password="), "secret leak: {msg}");
    assert!(!msg.contains("://"), "possible userinfo leak: {msg}");
    // no user:pass@ pattern
    if let Some(at) = msg.find('@') {
        let before = &msg[..at];
        assert!(!before.contains(':'), "possible user:pass leak: {msg}");
    }
}

#[tokio::test]
async fn roundtrip_connect_query_success() {
    let facade = AppFacade::new_for_test().await.expect("open facade");
    facade
        .connect("c1", "fake", "fake://dummy")
        .await
        .expect("connect");
    assert!(facade.is_connected("c1").await);
    let res = facade.query("c1", "SELECT * FROM users").await;
    assert!(res.success, "message: {}", res.message);
    let rows = res.data.expect("rows");
    assert_eq!(rows["rows"].as_array().unwrap().len(), 2);
    assert_eq!(
        res.fields.expect("fields"),
        vec!["id".to_string(), "name".to_string()]
    );
}

#[tokio::test]
async fn roundtrip_failure_shape_no_leak() {
    let facade = AppFacade::new_for_test().await.expect("open facade");
    // unknown connection
    let res = facade.query("nope", "SELECT 1").await;
    assert!(!res.success);
    assert_no_secrets(&res.message);
    // scripted driver denial
    facade
        .connect("c2", "fake", "fake://dummy?password=hunter2")
        .await
        .expect("connect");
    let res = facade.query("c2", "SELECT boom").await;
    assert!(!res.success);
    assert_no_secrets(&res.message);
    assert!(!res.message.contains("hunter2"));
}

#[tokio::test]
async fn connect_unknown_engine_errors() {
    let facade = AppFacade::new_for_test().await.expect("open facade");
    let err = facade
        .connect("c9", "oracle-x", "dsn")
        .await
        .expect_err("unknown engine");
    assert_no_secrets(&err.to_string());
}
