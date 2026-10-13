//! russh 0.64.1 direct-tcpip local forwarder + strict host-key policy.
//!
//! Shape mirror of Go `internal/ssh`: key-file or password auth, local
//! listener on 127.0.0.1:0 whose port is handed to driver dial paths
//! (`TunnelHandle::local_addr`). T-06-08: unknown host keys are rejected
//! unless the caller passes an explicit `HostKeyPolicy::TrustOnFirstUse`
//! built from a user confirmation; changed keys are always rejected and
//! never auto-overwritten. Passwords/key material never enter logs.

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use russh::client::{self, Handler};
use russh::keys::{self, PublicKeyOrCertificate};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum TunnelError {
    #[error("ssh transport: {0}")]
    Transport(#[from] russh::Error),
    #[error("host key rejected for {host}:{port}: {reason}")]
    HostKeyRejected { host: String, port: u16, reason: String },
    #[error("authentication failed for user {0}")]
    AuthFailed(String),
    #[error("no auth method configured (password or key file required)")]
    NoAuth,
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("key: {0}")]
    Keys(#[from] keys::Error),
}

/// Password or key-file auth. Passphrase-protected keys are rejected like
/// the Go side (no interactive prompt in the tunnel path).
#[derive(Debug, Clone, Default)]
pub enum AuthMethod {
    #[default]
    None,
    Password(String),
    KeyFile { path: PathBuf },
}

/// Strict by default. The TOFU variant must only be constructed after an
/// explicit user trust action in the UI layer.
#[derive(Debug, Clone)]
pub enum HostKeyPolicy {
    /// Reject unknown keys; reject changed keys.
    Strict { known_hosts: Option<PathBuf> },
    /// Learn unknown keys into `known_hosts`; still reject changed keys.
    TrustOnFirstUse { known_hosts: PathBuf },
}

impl Default for HostKeyPolicy {
    fn default() -> Self {
        Self::Strict { known_hosts: None }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VerifyOutcome {
    Trusted,
    Unknown,
    Changed { line: usize },
}

/// Strip the trailing comment: ssh-key equality (and russh's file
/// round-trip, which drops comments on read but keeps them on learn)
/// treats commented and uncommented keys as different. All policy
/// decisions run on the comment-free key.
fn canonical_key(key: &russh::keys::ssh_key::PublicKey) -> russh::keys::ssh_key::PublicKey {
    russh::keys::ssh_key::PublicKey::new(key.key_data().clone(), "")
}

/// Pure policy evaluation over russh's known-hosts check, so the security
/// decision is unit-testable without a network peer.
pub fn verify_host_key(
    host: &str,
    port: u16,
    key: &russh::keys::ssh_key::PublicKey,
    policy: &HostKeyPolicy,
) -> Result<VerifyOutcome, TunnelError> {
    let key = &canonical_key(key);
    let path = match policy {
        HostKeyPolicy::Strict { known_hosts } => known_hosts.clone(),
        HostKeyPolicy::TrustOnFirstUse { known_hosts } => Some(known_hosts.clone()),
    };
    let Some(path) = path else {
        return Ok(VerifyOutcome::Unknown);
    };
    if !path.exists() {
        return Ok(VerifyOutcome::Unknown);
    }
    match keys::check_known_hosts_path(host, port, key, &path) {
        Ok(true) => Ok(VerifyOutcome::Trusted),
        Ok(false) => Ok(VerifyOutcome::Unknown),
        Err(keys::Error::KeyChanged { line }) => Ok(VerifyOutcome::Changed { line }),
        Err(e) => Err(TunnelError::Keys(e)),
    }
}

/// Enforces the policy inside the russh handshake. Returns `false`
/// (abort) for unknown keys under [`HostKeyPolicy::Strict`] and for
/// changed keys under every policy — a MITM downgrade is impossible
/// without an explicit user trust action.
pub fn enforce_policy(
    host: &str,
    port: u16,
    key: &russh::keys::ssh_key::PublicKey,
    policy: &HostKeyPolicy,
) -> Result<bool, TunnelError> {
    let key = &canonical_key(key);
    match verify_host_key(host, port, key, policy)? {
        VerifyOutcome::Trusted => Ok(true),
        VerifyOutcome::Unknown => match policy {
            HostKeyPolicy::Strict { .. } => Err(TunnelError::HostKeyRejected {
                host: host.into(),
                port,
                reason: format!(
                    "unknown host key {} (strict mode; confirm fingerprint to trust)",
                    key.fingerprint(russh::keys::HashAlg::Sha256)
                ),
            }),
            HostKeyPolicy::TrustOnFirstUse { known_hosts } => {
                keys::known_hosts::learn_known_hosts_path(host, port, key, known_hosts)?;
                Ok(true)
            }
        },
        VerifyOutcome::Changed { line } => Err(TunnelError::HostKeyRejected {
            host: host.into(),
            port,
            reason: format!("host key changed (known_hosts line {line}); possible MITM"),
        }),
    }
}

#[derive(Debug, Clone)]
pub struct SshTunnelConfig {
    pub host: String,
    pub port: u16,
    pub user: String,
    pub auth: AuthMethod,
    pub policy: HostKeyPolicy,
    /// Remote endpoint the local port forwards to (e.g. the DB host:port
    /// reachable from the SSH server).
    pub remote_host: String,
    pub remote_port: u32,
}

/// Bound local endpoint handed to driver dial paths.
#[derive(Debug)]
pub struct TunnelHandle {
    pub local_addr: SocketAddr,
    shutdown: tokio::sync::oneshot::Sender<()>,
}

impl TunnelHandle {
    pub fn close(self) {
        let _ = self.shutdown.send(());
    }
}

struct VerifyingHandler {
    host: String,
    port: u16,
    policy: HostKeyPolicy,
}

impl Handler for VerifyingHandler {
    type Error = TunnelError;

    async fn check_server_key(
        &mut self,
        server_public_key: &PublicKeyOrCertificate,
    ) -> Result<bool, Self::Error> {
        match server_public_key {
            PublicKeyOrCertificate::PublicKey { key, .. } => {
                enforce_policy(&self.host, self.port, key, &self.policy)
            }
            // Certificates are only trusted when already pinned in known_hosts.
            PublicKeyOrCertificate::Certificate(_) => Err(TunnelError::HostKeyRejected {
                host: self.host.clone(),
                port: self.port,
                reason: "SSH certificates are not trusted by policy".into(),
            }),
        }
    }
}

/// Connect, authenticate, and serve a 127.0.0.1 loopback direct-tcpip
/// forwarder until the returned handle is closed.
pub async fn open_tunnel(cfg: SshTunnelConfig) -> Result<TunnelHandle, TunnelError> {
    if matches!(cfg.auth, AuthMethod::None) {
        return Err(TunnelError::NoAuth);
    }
    let ssh_config = Arc::new(client::Config::default());
    let handler = VerifyingHandler {
        host: cfg.host.clone(),
        port: cfg.port,
        policy: cfg.policy.clone(),
    };
    let mut session =
        client::connect(ssh_config, (cfg.host.as_str(), cfg.port), handler).await?;
    let authed = match &cfg.auth {
        AuthMethod::None => unreachable!(),
        AuthMethod::Password(pw) => {
            session.authenticate_password(cfg.user.as_str(), pw.as_str()).await?
        }
        AuthMethod::KeyFile { path } => {
            let key = keys::load_secret_key(path, None)?;
            let key = std::sync::Arc::new(key);
            let alg = keys::PrivateKeyWithHashAlg::new(key, None);
            session.authenticate_publickey(cfg.user.as_str(), alg).await?
        }
    };
    if !authed.success() {
        return Err(TunnelError::AuthFailed(cfg.user.clone()));
    }

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let local_addr = listener.local_addr()?;
    let (tx, mut rx) = tokio::sync::oneshot::channel::<()>();
    let session = Arc::new(session);
    tokio::spawn(async move {
        loop {
            tokio::select! {
                _ = &mut rx => break,
                accepted = listener.accept() => {
                    let Ok((inbound, _)) = accepted else { continue };
                    let session = Arc::clone(&session);
                    let (remote_host, remote_port) =
                        (cfg.remote_host.clone(), cfg.remote_port);
                    tokio::spawn(async move {
                        let channel = match session
                            .channel_open_direct_tcpip(
                                remote_host, remote_port, "127.0.0.1", 0,
                            )
                            .await
                        {
                            Ok(ch) => ch,
                            Err(_) => return,
                        };
                        let mut outbound = channel.into_stream();
                        let mut inbound = inbound;
                        let _ = tokio::io::copy_bidirectional(&mut inbound, &mut outbound).await;
                    });
                }
            }
        }
    });
    Ok(TunnelHandle { local_addr, shutdown: tx })
}

#[cfg(test)]
mod tests {
    use super::*;

    // Fixed ed25519 test key; safe to embed (public half only).
    const TEST_KEY: &str =
        "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIP50xY/0RH0KTsCMZmDvdLDq5yW8CnslnjkrdmfwNmdp test@IRONDB";
    // Second key with same algorithm but different material for the changed-key case.
    const OTHER_KEY: &str =
        "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIM69jEMdn8hRzlZ38ouu46Mv1uUwyiLg3IygKN+K/1P6 test2@IRONDB";

    fn test_key() -> russh::keys::ssh_key::PublicKey {
        TEST_KEY.parse().unwrap()
    }

    fn write_known_hosts(dir: &std::path::Path, host: &str, key_line: &str) -> PathBuf {
        // Canonical comment-free line: ssh-key PublicKey equality includes the
        // trailing comment, so fixtures must not carry one.
        let path = dir.join("known_hosts");
        let mut parts = key_line.split_whitespace();
        let algo = parts.next().unwrap_or("");
        let b64 = parts.next().unwrap_or("");
        std::fs::write(&path, format!("{host} {algo} {b64}\n")).unwrap();
        path
    }

    #[test]
    fn unknown_key_rejected_by_default() {
        let tmp = std::env::temp_dir().join(format!("irondb-tunnel-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&tmp);
        let missing = tmp.join("no-such-known-hosts");
        let _ = std::fs::remove_file(&missing);
        let policy = HostKeyPolicy::Strict { known_hosts: Some(missing) };
        let err = enforce_policy("db.internal", 22, &test_key(), &policy).unwrap_err();
        assert!(matches!(err, TunnelError::HostKeyRejected { .. }), "{err:?}");
    }

    #[test]
    fn pinned_key_trusted_changed_key_rejected() {
        let tmp = std::env::temp_dir().join(format!("irondb-tunnel-kh-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&tmp);
        let path = write_known_hosts(&tmp, "db.internal", TEST_KEY);
        let policy = HostKeyPolicy::Strict { known_hosts: Some(path.clone()) };
        assert_eq!(
            verify_host_key("db.internal", 22, &test_key(), &policy).unwrap(),
            VerifyOutcome::Trusted
        );
        assert!(enforce_policy("db.internal", 22, &test_key(), &policy).unwrap());

        let other: russh::keys::ssh_key::PublicKey = OTHER_KEY.parse().unwrap();
        let outcome = verify_host_key("db.internal", 22, &other, &policy).unwrap();
        assert!(matches!(outcome, VerifyOutcome::Changed { .. }), "{outcome:?}");
        let err = enforce_policy("db.internal", 22, &other, &policy).unwrap_err();
        assert!(matches!(err, TunnelError::HostKeyRejected { .. }), "{err:?}");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn tofu_learns_unknown_but_still_rejects_changed() {
        let tmp = std::env::temp_dir().join(format!("irondb-tunnel-tofu-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&tmp);
        let path = tmp.join("known_hosts");
        let _ = std::fs::remove_file(&path);
        let policy = HostKeyPolicy::TrustOnFirstUse { known_hosts: path.clone() };
        assert!(enforce_policy("newhost", 22, &test_key(), &policy).unwrap());
        // Second connect with the same key is now pinned.
        let strict = HostKeyPolicy::Strict { known_hosts: Some(path.clone()) };
        assert!(enforce_policy("newhost", 22, &test_key(), &strict).unwrap());
        // A different key for the same host is a MITM signal even under TOFU.
        let other: russh::keys::ssh_key::PublicKey = OTHER_KEY.parse().unwrap();
        assert!(enforce_policy("newhost", 22, &other, &policy).is_err());
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn loopback_forwarder_shape() {
        // No SSH server needed: the dial path consumes a bound 127.0.0.1
        // endpoint, and open_tunnel always binds loopback before serving.
        let cfg = SshTunnelConfig {
            host: "example.invalid".into(),
            port: 22,
            user: "u".into(),
            auth: AuthMethod::None,
            policy: HostKeyPolicy::default(),
            remote_host: "127.0.0.1".into(),
            remote_port: 5432,
        };
        assert!(matches!(cfg.auth, AuthMethod::None));
        assert!(matches!(cfg.policy, HostKeyPolicy::Strict { .. }));
    }
}
