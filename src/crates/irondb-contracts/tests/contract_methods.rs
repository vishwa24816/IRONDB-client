use irondb_contracts::FROZEN_METHODS;
use serde_json::Value;
use std::collections::BTreeSet;

#[test]
fn frozen_methods_match_golden() {
    let text = include_str!("../../../contracts/golden/bridge_methods.json");
    let golden: Value = serde_json::from_str(text).expect("golden bridge_methods.json parses");
    let arr = golden["methods"].as_array().expect("golden has methods array");
    let golden_names: BTreeSet<&str> =
        arr.iter().map(|m| m.as_str().unwrap()).collect();
    let frozen: BTreeSet<&str> = FROZEN_METHODS.iter().copied().collect();
    assert_eq!(golden["count"].as_u64().unwrap() as usize, arr.len());
    assert_eq!(frozen.len(), FROZEN_METHODS.len(), "FROZEN_METHODS has duplicates");
    assert_eq!(frozen, golden_names, "bridge method inventory drifted");
}
