use irondb_contracts::QueryResult;
use serde_json::{json, Value};
use std::collections::BTreeSet;

fn golden() -> Value {
    let text = include_str!("../../../contracts/golden/query_result.schema.json");
    serde_json::from_str(text).expect("golden query_result.schema.json parses")
}

#[test]
fn serialized_key_set_matches_golden() {
    let golden_fields: BTreeSet<String> = golden()["fields"]
        .as_array()
        .expect("golden has fields array")
        .iter()
        .map(|f| f["jsonKey"].as_str().unwrap().to_string())
        .collect();
    // A fully-populated DTO must serialize exactly the golden key set.
    let full = QueryResult {
        success: true,
        message: "ok".into(),
        data: Some(json!({"rows": []})),
        fields: Some(vec!["a".into()]),
        messages: Some(vec!["m".into()]),
        partial: Some(false),
        executed_count: Some(1),
        failed_index: Some(0),
        boundary_mode: Some("b".into()),
        commit_mode: Some("c".into()),
        warnings: Some(vec!["w".into()]),
        outcome_unknown: Some(false),
        failed_object_types: Some(vec!["t".into()]),
        retryable: Some(false),
        truncated: Some(false),
        scanned_count: Some(1),
        duration_ms: Some(2),
        query_id: Some("q".into()),
        cancellation_state: Some("s".into()),
        transaction_id: Some("t".into()),
        transaction_pending: Some(false),
    };
    let live = serde_json::to_value(&full).unwrap();
    let live_keys: BTreeSet<String> = live
        .as_object()
        .unwrap()
        .keys()
        .cloned()
        .collect();
    assert_eq!(live_keys, golden_fields, "QueryResult schema drifted");
}

#[test]
fn omitempty_fields_absent_when_none() {
    let minimal = QueryResult {
        success: true,
        message: "ok".into(),
        data: None,
        ..Default::default()
    };
    // data has no omitempty in Go, so it serializes as null; only omitempty keys are checked.
    let live = serde_json::to_value(&minimal).unwrap();
    let obj = live.as_object().unwrap();
    for key in [
        "fields", "messages", "partial", "executedCount", "failedIndex",
        "boundaryMode", "commitMode", "warnings", "outcomeUnknown",
        "failedObjectTypes", "retryable", "truncated", "scannedCount",
        "durationMs", "queryId", "cancellationState", "transactionId",
        "transactionPending",
    ] {
        assert!(!obj.contains_key(key), "optional key {key} must be absent when None");
    }
    // success/message/data always present per Go struct (no omitempty).
    assert!(obj.contains_key("success"));
    assert!(obj.contains_key("message"));
}

#[test]
fn camel_case_spot_check() {
    let r = QueryResult {
        success: true,
        message: "ok".into(),
        data: None,
        executed_count: Some(7),
        ..Default::default()
    };
    let live = serde_json::to_value(&r).unwrap();
    assert_eq!(live["executedCount"], json!(7));
    assert!(live.get("executed_count").is_none());
}
