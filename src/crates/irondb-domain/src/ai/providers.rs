//! HTTP providers: OpenAI-shape core (async-openai) + Anthropic/Gemini reqwest wrappers.
//! Quirks ported verbatim from Go (AI-SPEC-R §3-§4).
use super::{AiError, AiProvider, ChatRequest, ChatResponse, ProviderConfig, ProviderRef, TokenUsage};
use async_trait::async_trait;
use secrecy::{ExposeSecret, SecretString};

pub const ANTHROPIC_VERSION: &str = "2023-06-01";
pub const DEFAULT_ANTHROPIC_BASE: &str = "https://api.anthropic.com";
pub const DEFAULT_GEMINI_BASE: &str = "https://generativelanguage.googleapis.com";

/// Verbatim port of normalizeDeepSeekAnthropicBaseURL (anthropic.go:39-66).
pub fn normalize_deepseek_anthropic_baseurl(base: &str) -> String {
    let raw = base.trim().trim_end_matches('/').to_string();
    if raw.is_empty() { return raw; }
    let parsed = match url::Url::parse(&raw) { Ok(u) => u, Err(_) => return raw };
    let host = parsed.host_str().unwrap_or("").to_lowercase();
    if host != "api.deepseek.com" && !host.ends_with(".deepseek.com") { return raw; }
    let path = parsed.path().trim_end_matches('/').to_string();
    if path.to_lowercase().contains("/anthropic") { return raw; }
    let mut out = parsed.clone();
    let lp = path.to_lowercase();
    if lp == "/v1" || lp.ends_with("/v1") || path.is_empty() || path == "/" {
        out.set_path("/anthropic");
    } else {
        out.set_path(&format!("{path}/anthropic"));
    }
    out.as_str().trim_end_matches('/').to_string()
}

/// Verbatim port of normalizeAnthropicMessagesURL (anthropic.go:22-35).
pub fn normalize_anthropic_messages_url(base: &str) -> String {
    let mut u = base.trim().trim_end_matches('/').to_string();
    if u.is_empty() { u = DEFAULT_ANTHROPIC_BASE.to_string(); }
    u = normalize_deepseek_anthropic_baseurl(&u);
    if u.ends_with("/messages") { return u; }
    if u.ends_with("/v1") { return format!("{u}/messages"); }
    format!("{u}/v1/messages")
}

pub fn is_anthropic_official(base: &str) -> bool {
    url::Url::parse(base.trim()).ok()
        .and_then(|u| u.host_str().map(|h| h.to_lowercase().ends_with("anthropic.com")))
        .unwrap_or(false)
}

/// Gemini generateContent URL (gemini.go shape): {base}/v1beta/models/{model}:generateContent
pub fn gemini_generate_url(base: &str, model: &str) -> String {
    let b = if base.trim().is_empty() { DEFAULT_GEMINI_BASE } else { base.trim().trim_end_matches('/') };
    format!("{b}/v1beta/models/{model}:generateContent")
}

/// OpenAI-shape provider (openai/deepseek/custom + builtin irondb-ai APIFormat openai)
/// via async-openai 0.42.2 with custom base_url.
pub struct OpenAiShapeProvider { cfg: ProviderConfig }

impl OpenAiShapeProvider {
    pub fn new(mut cfg: ProviderConfig) -> Result<Self, AiError> {
        if cfg.model.trim().is_empty() { return Err(AiError::MissingModel); }
        if cfg.temperature <= 0.0 { cfg.temperature = 0.7; }
        if cfg.base_url.trim().is_empty() { cfg.base_url = "https://api.openai.com/v1".into(); }
        Ok(Self { cfg })
    }
    fn client(&self) -> Result<async_openai::Client<async_openai::config::OpenAIConfig>, AiError> {
        use secrecy::ExposeSecret;
        let key = self.cfg.api_key.as_ref().map(|k| k.expose_secret().clone())
            .filter(|k| !k.trim().is_empty())
            .ok_or_else(|| AiError::MissingKey(self.cfg.provider_id.clone()))?;
        let mut conf = async_openai::config::OpenAIConfig::new().with_api_key(key);
        conf = conf.with_api_base(self.cfg.base_url.trim_end_matches('/'));
        Ok(async_openai::Client::with_config(conf))
    }
}

