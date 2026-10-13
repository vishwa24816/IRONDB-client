//! SSH tunnel domain: config, host-key policy, local forwarder.
//!
//! Mirrors `internal/ssh` (Go): direct-tcpip forwarding whose bound local
//! port is handed to driver dial paths. Host-key verification is strict by
//! default (T-06-08); TOFU learning only runs after explicit user trust.

pub mod kerberos;
pub mod ssh;

pub use ssh::{AuthMethod, HostKeyPolicy, SshTunnelConfig, TunnelHandle, VerifyOutcome};
