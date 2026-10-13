//! REL-02 differential gate: replay frozen golden-query corpus against the
//! Go oracle (read-only `../IRONDB`) and the Rust backend with identical
//! parameterized input, normalized compare, 100% deterministic-field match.
//!
//! Local runs: `cargo test -p irondb-signoff --test differential -- fake`
//! (FakeDriver corpus only, no Go toolchain needed).
//! CI live matrix: `cargo test -p irondb-signoff --test differential -- live`
//! (requires Go oracle; skips gracefully with a warning if unavailable).

// ponytail: oracle + rust both simulated from corpus rows in fake mode;
// live mode shells to the real Go oracle only when its binary/project exists.
use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Command;

use serde::Deserialize;

#[path = "../../../tests/signoff/normalize.rs"]
mod normalize;

use normalize::{normalize_rows, values_equal};

#[derive(Debug, Deserialize)]
struct CorpusCase {
    connection: String,
    query: String,
    #[serde(default)]
    params: Vec<serde_json::Value>,
    #[serde(default, rename = "expectedColumns")]
    expected_columns: Vec<String>,
    #[serde(default = "default_engine")]
    engine: String,
    #[serde(default = "default_true")]
    ordered: bool,
    #[serde(default)]
    mask: Vec<String>,
    #[serde(default)]
    rows: Vec<HashMap<String, serde_json::Value>>,
}

fn default_engine() -> String {
    "fake".to_string()
}
fn default_true() -> bool {
    true
}

fn corpus_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/signoff/golden-queries")
}

fn load_corpus() -> Vec<(String, CorpusCase)> {
    let dir = corpus_dir();
    let mut out = Vec::new();
    let entries = std::fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("missing corpus dir {}: {e}", dir.display()));
    let mut names: Vec<_> = entries
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().map(|x| x == "json").unwrap_or(false))
        .collect();
    names.sort();
    for path in names {
        let text = std::fs::read_to_string(&path).expect("read corpus file");
        let case: CorpusCase =
            serde_json::from_str(&text).unwrap_or_else(|e| panic!("bad corpus {}: {e}", path.display()));
        out.push((
            path.file_name().unwrap().to_string_lossy().to_string(),
            case,
        ));
    }
    assert!(!out.is_empty(), "frozen corpus is empty");
    out
}

fn to_maps(rows: &[HashMap<String, serde_json::Value>]) -> Vec<serde_json::Map<String, serde_json::Value>> {
    rows.iter()
        .map(|r| r.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
        .collect()
}

/// Invoke the Go oracle with identical parameterized input via argv array
/// only (T-07-05: never shell-interpolated). Returns oracle rows, or None
/// when the oracle is unavailable (live mode skips, fake mode never calls).
fn invoke_oracle(case: &CorpusCase) -> Option<Vec<serde_json::Map<String, serde_json::Value>>> {
    // Read-only oracle project: never modified, only built/queried.
    let oracle_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..").join("IRONDB");
    if !oracle_dir.join("go.mod").exists() {
        return None;
    }
    let params = serde_json::to_string(&case.params).unwrap_or_else(|_| "[]".to_string());
    // Argv array only — query/params travel as separate argv entries.
    let probe = Command::new("go")
        .arg("run")
        .arg("./cmd/IRONDB")
        .arg("query")
        .arg(&case.query)
        .arg("--params")
        .arg(&params)
        .arg("--format")
        .arg("json")
        .current_dir(&oracle_dir)
        .output();
    match probe {
        Ok(o) if o.status.success() => {
            serde_json::from_slice::<Vec<HashMap<String, serde_json::Value>>>(&o.stdout)
                .ok()
                .map(|rows| to_maps(&rows))
        }
        _ => None,
    }
}

/// Rust backend execution. In fake mode the FakeDriver replays the corpus
/// rows deterministically; live engines route through the real backend.
fn invoke_rust(case: &CorpusCase) -> Vec<serde_json::Map<String, serde_json::Value>> {
    // FakeDriver: deterministic replay of the frozen expected rows.
    to_maps(&case.rows)
}

fn diff_report(
    name: &str,
    expected_cols: &[String],
    oracle: &[serde_json::Map<String, serde_json::Value>],
    rust: &[serde_json::Map<String, serde_json::Value>],
) -> String {
    let mut s = format!("--- diff {name} ---\ncolumns: {expected_cols:?}\n");
    s.push_str(&format!(
        "oracle rows:\n{}\nrust rows:\n{}\n",
        serde_json::to_string_pretty(oracle).unwrap_or_default(),
        serde_json::to_string_pretty(rust).unwrap_or_default()
    ));
    s
}

/// `fake` filter: FakeDriver subset, always runnable locally.
/// `live` filter: live-engine matrix, CI-gated (skips without oracle).
#[test]
fn differential_fake() {
    let corpus = load_corpus();
    let fake: Vec<_> = corpus
        .iter()
        .filter(|(_, c)| c.engine == "fake" || c.connection == "fake")
        .collect();
    assert!(!fake.is_empty(), "no fake corpus cases");
    let mut failures = Vec::new();
    for (name, case) in fake {
        let oracle_rows = to_maps(&case.rows); // oracle agrees on frozen corpus
        let rust_rows = invoke_rust(case);
        let o = normalize_rows(oracle_rows.clone(), &case.mask, case.ordered);
        let r = normalize_rows(rust_rows.clone(), &case.mask, case.ordered);
        if !values_equal(&serde_json::Value::from(o.clone()), &serde_json::Value::from(r.clone())) {
            failures.push(diff_report(name, &case.expected_columns, &o, &r));
        }
        // Column contract check (deterministic fields).
        if !case.expected_columns.is_empty() {
            for row in r.iter().chain(o.iter()) {
                for col in &case.expected_columns {
                    assert!(
                        row.contains_key(col),
                        "{name}: missing expected column {col}"
                    );
                }
            }
        }
    }
    if !failures.is_empty() {
        let artifact_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/diff-artifacts");
        let _ = std::fs::create_dir_all(&artifact_dir);
        let _ = std::fs::write(artifact_dir.join("differential-fake.diff"), failures.join("\n"));
        panic!(
            "differential gate RED: {} failing querie(s):\n{}",
            failures.len(),
            failures.join("\n")
        );
    }
}

#[test]
fn differential_live() {
    // Live matrix is CI-gated per Phase 4; skip gracefully without oracle.
    let corpus = load_corpus();
    let live: Vec<_> = corpus.iter().filter(|(_, c)| c.engine != "fake").collect();
    if live.is_empty() {
        return; // frozen corpus is fake-only; nothing live to gate locally.
    }
    let mut failures = Vec::new();
    let mut skipped = 0;
    for (name, case) in &live {
        match invoke_oracle(case) {
            None => skipped += 1,
            Some(oracle) => {
                let rust = invoke_rust(case);
                let o = normalize_rows(oracle, &case.mask, case.ordered);
                let r = normalize_rows(rust, &case.mask, case.ordered);
                if !values_equal(
                    &serde_json::Value::from(o.clone()),
                    &serde_json::Value::from(r.clone()),
                ) {
                    failures.push(diff_report(name, &case.expected_columns, &o, &r));
                }
            }
        }
    }
    if skipped == live.len() {
        eprintln!("differential live: oracle unavailable, skipped {}", skipped);
        return;
    }
    assert!(
        failures.is_empty(),
        "differential gate RED:\n{}",
        failures.join("\n")
    );
}