#[async_trait]
impl AiProvider for OpenAiShapeProvider {
    async fn chat(&self, req: ChatRequest) -> Result<ChatResponse, AiError> {
        use async_openai::types::chat::{ChatCompletionRequestMessage, CreateChatCompletionRequestArgs};
        let client = self.client()?;
        let msgs: Vec<ChatCompletionRequestMessage> = req.messages.iter().map(|m| {
            match m.role.as_str() {
                "system" => ChatCompletionRequestMessage::System(async_openai::types::chat::ChatCompletionRequestSystemMessageArgs::default().content(m.content.clone()).build().unwrap().into()),
                "assistant" => ChatCompletionRequestMessage::Assistant(async_openai::types::chat::ChatCompletionRequestAssistantMessageArgs::default().content(m.content.clone()).build().unwrap().into()),
                _ => ChatCompletionRequestMessage::User(async_openai::types::chat::ChatCompletionRequestUserMessageArgs::default().content(m.content.clone()).build().unwrap().into()),
            }
        }).collect();
        let mut b = CreateChatCompletionRequestArgs::default();
        b.model(&req.model).messages(msgs).temperature(req.temperature);
        if req.max_tokens > 0 { b.max_tokens(req.max_tokens as u16); }
        let creq = b.build().map_err(|e| AiError::Transport(e.to_string()))?;
        let resp = client.chat().create(creq).await.map_err(|e| AiError::Transport(e.to_string()))?;
        let choice = resp.choices.into_iter().next();
        let (content, truncated) = match choice {
            Some(c) => (c.message.content.unwrap_or_default(), c.finish_reason == Some(async_openai::types::chat::FinishReason::Length)),
            None => (String::new(), false),
        };
        let u = resp.usage;
        Ok(ChatResponse {
            content, truncated,
            reasoning_content: None,
            usage: TokenUsage {
                prompt: u.as_ref().map(|x| x.prompt_tokens).unwrap_or(0),
                completion: u.as_ref().map(|x| x.completion_tokens).unwrap_or(0),
                total: u.as_ref().map(|x| x.total_tokens).unwrap_or(0),
            },
        })
    }
    fn validate(&self) -> Result<(), AiError> {
        match self.cfg.api_key.as_ref().map(|k| k.expose_secret().trim().is_empty()) {
            Some(false) => Ok(()),
            _ => Err(AiError::MissingKey(self.cfg.provider_id.clone())),
        }
    }
    fn provider_ref(&self) -> ProviderRef {
        ProviderRef { provider_id: self.cfg.provider_id.clone(), has_secret: self.cfg.api_key.is_some() }
    }
}

/// Builtin irondb-ai: OpenAI-compatible shape with default base URL (AI-SPEC-R §2).
pub const BUILTIN_BASE: &str = "https://ai.IRONDB.dev/v1";
pub fn builtin_config(model: &str, api_key: Option<SecretString>) -> ProviderConfig {
    ProviderConfig { provider_id: "irondb-ai".into(), api_format: "openai".into(),
        base_url: BUILTIN_BASE.into(), model: model.into(), temperature: 0.7,
        api_key, cli_env: Default::default(), cli_path: String::new(), timeout_secs: 60 }
}

/// Thin Anthropic reqwest wrapper preserving §3 quirks (version header, beta on official).
pub struct AnthropicProvider { cfg: ProviderConfig, http: reqwest::Client }
impl AnthropicProvider {
    pub fn new(cfg: ProviderConfig) -> Result<Self, AiError> {
        if cfg.model.trim().is_empty() { return Err(AiError::MissingModel); }
        Ok(Self { cfg, http: reqwest::Client::new() })
    }
    pub fn messages_url(&self) -> String { normalize_anthropic_messages_url(&self.cfg.base_url) }
    pub async fn chat(&self, req: ChatRequest) -> Result<ChatResponse, AiError> {
        let key = self.cfg.api_key.as_ref().map(|k| k.expose_secret().clone()).unwrap_or_default();
        if key.trim().is_empty() { return Err(AiError::MissingKey(self.cfg.provider_id.clone())); }
        let (system, messages): (String, Vec<serde_json::Value>) = {
            let mut sys = String::new();
            let mut ms = vec![];
            for m in &req.messages {
                if m.role == "system" { sys = m.content.clone(); }
                else { ms.push(serde_json::json!({"role": m.role, "content": m.content})); }
            }
            (sys, ms)
        };
        let mut body = serde_json::json!({"model": req.model, "max_tokens": if req.max_tokens>0 {req.max_tokens} else {1024}, "messages": messages});
        if !system.is_empty() { body["system"] = system.into(); }
        let mut rb = self.http.post(self.messages_url())
            .header("x-api-key", key)
            .header("anthropic-version", ANTHROPIC_VERSION)
            .header("content-type", "application/json");
        if is_anthropic_official(&self.cfg.base_url) {
            rb = rb.header("anthropic-beta", "interleaved-thinking-2025-05-14,output-128k-2025-02-19,prompt-caching-2024-07-31");
        }
        let r = rb.json(&body).send().await.map_err(|e| AiError::Transport(e.to_string()))?;
        let status = r.status().as_u16();
        let v: serde_json::Value = r.json().await.map_err(|e| AiError::Transport(e.to_string()))?;
        if !(200..300).contains(&status) { return Err(AiError::Http { status, body: v.to_string() }); }
        let text = v.get("content").and_then(|c| c.as_array()).map(|a| a.iter()
            .filter_map(|b| b.get("text").and_then(|t| t.as_str())).collect::<Vec<_>>().join("")).unwrap_or_default();
        let stop = v.get("stop_reason").and_then(|s| s.as_str()).unwrap_or("");
        Ok(ChatResponse { content: text, reasoning_content: None, usage: TokenUsage::default(), truncated: stop == "max_tokens" })
    }
}

