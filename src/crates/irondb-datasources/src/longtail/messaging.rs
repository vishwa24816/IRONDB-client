//! Messaging/CDC streaming adapters (DRV-04): kafka/mqtt/rabbitmq/pulsar/
//! rocketmq + iotdb/kwdb + zookeeper + diros-navicat helpers.
//!
//! Vehicle: Phase-4 sidecar reuse (Go agents own the binary protocols —
//! NO rdkafka per approved resolution). Streaming reads honor timeout +
//! row-budget pipeline semantics; stream positions are redacted before
//! logging (T-06-10). Thin wrapper over `cn::SidecarAdapter`.

use irondb_drivers::{ColumnDef, DatabaseDriver, DriverError, FakeRows};

use super::cn::{redact_secrets, SidecarAdapter};

/// Streaming adapter: sidecar vehicle + redacted-position discipline.
/// `dsn` is `topic@broker` style; passwords never persist (mirrors Go
/// `mongoSafeEndpoint` / never-persist-credentials tests).
pub struct StreamAdapter {
    inner: SidecarAdapter,
    engine_id: &'static str,
    position: Option<String>,
}

impl StreamAdapter {
    pub fn new(engine_id: &'static str) -> Self {
        Self { inner: SidecarAdapter::new(engine_id), engine_id, position: None }
    }

    /// Record a stream position; the stored form is always redacted.
    pub fn set_position(&mut self, pos: &str) {
        self.position = Some(redact_secrets(pos));
    }

    /// Redacted position for logs — never the raw endpoint.
    pub fn redacted_position(&self) -> Option<&str> {
        self.position.as_deref()
    }
}

#[async_trait::async_trait]
impl DatabaseDriver for StreamAdapter {
    async fn connect(&mut self, dsn: &str) -> Result<(), DriverError> {
        // Never persist the raw DSN: keep only the redacted position.
        self.set_position(dsn);
        self.inner.connect(&redact_secrets(dsn)).await
    }

    async fn close(&mut self) -> Result<(), DriverError> {
        self.position = None;
        self.inner.close().await
    }

    async fn ping(&self) -> Result<(), DriverError> {
        self.inner.ping().await
    }

    async fn query(&self, sql: &str) -> Result<FakeRows, DriverError> {
        // Row budget enforced inside the sidecar request (cn::SidecarAdapter).
        self.inner.query(sql).await
    }

    async fn exec(&self, sql: &str) -> Result<i64, DriverError> {
        self.inner.exec(sql).await
    }

    async fn get_databases(&self) -> Result<Vec<String>, DriverError> {
        self.inner.get_databases().await
    }

    async fn get_tables(&self, db: &str) -> Result<Vec<String>, DriverError> {
        self.inner.get_tables(db).await
    }

    async fn get_columns(&self, db: &str, table: &str) -> Result<Vec<ColumnDef>, DriverError> {
        self.inner.get_columns(db, table).await
    }

    fn capabilities(&self) -> irondb_drivers::CapabilityFlags {
        irondb_drivers::CapabilityFlags::empty()
    }

    fn engine_id(&self) -> &'static str {
        self.engine_id
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stream_position_never_holds_password() {
        let mut a = StreamAdapter::new("kafka");
        a.set_position("kafka://user:hunter2@broker:9092/topic?token=abc");
        let pos = a.redacted_position().unwrap();
        assert!(!pos.contains("hunter2"), "{pos}");
        assert!(!pos.contains("abc"), "{pos}");
    }

    #[test]
    fn stream_adapter_fails_closed_without_bin() {
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        rt.block_on(async {
            std::env::remove_var("irondb_SIDECAR_BIN");
            let mut a = StreamAdapter::new("kafka");
            assert!(a.connect("kafka://broker:9092/t").await.is_err());
        });
    }
}
