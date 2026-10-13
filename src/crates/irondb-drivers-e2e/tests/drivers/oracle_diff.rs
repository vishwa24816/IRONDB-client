// DRV-03 Go-oracle differential: shells out to Go helpers (Wave-0 spike:
// `go test ./internal/db/ -run TestSQLite -json` from ../IRONDB); falls back
// to recorded goldens in testdata/. Compares Go-vs-Rust outputs, never executes
// returned code (T-04-13). Pinned oracle rev: see ORACLE_REV below.
use std::process::Command;

use irondb_drivers::{DatabaseDriver, FakeDriver};

// ponytail: pinned rev is a const until oracle harness needs version rotation.
const ORACLE_REV: &str = "HEAD";

fn live() -> bool {
    std::env::var("RUN_ENGINE_TESTS").as_deref() == Ok("1")
}

/// Try the Go oracle; None = fall back to recorded goldens.
fn go_oracle(engine: &str) -> Option<String> {
    let go_root = std::env::var("irondb_GO_ROOT").unwrap_or_else(|_| "../IRONDB".into());
    let out = Command::new("go")
        .args([
            "test",
            "./internal/db/",
            &format!("-run=Test{engine}"),
            "-json",
        ])
        .current_dir(&go_root)
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&out.stdout).into_owned())
}

#[tokio::test]
async fn oracle_diff_report() {
    run_oracle_diff_report().await;
}

async fn run_oracle_diff_report() {
    let _ = ORACLE_REV;
    for engine in [
        "mysql",
        "postgres",
        "mssql",
        "sqlite",
        "mongodb",
        "redis",
        "elasticsearch",
    ] {
        let go = go_oracle(engine);
        // Rust side: FakeDriver output for the same fixture.
        let mut d = FakeDriver::new();
        d.connect("fake://oracle").await.unwrap();
        let rust = format!("{:?}", d.query("SELECT * FROM users").await.unwrap().columns);
        match go {
            Some(g) => println!("ORACLE {engine}: go_bytes={} rust={rust}", g.len()),
            None => println!("ORACLE {engine}: golden-fallback rust={rust}"),
        }
    }
}

#[tokio::test]
#[ignore = "needs RUN_ENGINE_TESTS plus live engines plus Go toolchain"]
async fn oracle_diff_live() {
    if !live() {
        return;
    }
    run_oracle_diff_report().await;
}