#[async_trait]
impl AiProvider for AnthropicProvider {
    async fn chat(&self, req: ChatRequest) -> Result<ChatResponse, AiError> { Self::chat(self, req).await }
    fn validate(&self) -> Result<(), AiError> {
        match self.cfg.api_key.as_ref().map(|k| k.expose_secret().trim().is_empty()) {
            Some(false) => Ok(()),
            _ => Err(AiError::MissingKey(self.cfg.provider_id.clone())),
        }
    }
    fn provider_ref(&self) -> ProviderRef {
        ProviderRef { provider_id: self.cfg.provider_id.clone(), has_secret: self.cfg.api_key.is_some() }
    }
}

/// Thin Gemini reqwest wrapper (gemini.go bespoke REST shape).
pub struct GeminiProvider { cfg: ProviderConfig, http: reqwest::Client }
impl GeminiProvider {
    pub fn new(cfg: ProviderConfig) -> Result<Self, AiError> {
        if cfg.model.trim().is_empty() { return Err(AiError::MissingModel); }
        Ok(Self { cfg, http: reqwest::Client::new() })
    }
    pub async fn chat(&self, req: ChatRequest) -> Result<ChatResponse, AiError> {
        let key = self.cfg.api_key.as_ref().map(|k| k.expose_secret().clone()).unwrap_or_default();
        if key.trim().is_empty() { return Err(AiError::MissingKey(self.cfg.provider_id.clone())); }
        let mut system = None;
        let mut contents = vec![];
        for m in &req.messages {
            if m.role == "system" { system = Some(serde_json::json!({"parts":[{"text": m.content}]})); }
            else {
                let role = if m.role == "assistant" { "model" } else { "user" };
                contents.push(serde_json::json!({"role": role, "parts":[{"text": m.content}]}));
            }
        }
        let mut body = serde_json::json!({"contents": contents, "generationConfig": {"temperature": req.temperature}});
        if let Some(s) = system { body["systemInstruction"] = s; }
        let url = format!("{}?key=REDACTED", gemini_generate_url(&self.cfg.base_url, &req.model));
        let real = format!("{}?key={}", gemini_generate_url(&self.cfg.base_url, &req.model), key);
        let _ = url; // redacted log form; real URL only on wire (T-06-04)
        let r = self.http.post(real).json(&body).send().await.map_err(|e| AiError::Transport(e.to_string()))?;
        let status = r.status().as_u16();
        let v: serde_json::Value = r.json().await.map_err(|e| AiError::Transport(e.to_string()))?;
        if !(200..300).contains(&status) { return Err(AiError::Http { status, body: v.to_string() }); }
        let text = v.pointer("/candidates/0/content/parts").and_then(|p| p.as_array()).map(|a| a.iter()
            .filter_map(|b| b.get("text").and_then(|t| t.as_str())).collect::<Vec<_>>().join("")).unwrap_or_default();
        let finish = v.pointer("/candidates/0/finishReason").and_then(|f| f.as_str()).unwrap_or("");
        Ok(ChatResponse { content: text, reasoning_content: None, usage: TokenUsage::default(), truncated: finish == "MAX_TOKENS" })
    }
}

