//! Spawn + JSON-lines IPC harness. Shutdown: stdin-close → 2s wait → kill (agent_process.go).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures::{SinkExt, StreamExt};
use thiserror::Error;
use tokio::process::{Child, ChildStdin, ChildStdout, Command};
use tokio::sync::mpsc;
use tokio_util::codec::{FramedRead, FramedWrite, LinesCodec};

use crate::protocol::{AgentRequest, AgentResponse};

pub const METADATA_PROBE_TIMEOUT: Duration = Duration::from_secs(30);
pub const METADATA_PROBE_RETRY_TIMEOUT: Duration = Duration::from_secs(5);
pub const CONTROL_CALL_TIMEOUT: Duration = Duration::from_secs(30);
pub const SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(2);
pub const STDERR_TAIL_LINES: usize = 50;
const MAX_LINE_BYTES: usize = 8 << 20;

#[derive(Debug, Error)]
pub enum SidecarError {
    #[error("invalid executable {0}: {1}")]
    InvalidExecutable(String, String),
    #[error("spawn failed: {0}")]
    Spawn(#[source] std::io::Error),
    #[error("transport closed")]
    TransportClosed,
    #[error("call timed out after {0:?}")]
    Timeout(Duration),
    #[error("protocol: {0}")]
    Protocol(String),
    #[error("agent error: {0}")]
    Agent(String),
    #[error("shutdown: process did not exit after kill")]
    ShutdownKill,
}

/// T-04-04: path + fingerprint validation before spawn (port of ValidateOptionalDriverAgentExecutable).
pub fn validate_executable(path: &Path, expected_sha256: Option<&str>) -> Result<PathBuf, SidecarError> {
    let p = path.to_path_buf();
    if !p.is_file() {
        return Err(SidecarError::InvalidExecutable(p.display().to_string(), "not a file".into()));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&p).map(|m| m.permissions().mode()).unwrap_or(0);
        if mode & 0o111 == 0 {
            return Err(SidecarError::InvalidExecutable(p.display().to_string(), "not executable".into()));
        }
    }
    if let Some(fp) = expected_sha256 {
        // Phase 6 upgrades this to a real sha2 hex-digest compare; for now
        // enforce fingerprint shape so callers pass well-formed values.
        if fp.len() != 64 || !fp.chars().all(|c| c.is_ascii_hexdigit()) {
            return Err(SidecarError::InvalidExecutable(p.display().to_string(), "bad fingerprint".into()));
        }
    }
    Ok(p)
}

type Pending = Arc<Mutex<HashMap<i64, tokio::sync::oneshot::Sender<AgentResponse>>>>;

pub struct SidecarClient {
    stdin_tx: Mutex<Option<mpsc::UnboundedSender<String>>>,
    pending: Pending,
    child: Mutex<Option<Child>>,
    stderr_tail: Arc<Mutex<Vec<String>>>,
    next_id: Mutex<i64>,
}

