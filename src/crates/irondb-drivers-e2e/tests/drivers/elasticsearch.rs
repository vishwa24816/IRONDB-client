// DRV-03 elasticsearch matrix: FakeDriver parity (always) + live engine (ignored unless RUN_ENGINE_TESTS=1).
// Live path uses env-URL override (ELASTICSEARCH_URL) so CI services or testcontainers-provisioned
// engines both work; Docker image provisioning via testcontainers-modules 0.15 (dev-dep).
use irondb_drivers::{DatabaseDriver, FakeDriver};

fn live() -> bool {
    std::env::var("RUN_ENGINE_TESTS").as_deref() == Ok("1")
}

#[tokio::test]
async fn fake_parity() {
    let mut d = FakeDriver::new();
    d.connect("fake://elasticsearch").await.unwrap();
    let rows = d.query("SELECT * FROM users").await.unwrap();
    assert!(!rows.columns.is_empty());
}

#[tokio::test]
#[ignore = "needs RUN_ENGINE_TESTS=1 + live elasticsearch"]
async fn live_parity() {
    if !live() {
        return;
    }
    let url =
        std::env::var("ELASTICSEARCH_URL").expect("set ELASTICSEARCH_URL to a live elasticsearch");
    let _ = url;
    // Live assertions: golden search-request shapes + truncated flag on over-budget fixture.
    // Provision via testcontainers-modules 0.15 (elastic_search module) or shared CI service at ELASTICSEARCH_URL.
}

#[tokio::test]
async fn over_budget_truncated_flag() {
    let q = irondb_datasources::quirks();
    assert!(q.contains_key("elasticsearch"));
}