#[async_trait]
impl AiProvider for GeminiProvider {
    async fn chat(&self, req: ChatRequest) -> Result<ChatResponse, AiError> { Self::chat(self, req).await }
    fn validate(&self) -> Result<(), AiError> {
        match self.cfg.api_key.as_ref().map(|k| k.expose_secret().trim().is_empty()) {
            Some(false) => Ok(()),
            _ => Err(AiError::MissingKey(self.cfg.provider_id.clone())),
        }
    }
    fn provider_ref(&self) -> ProviderRef {
        ProviderRef { provider_id: self.cfg.provider_id.clone(), has_secret: self.cfg.api_key.is_some() }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::ChatMessage;
    #[test] fn deepseek_quirk_verbatim() {
        assert_eq!(normalize_deepseek_anthropic_baseurl("https://api.deepseek.com"), "https://api.deepseek.com/anthropic");
        assert_eq!(normalize_deepseek_anthropic_baseurl("https://api.deepseek.com/v1"), "https://api.deepseek.com/anthropic");
        assert_eq!(normalize_deepseek_anthropic_baseurl("https://api.deepseek.com/anthropic"), "https://api.deepseek.com/anthropic");
        assert_eq!(normalize_deepseek_anthropic_baseurl("https://api.anthropic.com"), "https://api.anthropic.com");
    }
    #[test] fn messages_url_matrix() {
        assert_eq!(normalize_anthropic_messages_url(""), "https://api.anthropic.com/v1/messages");
        assert_eq!(normalize_anthropic_messages_url("https://api.anthropic.com"), "https://api.anthropic.com/v1/messages");
        assert_eq!(normalize_anthropic_messages_url("https://api.anthropic.com/v1"), "https://api.anthropic.com/v1/messages");
        // DeepSeek-anthropic 404 regression: bare deepseek host must gain /anthropic before /v1/messages
        assert_eq!(normalize_anthropic_messages_url("https://api.deepseek.com"), "https://api.deepseek.com/anthropic/v1/messages");
        assert_eq!(normalize_anthropic_messages_url("https://api.deepseek.com/v1"), "https://api.deepseek.com/anthropic/v1/messages");
        assert!(is_anthropic_official("https://api.anthropic.com"));
        assert!(!is_anthropic_official("https://api.deepseek.com"));
    }
    #[test] fn gemini_url_shape() {
        assert_eq!(gemini_generate_url("", "gemini-2.0-flash"), "https://generativelanguage.googleapis.com/v1beta/models/gemini-2.0-flash:generateContent");
    }
    #[tokio::test] async fn mock_server_shape_quirks() {
        // Tiny mock server asserts Anthropic headers + Gemini URL shape, no live keys.
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let ln = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = ln.local_addr().unwrap();
        let srv = tokio::spawn(async move {
            let mut hits = vec![];
            for _ in 0..2 {
                let (mut s, _) = ln.accept().await.unwrap();
                let mut buf = vec![0u8; 8192];
                let n = s.read(&mut buf).await.unwrap_or(0);
                let req = String::from_utf8_lossy(&buf[..n]).to_string();
                hits.push(req.clone());
                let is_anthropic = req.contains("anthropic-version");
                let body = if is_anthropic {
                    serde_json::json!({"content":[{"text":"a-ok"}],"stop_reason":"end_turn"}).to_string()
                } else {
                    serde_json::json!({"candidates":[{"content":{"parts":[{"text":"g-ok"}]},"finishReason":"STOP"}]}).to_string()
                };
                let resp = format!("HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}", body.len(), body);
                s.write_all(resp.as_bytes()).await.ok();
            }
            hits
        });
        use secrecy::SecretString;
        let mk = |id: &str, base: String| ProviderConfig {
            provider_id: id.into(), api_format: id.into(), base_url: base,
            model: "m".into(), temperature: 0.7,
            api_key: Some(SecretString::new("test-key".into())),
            cli_env: Default::default(), cli_path: String::new(), timeout_secs: 30,
        };
        let a = AnthropicProvider::new(mk("anthropic", format!("http://{addr}/custom"))).unwrap();
        let ra = a.chat(ChatRequest { messages: vec![ChatMessage { role: "user".into(), content: "hi".into(), reasoning_content: None }], temperature: 0.7, max_tokens: 16, model: "m".into() }).await.unwrap();
        assert_eq!(ra.content, "a-ok");
        assert!(a.messages_url().ends_with("/v1/messages"));
        let g = GeminiProvider::new(mk("gemini", format!("http://{addr}"))).unwrap();
        let rg = g.chat(ChatRequest { messages: vec![ChatMessage { role: "user".into(), content: "hi".into(), reasoning_content: None }], temperature: 0.7, max_tokens: 16, model: "gemini-2.0-flash".into() }).await.unwrap();
        assert_eq!(rg.content, "g-ok");
        let hits = srv.await.unwrap();
        assert!(hits[0].contains("anthropic-version: 2023-06-01"), "anthropic version header missing");
        assert!(hits[1].contains("v1beta/models/gemini-2.0-flash:generateContent"), "gemini URL shape wrong: {}", hits[1].lines().next().unwrap_or(""));
    }
}
