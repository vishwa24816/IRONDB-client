//! HEAD-01: cli_roundtrip — facade query shape, write-guard denial, connection-file round-trip.
//! Uses unique temp dirs (new_for_test) — no SQLITE_BUSY flakes.

use irondb_facade::AppFacade;
use std::path::PathBuf;

#[path = "../src/cli.rs"]
mod cli;
#[path = "../src/output.rs"]
mod output;

#[tokio::test]
async fn cli_query_success_shape() {
    let facade = AppFacade::new_for_test().await.expect("open");
    facade.connect("c1", "fake", "fake://dummy").await.expect("connect");
    let res = facade.query("c1", "SELECT * FROM users").await;
    assert!(res.success, "message: {}", res.message);
    assert_eq!(res.fields.clone().unwrap(), vec!["id".to_string(), "name".to_string()]);
    let out = output::render(&res, "jsonl").expect("render");
    assert!(out.lines().count() == 2, "expected 2 jsonl rows: {out}");
}

#[tokio::test]
async fn cli_guard_denial_exit_4() {
    let err = cli::write_guard("DROP TABLE users", false).expect_err("must deny");
    assert_eq!(err.exit_code(), cli::EXIT_POLICY);
    assert_eq!(cli::EXIT_POLICY, 4);
    // allow-write passes
    assert!(cli::write_guard("DROP TABLE users", true).is_ok());
    assert!(cli::write_guard("SELECT 1", false).is_ok());
}

#[tokio::test]
async fn cli_connection_file_roundtrip() {
    let dir = tempfile::tempdir().expect("tmp");
    let p: PathBuf = dir.path().join("conn.json");
    std::fs::write(&p, r#"{"engine":"fake","dsn":"fake://dummy","id":"cf1"}"#).expect("write");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o600)).expect("chmod");
    }
    let cf = cli::load_connection_file(&p).expect("load");
    assert_eq!(cf.engine.unwrap(), "fake");
    let facade = AppFacade::new_for_test().await.expect("open");
    facade.connect("cf1", "fake", "fake://dummy").await.expect("connect");
    let res = facade.query("cf1", "SELECT * FROM users").await;
    assert!(res.success);
}
