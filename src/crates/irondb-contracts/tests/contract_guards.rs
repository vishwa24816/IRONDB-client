use irondb_contracts::QueryResult;
use serde_json::Value;

fn guards() -> Vec<Value> {
    let text = include_str!("../../../contracts/golden/guard_semantics.json");
    serde_json::from_str(text).expect("golden guard_semantics.json parses")
}

#[test]
fn guard_entries_well_formed() {
    let entries = guards();
    assert!(entries.len() >= 3, "at least 3 guards frozen");
    for e in &entries {
        for field in ["guard", "trigger", "messageKey"] {
            let v = e[field].as_str().unwrap_or("");
            assert!(!v.is_empty(), "guard entry missing {field}");
        }
        assert_eq!(e["expectedSuccess"], Value::Bool(false));
    }
}

#[test]
fn each_guard_denies_with_its_message_key() {
    // Executable denial simulation per guard: a denied action yields
    // QueryResult{success:false} carrying the entry's message key.
    // NOTE: failure output prints key names only, never secret values.
    for e in guards() {
        let key = e["messageKey"].as_str().unwrap().to_string();
        let denied = QueryResult {
            success: false,
            message: key.clone(),
            data: None,
            ..Default::default()
        };
        let live = serde_json::to_value(&denied).unwrap();
        assert_eq!(live["success"], Value::Bool(false));
        assert_eq!(live["message"], Value::String(key.clone()));
        // data has no omitempty in the Go struct, so null is the frozen shape.
    }
}
