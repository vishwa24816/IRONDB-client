//! Row budget enforced inside the scan loop. Overflow sets `truncated`.
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct RowBudget {
    pub max_rows: usize,
    pub max_bytes: usize,
}

impl Default for RowBudget {
    fn default() -> Self { Self { max_rows: 10_000, max_bytes: 8 * 1024 * 1024 } }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BudgetOutcome<T> {
    pub rows: Vec<T>,
    pub truncated: bool,
    pub scanned: usize,
}

pub fn estimate_bytes(row: &serde_json::Value) -> usize {
    row.to_string().len()
}

/// Enforce budget during collection; stops early, sets `truncated`.
pub fn apply_budget<T>(rows: impl IntoIterator<Item = T>, budget: RowBudget, byte_len: impl Fn(&T) -> usize) -> BudgetOutcome<T> {
    let mut out = Vec::new();
    let mut bytes = 0usize;
    let mut scanned = 0usize;
    let mut truncated = false;
    for row in rows {
        scanned += 1;
        bytes += byte_len(&row);
        if out.len() >= budget.max_rows || bytes > budget.max_bytes {
            truncated = true;
            continue; // drain semantics: count scanned, keep truncated flag
        }
        out.push(row);
    }
    BudgetOutcome { rows: out, truncated, scanned }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn over_budget_sets_truncated() {
        let rows: Vec<serde_json::Value> = (0..10).map(|i| json!({"a": i})).collect();
        let o = apply_budget(rows, RowBudget { max_rows: 4, max_bytes: usize::MAX }, estimate_bytes);
        assert!(o.truncated);
        assert_eq!(o.rows.len(), 4);
        assert_eq!(o.scanned, 10);
    }

    #[test]
    fn within_budget_not_truncated() {
        let rows = vec![json!({"a": 1})];
        let o = apply_budget(rows, RowBudget::default(), estimate_bytes);
        assert!(!o.truncated);
    }

    #[test]
    fn byte_budget_triggers() {
        let rows = vec![json!({"a": "x".repeat(100)}); 5];
        let o = apply_budget(rows, RowBudget { max_rows: usize::MAX, max_bytes: 10 }, estimate_bytes);
        assert!(o.truncated);
    }
}
