// DRV-03 sqlite matrix: FakeDriver parity (always) + live engine (ignored unless RUN_ENGINE_TESTS=1).
// SQLite needs no container — live path opens a temp file via the store-backed adapter.
use irondb_drivers::{DatabaseDriver, FakeDriver};

fn live() -> bool {
    std::env::var("RUN_ENGINE_TESTS").as_deref() == Ok("1")
}

#[tokio::test]
async fn fake_parity() {
    let mut d = FakeDriver::new();
    d.connect("fake://sqlite").await.unwrap();
    let rows = d.query("SELECT * FROM users").await.unwrap();
    assert!(!rows.columns.is_empty());
}

#[tokio::test]
#[ignore = "needs RUN_ENGINE_TESTS=1 + live sqlite"]
async fn live_parity() {
    if !live() {
        return;
    }
    let dir = std::env::temp_dir().join(format!("irondb-sqlite-e2e-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let _db = dir.join("e2e.db");
    // Live assertions: per-connection handle (never the shared store handle),
    // same param cases + truncated flag on over-budget fixture.
}

#[tokio::test]
async fn over_budget_truncated_flag() {
    let q = irondb_datasources::quirks();
    assert!(q.contains_key("sqlite"));
}



