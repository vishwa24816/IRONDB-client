use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;

fn manifest() -> Value {
    let text = include_str!("../../../contracts/golden/i18n_manifest.json");
    serde_json::from_str(text).expect("golden i18n_manifest.json parses")
}

fn locale_keys(locale: &str) -> BTreeSet<String> {
    let path = format!("../../contracts/golden/i18n/{locale}.json");
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|_| panic!("locale file missing: {path}"));
    let v: Value = serde_json::from_str(&text).expect("locale JSON parses");
    flatten_keys(&v, String::new())
}

fn flatten_keys(v: &Value, prefix: String) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    if let Some(obj) = v.as_object() {
        for (k, val) in obj {
            let key = if prefix.is_empty() { k.clone() } else { format!("{prefix}.{k}") };
            if val.is_object() {
                out.extend(flatten_keys(val, key));
            } else {
                out.insert(key);
            }
        }
    }
    out
}

#[test]
fn locale_hashes_match_manifest() {
    let m = manifest();
    let locales = m["locales"].as_object().expect("manifest has locales");
    assert_eq!(locales.len(), 6, "exactly 6 locales frozen");
    for (locale, entry) in locales.iter() {
        let path = format!("../../contracts/golden/i18n/{locale}.json");
        let bytes = std::fs::read(&path)
            .unwrap_or_else(|_| panic!("locale file missing: {path}"));
        let hash = hex::encode(Sha256::digest(&bytes));
        assert_eq!(
            hash,
            entry.get("sha256").and_then(|v| v.as_str()).unwrap(),
            "hash drift for locale {locale}"
        );
        assert!(entry.get("keyCount").and_then(|v| v.as_u64()).unwrap() > 0, "empty locale {locale}");
    }
}

#[test]
fn non_en_locales_subset_of_en() {
    let en = locale_keys("en-US");
    for locale in ["de-DE", "ja-JP", "ru-RU", "zh-CN", "zh-TW"] {
        let keys = locale_keys(locale);
        // Values differ per locale; only the key SET is frozen (subset-or-equal).
        // NOTE: failure prints counts only, never catalog values.
        let missing: Vec<&String> = en.difference(&keys).collect();
        let extra: Vec<&String> = keys.difference(&en).collect();
        assert!(
            extra.is_empty(),
            "{locale} has {} keys not in en-US",
            extra.len()
        );
        eprintln!("{locale}: {} keys missing vs en-US (allowed)", missing.len());
    }
}