impl SidecarClient {
    pub async fn spawn(bin: &Path, fingerprint: Option<&str>) -> Result<Arc<Self>, SidecarError> {
        let bin = validate_executable(bin, fingerprint)?;
        let mut child = Command::new(&bin)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(SidecarError::Spawn)?;
        let stdin: ChildStdin = child.stdin.take().ok_or(SidecarError::TransportClosed)?;
        let stdout: ChildStdout = child.stdout.take().ok_or(SidecarError::TransportClosed)?;
        let stderr = child.stderr.take();

        let (stdin_tx, mut stdin_rx) = mpsc::unbounded_channel::<String>();
        let pending: Pending = Arc::new(Mutex::new(HashMap::new()));
        let stderr_tail: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));

        // stdin writer
        tokio::spawn(async move {
            let mut w = FramedWrite::new(stdin, LinesCodec::new());
            while let Some(line) = stdin_rx.recv().await {
                if w.send(line).await.is_err() { break; }
            }
        });
        // stdout reader
        {
            let pending = pending.clone();
            tokio::spawn(async move {
                let mut r = FramedRead::new(stdout, LinesCodec::new_with_max_length(MAX_LINE_BYTES));
                while let Some(line) = r.next().await {
                    let Ok(line) = line else { break; };
                    let Ok(resp) = AgentResponse::from_line(&line) else { continue; };
                    if let Some(tx) = pending.lock().unwrap().remove(&resp.id) {
                        let _ = tx.send(resp);
                    }
                }
            });
        }
        // stderr tail (bounded; T-04-05 never log secrets — tail only exposed on error paths)
        if let Some(stderr) = stderr {
            let tail = stderr_tail.clone();
            tokio::spawn(async move {
                let mut r = FramedRead::new(stderr, LinesCodec::new_with_max_length(64 * 1024));
                while let Some(line) = r.next().await {
                    if let Ok(line) = line {
                        let mut t = tail.lock().unwrap();
                        t.push(line);
                        if t.len() > STDERR_TAIL_LINES { t.remove(0); }
                    } else { break; }
                }
            });
        }

        Ok(Arc::new(Self { stdin_tx: Mutex::new(Some(stdin_tx)), pending, child: Mutex::new(Some(child)), stderr_tail, next_id: Mutex::new(1) }))
    }

    pub fn alloc_id(&self) -> i64 {
        let mut n = self.next_id.lock().unwrap();
        let id = *n; *n += 1; id
    }

    pub async fn call(self: &Arc<Self>, mut req: AgentRequest, timeout: Duration) -> Result<AgentResponse, SidecarError> {
        crate::methods::check_method(&req.method).map_err(|e| SidecarError::Protocol(e.to_string()))?;
        if req.id == 0 { req.id = self.alloc_id(); }
        let id = req.id;
        let line = req.to_line().map_err(|e| SidecarError::Protocol(e.to_string()))?;
        let (tx, rx) = tokio::sync::oneshot::channel();
        self.pending.lock().unwrap().insert(id, tx);
        self.send_line(line)?;
        let resp = tokio::time::timeout(timeout, rx).await
            .map_err(|_| { self.pending.lock().unwrap().remove(&id); SidecarError::Timeout(timeout) })?
            .map_err(|_| SidecarError::TransportClosed)?;
        if resp.success { Ok(resp) } else { Err(SidecarError::Agent(resp.error.unwrap_or_default())) }
    }

    /// Metadata probe with 30s budget + 5s cold-start retry (AV scan).
    pub async fn probe_metadata(self: &Arc<Self>) -> Result<AgentResponse, SidecarError> {
        let req = AgentRequest { id: 0, method: "metadata".into(), session_id: None, query: None, args: None, timeout_ms: None, row_budget: None, db_name: None, table_name: None, alias: None, target_id: None, data: None };
        match self.call(req.clone(), METADATA_PROBE_TIMEOUT).await {
            Ok(r) => Ok(r),
            Err(SidecarError::Timeout(_)) => self.call(req, METADATA_PROBE_RETRY_TIMEOUT).await,
            Err(e) => Err(e),
        }
    }

    pub fn stderr_tail(&self) -> Vec<String> {
        self.stderr_tail.lock().unwrap().clone()
    }

    pub fn send_line(&self, line: String) -> Result<(), SidecarError> {
        self.stdin_tx.lock().unwrap().as_ref()
            .ok_or(SidecarError::TransportClosed)?
            .send(line).map_err(|_| SidecarError::TransportClosed)
    }

    /// Shutdown: close stdin → 2s wait → kill (Go `closeAgentProcess`).
    pub async fn shutdown(&self) -> Result<(), SidecarError> {
        // Drop the sender so the writer task ends and the child sees stdin EOF.
        drop(self.stdin_tx.lock().unwrap().take());
        // Take child out to wait/kill.
        let mut child = self.child.lock().unwrap().take();
        if let Some(c) = child.as_mut() {
            match tokio::time::timeout(SHUTDOWN_TIMEOUT, c.wait()).await {
                Ok(_) => Ok(()),
                Err(_) => {
                    let _ = c.kill().await;
                    match tokio::time::timeout(SHUTDOWN_TIMEOUT, c.wait()).await {
                        Ok(_) => Ok(()),
                        Err(_) => Err(SidecarError::ShutdownKill),
                    }
                }
            }
        } else { Ok(()) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(unix)]
    use std::io::Write;

    fn loopback_script() -> (tempfile::NamedTempFile, &'static str) {
        // Loopback agent: echo query back as data, respond to metadata/ping, honor targetId cancel.
        #[cfg(unix)]
        let code = "while read -r line; do m=$(echo \"$line\" | grep -o '\"method\":\"[^\"]*\"' | cut -d'\"' -f4); id=$(echo \"$line\" | grep -o '\"id\":[0-9]*' | cut -d: -f2); case \"$m\" in metadata) echo \"{\\\"id\\\":$id,\\\"success\\\":true,\\\"data\\\":{\\\"driverType\\\":\\\"loopback\\\"}}\";; ping) echo \"{\\\"id\\\":$id,\\\"success\\\":true}\";; cancel|*targetId*) echo \"{\\\"id\\\":$id,\\\"success\\\":true}\";; *) q=$(echo \"$line\" | grep -o '\"query\":\"[^\"]*\"'); echo \"{\\\"id\\\":$id,\\\"success\\\":true,\\\"fields\\\":[\\\"echo\\\"],\\\"data\\\":[{\\\"echo\\\":$q}]}\";; esac; done";
        #[cfg(windows)]
        let code = "powershell-stub";
        #[allow(unused_mut)]
        let mut f = tempfile::NamedTempFile::new().unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            write!(f, "#!/bin/sh\n{code}\n").unwrap();
            std::fs::set_permissions(f.path(), std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        (f, code)
    }

    #[test]
    fn validate_rejects_missing_and_bad_fingerprint() {
        assert!(validate_executable(Path::new("/no/such/bin"), None).is_err());
        let (f, _) = loopback_script();
        let _ = f.path();
        #[cfg(unix)]
        assert!(validate_executable(f.path(), Some("nothex")).is_err());
    }

    #[tokio::test]
    #[cfg(unix)]
    async fn loopback_round_trip_and_shutdown() {
        let (f, _) = loopback_script();
        let c = SidecarClient::spawn(f.path(), None).await.unwrap();
        let req = AgentRequest { id: 0, method: "query".into(), session_id: None, query: Some("SELECT 1".into()), args: None, timeout_ms: Some(30000), row_budget: None, db_name: None, table_name: None, alias: None, target_id: None, data: None };
        let r = c.call(req, Duration::from_secs(5)).await.unwrap();
        assert!(r.success);
        c.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn framed_json_lines_round_trip_in_process() {
        // Cross-platform IPC proof (runs on Windows too): request serializes to one
        // JSON line, LinesCodec frames it, response parses back.
        use tokio::io::duplex;
        let (a, b) = duplex(64 * 1024);
        let mut w = FramedWrite::new(a, LinesCodec::new());
        let mut r = FramedRead::new(b, LinesCodec::new());
        let req = AgentRequest { id: 3, method: "query".into(), session_id: None, query: Some("SELECT 1".into()), args: None, timeout_ms: Some(30000), row_budget: None, db_name: None, table_name: None, alias: None, target_id: None, data: None };
        w.send(req.to_line().unwrap()).await.unwrap();
        let line = r.next().await.unwrap().unwrap();
        let back: AgentRequest = serde_json::from_str(&line).unwrap();
        assert_eq!(back.id, 3);
        assert_eq!(back.query.unwrap(), "SELECT 1");
    }

    #[tokio::test]
    #[cfg(unix)]
    async fn loopback_cancel_notify_then_reusable() {
        let (f, _) = loopback_script();
        let c = SidecarClient::spawn(f.path(), None).await.unwrap();
        // Cancel notify: targetId set, no timeoutMs — sent best-effort, connection stays usable.
        let notify = AgentRequest { id: 0, method: "cancel".into(), session_id: None, query: None, args: None, timeout_ms: None, row_budget: None, db_name: None, table_name: None, alias: None, target_id: Some(1), data: None };
        c.send_line(notify.to_line().unwrap()).unwrap();
        let req = AgentRequest { id: 0, method: "ping".into(), session_id: None, query: None, args: None, timeout_ms: None, row_budget: None, db_name: None, table_name: None, alias: None, target_id: None, data: None };
        let r = c.call(req, Duration::from_secs(5)).await.unwrap();
        assert!(r.success);
        c.shutdown().await.unwrap();
    }

    #[tokio::test]
    #[cfg(unix)]
    async fn shutdown_kills_hung_child() {
        // `sleep 60` ignores stdin-close → must be killed within 2s bound.
        let mut f = tempfile::NamedTempFile::new().unwrap();
        write!(f, "#!/bin/sh\nsleep 60\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(f.path(), std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        let c = SidecarClient::spawn(f.path(), None).await.unwrap();
        let t = std::time::Instant::now();
        c.shutdown().await.unwrap();
        assert!(t.elapsed() < Duration::from_secs(10), "kill fallback bound");
    }
}
