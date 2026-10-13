use irondb_drivers::{CapabilityFlags, DatabaseDriver, DriverError, DriverRegistry, FakeDriver};

fn fake_factory() -> Box<dyn irondb_drivers::DatabaseDriver> {
    Box::new(FakeDriver::new())
}

#[tokio::test]
async fn register_resolve_caps_and_query() {
    let mut reg = DriverRegistry::new();
    reg.register(FakeDriver::ENGINE_ID, FakeDriver::CAPS, fake_factory);
    let caps = reg.capabilities("fake").expect("caps");
    assert!(caps.contains(CapabilityFlags::IN_MEMORY));
    let mut drv = reg.resolve("fake").expect("resolve");
    drv.connect("fake://dummy").await.expect("connect");
    drv.ping().await.expect("ping");
    let rows = drv.query("SELECT * FROM users").await.expect("query");
    assert_eq!(rows.columns, vec!["id".to_string(), "name".to_string()]);
    assert_eq!(rows.rows.len(), 2);
}

#[test]
fn unknown_engine_yields_typed_error() {
    let reg = DriverRegistry::new();
    let err = match reg.resolve("nope") {
        Ok(_) => panic!("expected error"),
        Err(e) => e,
    };
    assert_eq!(err, DriverError::UnknownEngine("nope".to_string()));
    let err2 = match reg.capabilities("nope") {
        Ok(_) => panic!("expected error"),
        Err(e) => e,
    };
    assert_eq!(err2, DriverError::UnknownEngine("nope".to_string()));
}

#[tokio::test]
async fn readonly_denial_contains_no_secret() {
    let mut drv = FakeDriver::new_readonly();
    drv.connect("fake://dummy").await.unwrap();
    let err = match drv.exec("INSERT INTO users VALUES (3)").await {
        Ok(_) => panic!("expected denial"),
        Err(e) => e,
    };
    let msg = err.to_string();
    assert!(matches!(err, DriverError::Denied(_)));
    assert!(!msg.contains("password="), "denial leaks secret: {msg}");
    assert!(!msg.contains("://"), "denial leaks dsn: {msg}");
}
