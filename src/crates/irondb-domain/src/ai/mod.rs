//! domain::ai facade — AiProvider trait + shared types (AI-SPEC-R §2, §8).
//! Keys live in keyring only; store carries provider id + HasSecret flag (T-06-04).
use async_trait::async_trait;
use secrecy::SecretString;
use serde::{Deserialize, Serialize};
use thiserror::Error;

pub mod cli;
pub mod providers;
pub mod safety;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
    #[serde(default)]
    pub reasoning_content: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatRequest {
    #[serde(default)]
    pub messages: Vec<ChatMessage>,
    #[serde(default = "default_temp")]
    pub temperature: f32,
    #[serde(default)]
    pub max_tokens: u32,
    #[serde(default)]
    pub model: String,
}
fn default_temp() -> f32 { 0.7 }

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TokenUsage { pub prompt: u32, pub completion: u32, pub total: u32 }

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ChatResponse {
    #[serde(default)]
    pub content: String,
    #[serde(default)]
    pub reasoning_content: Option<String>,
    #[serde(default)]
    pub usage: TokenUsage,
    #[serde(default)]
    pub truncated: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct StreamChunk {
    #[serde(default)]
    pub content: String,
    #[serde(default)]
    pub done: bool,
    #[serde(default)]
    pub error: Option<String>,
    #[serde(default)]
    pub truncated: bool,
}

/// Provider identity persisted in SQLite — never the key itself (T-06-04).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderRef {
    pub provider_id: String,
    pub has_secret: bool,
}

#[derive(Debug, Clone)]
pub struct ProviderConfig {
    pub provider_id: String,
    pub api_format: String,
    pub base_url: String,
    pub model: String,
    pub temperature: f32,
    pub api_key: Option<SecretString>,
    /// CLI providers only: merged into subprocess env (AI-SPEC-R §5).
    #[allow(clippy::vec_box)]
    pub cli_env: std::collections::HashMap<String, String>,
    pub cli_path: String,
    pub timeout_secs: u64,
}

#[derive(Debug, Error)]
pub enum AiError {
    #[error("missing api key for {0}")]
    MissingKey(String),
    #[error("missing model id")]
    MissingModel,
    #[error("missing CLI binary: {0}")]
    MissingBinary(String),
    #[error("http {status}: {body}")]
    Http { status: u16, body: String },
    #[error("transport: {0}")]
    Transport(String),
    #[error("output truncated at provider limit")]
    OutputLimit,
}

#[async_trait]
pub trait AiProvider: Send + Sync {
    async fn chat(&self, req: ChatRequest) -> Result<ChatResponse, AiError>;
    async fn chat_stream(
        &self,
        req: ChatRequest,
    ) -> Result<Vec<StreamChunk>, AiError> {
        Ok(vec![StreamChunk { content: self.chat(req).await?.content, done: true, ..Default::default() }])
    }
    fn validate(&self) -> Result<(), AiError>;
    fn provider_ref(&self) -> ProviderRef;
}
