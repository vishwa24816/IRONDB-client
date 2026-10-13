//! Connection lifecycle: connect persists non-secret payload + caches handle.

use tokio::sync::Mutex;

use irondb_store::SavedConnection;

use crate::errors::{sanitize_dsn, FacadeError};
use crate::facade::AppFacade;

impl AppFacade {
    /// Resolve registry factory, connect, persist payload, cache handle.
    /// Never formats the raw DSN into errors — sanitized only.
    pub async fn connect(&self, id: &str, engine: &str, dsn: &str) -> Result<(), FacadeError> {
        let mut driver = self
            .drivers
            .resolve(engine)
            .map_err(|_| FacadeError::UnknownEngine(engine.to_string()))?;
        driver
            .connect(dsn)
            .await
            .map_err(|_| FacadeError::Connect {
                engine: sanitize_dsn(engine),
            })?;
        let payload = SavedConnection {
            id: id.to_string(),
            engine: engine.to_string(),
            payload_json: "{}".to_string(),
        };
        irondb_store::upsert_connection(&self.store, payload)
            .await
            .map_err(|e| FacadeError::Store(e.to_string()))?;
        let mut guard = self.connections.write().await;
        guard.insert(id.to_string(), Mutex::new(driver));
        Ok(())
    }

    pub async fn disconnect(&self, id: &str) -> Result<(), FacadeError> {
        let entry = {
            let mut guard = self.connections.write().await;
            guard.remove(id)
        };
        match entry {
            Some(cell) => {
                let mut driver = cell.into_inner();
                driver
                    .close()
                    .await
                    .map_err(|e| FacadeError::Driver(e.to_string()))?;
                Ok(())
            }
            None => Err(FacadeError::UnknownConnection(id.to_string())),
        }
    }

    pub async fn is_connected(&self, id: &str) -> bool {
        let guard = self.connections.read().await;
        guard.contains_key(id)
    }
}
