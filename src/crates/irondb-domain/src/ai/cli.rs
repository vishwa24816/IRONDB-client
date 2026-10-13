//! CLI-agent providers (claude/codex/cursor/cursor-agent/grok/codebuddy) via subprocess.
use super::{AiError, AiProvider, ChatRequest, ChatResponse, ProviderConfig, ProviderRef};
use async_trait::async_trait;
use std::collections::HashMap;

pub const CLI_IDS: [&str; 6] = ["claude-cli", "codex-cli", "cursor-cli", "cursor-agent", "grok-cli", "codebuddy-cli"];
const DEFAULT_BINS: [(&str, &str); 6] = [
    ("claude-cli", "claude"), ("codex-cli", "codex"), ("cursor-cli", "cursor"),
    ("cursor-agent", "cursor-agent"), ("grok-cli", "grok"), ("codebuddy-cli", "codebuddy"),
];

pub struct CliProvider { cfg: ProviderConfig, bin: String }

impl CliProvider {
    pub fn new(provider_id: &str, cfg: ProviderConfig) -> Result<Self, AiError> {
        if !CLI_IDS.contains(&provider_id) { return Err(AiError::Transport(format!("unknown CLI provider {provider_id}"))); }
        let bin = if cfg.cli_path.trim().is_empty() {
            DEFAULT_BINS.iter().find(|(id, _)| *id == provider_id).map(|(_, b)| b.to_string()).unwrap()
        } else { cfg.cli_path.clone() };
        Ok(Self { cfg, bin })
    }
    /// Env merge: process env + ProviderConfig.CLIEnv (CLIEnv wins), key never injected as flag.
    pub fn merged_env(&self) -> HashMap<String, String> {
        let mut m: HashMap<String, String> = std::env::vars().collect();
        m.extend(self.cfg.cli_env.clone());
        m
    }
    fn timeout(&self) -> std::time::Duration {
        std::time::Duration::from_secs(self.cfg.timeout_secs.max(1).min(600))
    }
}

#[async_trait]
impl AiProvider for CliProvider {
    async fn chat(&self, req: ChatRequest) -> Result<ChatResponse, AiError> {
        let prompt = req.messages.iter().map(|m| format!("{}: {}", m.role, m.content)).collect::<Vec<_>>().join("\n");
        let mut cmd = tokio::process::Command::new(&self.bin);
        cmd.arg("--prompt").arg(prompt).envs(self.merged_env()).kill_on_drop(true);
        let mut child = cmd.stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::piped())
            .spawn().map_err(|e| {
                if e.kind() == std::io::ErrorKind::NotFound { AiError::MissingBinary(self.bin.clone()) }
                else { AiError::Transport(e.to_string()) }
            })?;
        let out = tokio::time::timeout(self.timeout(), child.wait_with_output()).await
            .map_err(|_| { AiError::Transport("cli timeout; killed".into()) })
            .and_then(|r| r.map_err(|e| AiError::Transport(e.to_string())))?;
        if !out.status.success() {
            return Err(AiError::Transport(format!("cli exit {}: {}", out.status, String::from_utf8_lossy(&out.stderr).chars().take(300).collect::<String>())));
        }
        Ok(ChatResponse { content: String::from_utf8_lossy(&out.stdout).trim().to_string(), ..Default::default() })
    }
    fn validate(&self) -> Result<(), AiError> {
        // PATH probe only — never spawn blindly at validate time.
        if !self.cfg.cli_path.trim().is_empty() && !std::path::Path::new(&self.cfg.cli_path).exists() {
            return Err(AiError::MissingBinary(self.cfg.cli_path.clone()));
        }
        Ok(())
    }
    fn provider_ref(&self) -> ProviderRef {
        ProviderRef { provider_id: self.cfg.provider_id.clone(), has_secret: false }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn cfg(id: &str) -> ProviderConfig {
        ProviderConfig { provider_id: id.into(), api_format: id.into(), base_url: String::new(),
            model: "x".into(), temperature: 0.7, api_key: None,
            cli_env: [("irondb_TEST".into(), "1".into())].into(), cli_path: String::new(), timeout_secs: 5 }
    }
    #[test] fn all_six_constructors() {
        for id in CLI_IDS { assert!(CliProvider::new(id, cfg(id)).is_ok(), "{id}"); }
        assert!(CliProvider::new("nope", cfg("nope")).is_err());
    }
    #[test] fn env_merge_cli_wins() {
        let p = CliProvider::new("claude-cli", cfg("claude-cli")).unwrap();
        assert_eq!(p.merged_env().get("irondb_TEST").map(|s| s.as_str()), Some("1"));
    }
    #[tokio::test] async fn missing_binary_typed_error() {
        let mut c = cfg("codex-cli"); c.cli_path = "irondb-definitely-missing-bin-xyz".into();
        let p = CliProvider::new("codex-cli", c).unwrap();
        assert!(matches!(p.chat(ChatRequest { messages: vec![], temperature: 0.7, max_tokens: 0, model: "x".into() }).await, Err(AiError::MissingBinary(_))));
    }
}
