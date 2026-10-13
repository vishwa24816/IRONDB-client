//! Result-diff service — port of Go `resultdiff.Manager` (LAYER-02 remainder).
//! Sessions: start → upload (chunked, capped) → compute → page → close,
//! with 30-min TTL + eviction. Post-execute pipeline stage only.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// Session TTL: 30 minutes.
pub const SESSION_TTL: Duration = Duration::from_secs(30 * 60);
/// Max rows per session (chunked-upload cap, T-04-11).
pub const MAX_SESSION_ROWS: usize = 100_000;
/// Max rows per single upload chunk.
pub const MAX_CHUNK_ROWS: usize = 10_000;

pub type Row = HashMap<String, serde_json::Value>;

/// One diff entry keyed by the complete ordered key tuple.
#[derive(Debug, Clone, PartialEq)]
pub struct DiffEntry {
    pub key: Vec<String>,
    pub kind: DiffKind,
    pub left: Option<Row>,
    pub right: Option<Row>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiffKind {
    Added,
    Removed,
    Changed,
}

/// A diff session holding two uploaded row sets + computed result.
struct Session {
    left: Vec<Row>,
    right: Vec<Row>,
    result: Vec<DiffEntry>,
    computed: bool,
    last_used: Instant,
}

impl Session {
    fn new() -> Self {
        Self { left: vec![], right: vec![], result: vec![], computed: false, last_used: Instant::now() }
    }

    fn touch(&mut self) {
        self.last_used = Instant::now();
    }

    fn expired(&self) -> bool {
        self.last_used.elapsed() > SESSION_TTL
    }
}

/// Thread-safe diff manager. Wired as post-execute pipeline stage only.
pub struct DiffManager {
    sessions: Mutex<HashMap<String, Session>>,
    next_id: Mutex<u64>,
}

impl DiffManager {
    pub fn new() -> Self {
        Self { sessions: Mutex::new(HashMap::new()), next_id: Mutex::new(0) }
    }

    /// Start a session; returns session id.
    pub fn start(&self) -> String {
        self.evict_expired();
        let mut next = self.next_id.lock().unwrap();
        *next += 1;
        let id = format!("diff-{}", *next);
        self.sessions.lock().unwrap().insert(id.clone(), Session::new());
        id
    }

    /// Upload a chunk of rows to one side ("left" | "right"). Caps enforced.
    pub fn upload(&self, session_id: &str, side: &str, chunk: Vec<Row>) -> Result<usize, String> {
        if chunk.len() > MAX_CHUNK_ROWS {
            return Err(format!("chunk exceeds {MAX_CHUNK_ROWS} rows"));
        }
        let mut sessions = self.sessions.lock().unwrap();
        let s = sessions.get_mut(session_id).ok_or("unknown session")?;
        if s.expired() {
            sessions.remove(session_id);
            return Err("session expired".to_string());
        }
        let idx = match side {
            "left" => 0,
            "right" => 1,
            _ => return Err("side must be left|right".to_string()),
        };
        let cur_len = if idx == 0 { s.left.len() } else { s.right.len() };
        if cur_len + chunk.len() > MAX_SESSION_ROWS {
            return Err(format!("session exceeds {MAX_SESSION_ROWS} rows"));
        }
        if idx == 0 { s.left.extend(chunk); } else { s.right.extend(chunk); }
        let n = if idx == 0 { s.left.len() } else { s.right.len() };
        s.computed = false;
        s.touch();
        Ok(n)
    }

    /// Compute ordered-complete-key-tuple diff. Whitespace-preserving (no trimming).
    pub fn compute(&self, session_id: &str, key_columns: &[String]) -> Result<usize, String> {
        if key_columns.is_empty() {
            return Err("key_columns required".to_string());
        }
        let mut sessions = self.sessions.lock().unwrap();
        let s = sessions.get_mut(session_id).ok_or("unknown session")?;
        if s.expired() {
            sessions.remove(session_id);
            return Err("session expired".to_string());
        }
        let left: HashMap<Vec<String>, &Row> =
            s.left.iter().map(|r| (key_of(r, key_columns), r)).collect();
        let right: HashMap<Vec<String>, &Row> =
            s.right.iter().map(|r| (key_of(r, key_columns), r)).collect();
        let mut result = Vec::new();
        let mut keys: Vec<Vec<String>> = left.keys().chain(right.keys()).cloned().collect();
        keys.sort();
        keys.dedup();
        for k in keys {
            match (left.get(&k), right.get(&k)) {
                (Some(l), Some(r)) => {
                    if l != r {
                        result.push(DiffEntry { key: k, kind: DiffKind::Changed, left: Some((*l).clone()), right: Some((*r).clone()) });
                    }
                }
                (Some(l), None) => result.push(DiffEntry { key: k, kind: DiffKind::Removed, left: Some((*l).clone()), right: None }),
                (None, Some(r)) => result.push(DiffEntry { key: k, kind: DiffKind::Added, left: None, right: Some((*r).clone()) }),
                (None, None) => {}
            }
        }
        let n = result.len();
        s.result = result;
        s.computed = true;
        s.touch();
        Ok(n)
    }

