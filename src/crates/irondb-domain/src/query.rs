/// Trait boundary for query execution. No impls in Phase 2.
#[async_trait::async_trait]
pub trait QueryService: Send + Sync {
    async fn query(&self, id: &str, sql: &str) -> Result<serde_json::Value, String>;
    async fn exec(&self, id: &str, sql: &str) -> Result<i64, String>;
}
