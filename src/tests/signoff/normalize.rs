// Normalization layer for the REL-02 differential gate.
// Ported from the Phase 4 resultdiff pattern: mask volatile cells,
// canonical-sort unordered rows, epsilon-compare floats, CRLF normalize.
use serde_json::Value;

pub const FLOAT_EPSILON: f64 = 1e-9;

/// Mask volatile cells: any column listed in `mask`, plus well-known
/// volatile column names (timestamps, UUIDs) matched case-insensitively.
pub fn mask_volatile(row: &mut serde_json::Map<String, Value>, mask: &[String]) {
    let lowered: Vec<String> = mask.iter().map(|m| m.to_lowercase()).collect();
    for (k, v) in row.iter_mut() {
        let kl = k.to_lowercase();
        if lowered.iter().any(|m| *m == kl)
            || kl.contains("created_at")
            || kl.contains("updated_at")
            || kl == "ts"
            || kl == "timestamp"
            || kl == "uuid"
            || kl == "id_uuid"
        {
            *v = Value::String("<masked>".to_string());
            continue;
        }
        if let Value::String(s) = v {
            // Normalize line endings inside string cells.
            if s.contains("\r\n") {
                *s = s.replace("\r\n", "\n");
            }
        }
    }
}

/// Canonical string for a row (after masking), used for unordered sorting.
pub fn canonical_row(row: &serde_json::Map<String, Value>) -> String {
    let mut keys: Vec<&String> = row.keys().collect();
    keys.sort();
    let mut parts = Vec::with_capacity(keys.len());
    for k in keys {
        parts.push(format!("{}={}", k, canonical_value(&row[k])));
    }
    parts.join("|")
}

fn canonical_value(v: &Value) -> String {
    match v {
        Value::Null => "null".to_string(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => {
            if let Some(f) = n.as_f64() {
                format!("{:.9}", f)
            } else {
                n.to_string()
            }
        }
        Value::String(s) => s.clone(),
        other => serde_json::to_string(other).unwrap_or_default(),
    }
}

/// Compare two JSON values with float epsilon tolerance.
pub fn values_equal(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Number(x), Value::Number(y)) => match (x.as_f64(), y.as_f64()) {
            (Some(fx), Some(fy)) => (fx - fy).abs() <= FLOAT_EPSILON,
            _ => x == y,
        },
        (Value::Array(x), Value::Array(y)) => {
            x.len() == y.len() && x.iter().zip(y.iter()).all(|(a, b)| values_equal(a, b))
        }
        (Value::Object(x), Value::Object(y)) => {
            x.len() == y.len()
                && x.iter()
                    .all(|(k, v)| y.get(k).map(|w| values_equal(v, w)).unwrap_or(false))
        }
        _ => a == b,
    }
}

/// Normalize a row set: mask volatile cells, sort unless ordered.
pub fn normalize_rows(
    rows: Vec<serde_json::Map<String, Value>>,
    mask: &[String],
    ordered: bool,
) -> Vec<serde_json::Map<String, Value>> {
    let mut rows: Vec<_> = rows
        .into_iter()
        .map(|mut r| {
            mask_volatile(&mut r, mask);
            r
        })
        .collect();
    if !ordered {
        rows.sort_by(|a, b| canonical_row(a).cmp(&canonical_row(b)));
    }
    rows
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn volatile_only_diffs_do_not_fail() {
        let mask: Vec<String> = vec!["note".to_string()];
        let mut a = serde_json::Map::new();
        a.insert("id".to_string(), Value::from(1));
        a.insert(
            "created_at".to_string(),
            Value::String("2026-01-01T00:00:00Z".to_string()),
        );
        a.insert("note".to_string(), Value::String("x".to_string()));
        let mut b = a.clone();
        b.insert(
            "created_at".to_string(),
            Value::String("2026-05-05T00:00:00Z".to_string()),
        );
        b.insert("note".to_string(), Value::String("y".to_string()));
        let na = normalize_rows(vec![a], &mask, true);
        let nb = normalize_rows(vec![b], &mask, true);
        assert!(values_equal(&Value::from(na), &Value::from(nb)));
    }

    #[test]
    fn unordered_rows_canonical_sort() {
        let mask: Vec<String> = vec![];
        let mk = |id: i64| {
            let mut m = serde_json::Map::new();
            m.insert("id".to_string(), Value::from(id));
            m
        };
        let a = normalize_rows(vec![mk(2), mk(1)], &mask, false);
        let b = normalize_rows(vec![mk(1), mk(2)], &mask, false);
        assert!(values_equal(&Value::from(a), &Value::from(b)));
    }

    #[test]
    fn float_epsilon_compare() {
        let a = Value::from(0.1f64 + 0.2f64);
        let b = Value::from(0.3f64);
        assert!(values_equal(&a, &b));
    }
}
