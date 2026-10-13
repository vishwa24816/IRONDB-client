//! Secrets seam: round-trip through the OS keyring + masked list payloads.
//! Skips the round-trip body on headless CI with no keyring backend — the
//! masked-payload + envelope asserts always run.

use irondb_tauri::commands::secrets::{
    masked_ref, secret_delete_inner, secret_get_inner, secret_store_inner, MASKED,
};

fn assert_no_secrets(s: &str) {
    let lower = s.to_lowercase();
    assert!(!lower.contains("password="), "secret leak: {s}");
    assert!(!lower.contains("hunter2"), "secret leak: {s}");
    assert!(!s.contains("://"), "possible userinfo leak: {s}");
}

fn backend_available() -> bool {
    // Probe: a store+delete cycle on a probe id tells us a backend exists.
    let probe = "irondb-probe-secret-seam";
    if !secret_store_inner(probe, "probe").success {
        return false;
    }
    let _ = secret_delete_inner(probe);
    true
}

#[test]
fn secrets_masked_list_payload_never_contains_secret() {
    let secret = "hunter2-secret-value";
    let entry = masked_ref("c1");
    let json = serde_json::to_string(&entry).expect("serialize");
    assert!(json.contains("keyringRef"), "missing keyringRef: {json}");
    assert!(json.contains(MASKED), "missing mask: {json}");
    assert!(!json.contains(secret), "secret in list payload!");
    assert_no_secrets(&json);
}

#[test]
fn secret_roundtrip_or_envelope() {
    if !backend_available() {
        // Headless CI: command must still answer inside the envelope.
        let res = secret_store_inner("c1", "x");
        assert_no_secrets(&res.message);
        return;
    }
    let id = format!("irondb-test-{}", std::process::id());
    let secret = "hunter2-roundtrip";
    let res = secret_store_inner(&id, secret);
    assert!(res.success, "message: {}", res.message);
    assert_no_secrets(&res.message);

    let back = secret_get_inner(&id).expect("read back");
    assert_eq!(back, secret);

    let res = secret_delete_inner(&id);
    assert!(res.success, "message: {}", res.message);
    assert!(secret_get_inner(&id).is_err(), "deleted secret still readable");
}

#[test]
fn secret_validation_stays_in_envelope() {
    for bad in ["", "   "] {
        let res = secret_store_inner(bad, "x");
        assert!(!res.success);
        assert_no_secrets(&res.message);
    }
    let res = secret_store_inner("c1", "");
    assert!(!res.success);
}