    /// Page the computed result (0-based page).
    pub fn page(&self, session_id: &str, page: usize, page_size: usize) -> Result<Vec<DiffEntry>, String> {
        let mut sessions = self.sessions.lock().unwrap();
        let s = sessions.get_mut(session_id).ok_or("unknown session")?;
        if s.expired() {
            sessions.remove(session_id);
            return Err("session expired".to_string());
        }
        if !s.computed {
            return Err("compute first".to_string());
        }
        s.touch();
        let start = page.saturating_mul(page_size);
        Ok(s.result.iter().skip(start).take(page_size).cloned().collect())
    }

    /// Close (drop) a session.
    pub fn close(&self, session_id: &str) {
        self.sessions.lock().unwrap().remove(session_id);
    }

    /// Evict expired sessions.
    pub fn evict_expired(&self) -> usize {
        let mut sessions = self.sessions.lock().unwrap();
        let before = sessions.len();
        sessions.retain(|_, s| !s.expired());
        before - sessions.len()
    }

    /// Session count (for tests/monitoring).
    pub fn session_count(&self) -> usize {
        self.sessions.lock().unwrap().len()
    }
}

impl Default for DiffManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Build the complete ordered key tuple (exact match, whitespace preserved).
fn key_of(row: &Row, key_columns: &[String]) -> Vec<String> {
    key_columns
        .iter()
        .map(|c| row.get(c).map(|v| match v {
            serde_json::Value::String(s) => s.clone(),
            other => other.to_string(),
        }).unwrap_or_default())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn row(pairs: &[(&str, serde_json::Value)]) -> Row {
        pairs.iter().map(|(k, v)| (k.to_string(), v.clone())).collect()
    }

    fn keys(cols: &[&str]) -> Vec<String> {
        cols.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn diff_matches_key_tuple_semantics() {
        let m = DiffManager::new();
        let id = m.start();
        m.upload(&id, "left", vec![
            row(&[("id", json!(1)), ("v", json!("a"))]),
            row(&[("id", json!(2)), ("v", json!("b"))]),
        ]).unwrap();
        m.upload(&id, "right", vec![
            row(&[("id", json!(2)), ("v", json!("B"))]),
            row(&[("id", json!(3)), ("v", json!("c"))]),
        ]).unwrap();
        let n = m.compute(&id, &keys(&["id"])).unwrap();
        assert_eq!(n, 3); // removed 1, changed 2, added 3
        let p0 = m.page(&id, 0, 2).unwrap();
        assert_eq!(p0.len(), 2);
        let p1 = m.page(&id, 1, 2).unwrap();
        assert_eq!(p1.len(), 1);
        m.close(&id);
        assert_eq!(m.session_count(), 0);
    }

    #[test]
    fn whitespace_preserved_not_trimmed() {
        let m = DiffManager::new();
        let id = m.start();
        m.upload(&id, "left", vec![row(&[("id", json!(1)), ("v", json!("a "))])]).unwrap();
        m.upload(&id, "right", vec![row(&[("id", json!(1)), ("v", json!("a"))])]).unwrap();
        let n = m.compute(&id, &keys(&["id"])).unwrap();
        assert_eq!(n, 1); // trailing space counts as changed
    }

    #[test]
    fn chunk_and_key_guards() {
        let m = DiffManager::new();
        let id = m.start();
        assert!(m.upload(&id, "middle", vec![]).is_err());
        assert!(m.compute(&id, &[]).is_err());
        assert!(m.page(&id, 0, 10).is_err()); // compute first
        assert!(m.upload(&id, "left", vec![HashMap::new(); MAX_CHUNK_ROWS + 1]).is_err());
    }
}
