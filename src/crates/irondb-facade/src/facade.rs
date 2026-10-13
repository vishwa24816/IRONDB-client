//! AppFacade: single shared entry point owning store + registry + caches.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use tokio::sync::{Mutex, RwLock};

use irondb_drivers::{CapabilityFlags, DatabaseDriver, DriverRegistry, FakeDriver};

use crate::errors::FacadeError;

pub type SharedFacade = Arc<AppFacade>;

#[derive(Debug, Default)]
pub struct QueryState {
    pub next_id: u64,
}

pub struct AppFacade {
    pub(crate) store: tokio_rusqlite::Connection,
    pub(crate) drivers: DriverRegistry,
    pub(crate) connections: RwLock<HashMap<String, Mutex<Box<dyn DatabaseDriver>>>>,
    pub(crate) query_state: Mutex<QueryState>,
    pub(crate) cancels: irondb_domain::timeout::CancelRegistry,
}

fn default_registry() -> DriverRegistry {
    let mut r = DriverRegistry::new();
    r.register(FakeDriver::ENGINE_ID, FakeDriver::CAPS, || {
        Box::new(FakeDriver::new())
    });
    r.register("fake-readonly", CapabilityFlags::IN_MEMORY, || {
        Box::new(FakeDriver::new_readonly())
    });
    irondb_datasources::register_datasources(&mut r);
    r
}

impl AppFacade {
    /// Test constructor: temp store + fake-registered registry.
    pub async fn new_for_test() -> Result<SharedFacade, FacadeError> {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        static CTR: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let n = CTR.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "irondb-facade-test-{}-{}-{}",
            std::process::id(),
            nanos,
            n
        ));
        std::fs::create_dir_all(&dir).map_err(|e| FacadeError::Store(e.to_string()))?;
        let db = dir.join("app.db");
        Self::open(&db).await
    }

    /// Open against an app DB path.
    pub async fn open(app_db_path: &Path) -> Result<SharedFacade, FacadeError> {
        let store = irondb_store::open_store(app_db_path)
            .await
            .map_err(|e| FacadeError::Store(e.to_string()))?;
        Ok(Arc::new(Self {
            store,
            drivers: default_registry(),
            connections: RwLock::new(HashMap::new()),
            query_state: Mutex::new(QueryState::default()),
            cancels: irondb_domain::timeout::CancelRegistry::default(),
        }))
    }

    /// Explicit user cancel (targetId-notify analogue): flips registry flag.
    /// Drain-then-teardown shutdown: cancel all in-flight queries, then
    /// checkpoint WAL so a file copy/backup is self-contained. Never holds
    /// the rusqlite conn across await outside the bridge call.
    pub async fn shutdown(&self) -> Result<(), FacadeError> {
        self.cancels.cancel_all();
        irondb_store::checkpoint_on_shutdown(&self.store)
            .await
            .map_err(|e| FacadeError::Store(e.to_string()))
    }

    pub fn engines(&self) -> Vec<&'static str> {
        self.drivers.engines()
    }

    /// Next monotonically-increasing query id (≈ Go queryMu counter).
    pub async fn next_query_id(&self) -> u64 {
        let mut q = self.query_state.lock().await;
        q.next_id += 1;
        q.next_id
    }
}
