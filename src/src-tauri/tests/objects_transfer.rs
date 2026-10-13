use irondb_tauri::commands::{conns, objects, transfer};

#[test]
fn objects_levels_and_caps() {
    // databases always available
    let r = objects::db_objects_inner("c1", "databases", None, 0);
    assert!(r.success);
    // tables need a parent
    assert!(!objects::db_objects_inner("c1", "tables", None, 0).success);
    let r = objects::db_objects_inner("c1", "tables", Some("main"), 0);
    assert!(r.success);
    // schemas gated: empty without SCHEMAS bit (0x04), present with it
    let r = objects::db_objects_inner("c1", "schemas", None, 0);
    assert!(r.success);
    let kids = r.data.unwrap()["children"].as_array().unwrap().len();
    assert_eq!(kids, 0);
    let r = objects::db_objects_inner("c1", "schemas", None, 0x04);
    assert_eq!(r.data.unwrap()["children"].as_array().unwrap().len(), 1);
    // columns per table
    let r = objects::db_objects_inner("c1", "columns", Some("users"), 0);
    assert_eq!(r.data.unwrap()["children"].as_array().unwrap().len(), 2);
    // unknown level + bad id stay inside the envelope
    assert!(!objects::db_objects_inner("c1", "views", None, 0).success);
    assert!(!objects::db_objects_inner("", "databases", None, 0).success);
}

#[test]
fn objects_ddl() {
    let r = objects::db_table_ddl_inner("c1", "users");
    assert!(r.success);
    assert!(r.data.unwrap()["ddl"].as_str().unwrap().contains("CREATE TABLE users"));
    assert!(!objects::db_table_ddl_inner("c1", "nope").success);
}

#[test]
fn conns_masked_and_validated() {
    let r = conns::conn_list_inner();
    assert!(r.success);
    let r = conns::conn_save_inner("c1", "mysql", "local");
    assert!(r.success);
    let c = r.data.unwrap()["connection"].clone();
    assert_eq!(c["keyringRef"], true);
    assert_eq!(c["password"], "••••••");
    let raw = serde_json::to_string(&c).unwrap();
    assert!(!raw.contains("password=") && !raw.contains("://"));
    assert!(!conns::conn_save_inner("", "mysql", "x").success);
    assert!(conns::conn_delete_inner("c1").success);
    assert!(!conns::conn_delete_inner("").success);
}

#[test]
fn transfer_jobs_and_secret_default() {
    let r = transfer::import_run_inner("c1", "users", "csv");
    assert!(r.success);
    assert_eq!(r.data.unwrap()["status"], "queued");
    assert!(!transfer::import_run_inner("c1", "users", "xml").success);
    // secrets excluded unless explicitly opted in
    let r = transfer::export_run_inner("c1", "users", "sql", false);
    assert_eq!(r.data.unwrap()["secretsExcluded"], true);
    let r = transfer::export_run_inner("c1", "users", "sql", true);
    assert_eq!(r.data.unwrap()["secretsExcluded"], false);
    assert!(!transfer::export_run_inner("", "users", "sql", false).success);
}
