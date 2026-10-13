/// Trait boundary for connection lifecycle. No impls in Phase 2.
#[async_trait::async_trait]
pub trait ConnectionService: Send + Sync {
    async fn connect(&self, id: &str) -> Result<(), String>;
    async fn close(&self, id: &str) -> Result<(), String>;
    async fn ping(&self, id: &str) -> Result<(), String>;
}
