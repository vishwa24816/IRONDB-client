//! Kerberos / GSSAPI spike (ADV-04 decision record).
//!
//! Evaluated at task time (crates.io, 2026-10): no mature pure-Rust crate
//! implements SSH GSSAPI key exchange (`gssapi-with-mic`) against both
//! Windows SSPI and MIT/Heimdal Kerberos. The `sspi` crate covers
//! Windows-only NTLM/Negotiate surfaces and cannot drive OpenSSH-style
//! `gssapi-with-mic` auth from a russh channel; `gssapi`/`libgssapi` bind
//! system libs and break the static single-binary story.
//!
//! **Decision: route Kerberos through sidecar reuse.** The existing Go
//! `internal/ssh` path (which already negotiates GSSAPI via
//! `golang.org/x/crypto/ssh` + system Kerberos) stays reachable through
//! the Phase-4 `irondb-sidecar` harness, exactly like the JMX helper.
//! This module therefore carries only the config/negotiation types and
//! the sidecar method names so a future native implementation can slot
//! in without touching callers.

use serde::{Deserialize, Serialize};

/// Sidecar method names (frozen; mirror the Go SSH agent surface).
pub const METHOD_KERBEROS_TICKET: &str = "ssh.kerberos.ticket";
pub const METHOD_KERBEROS_FORWARD: &str = "ssh.kerberos.forward";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KerberosConfig {
    /// Service principal, e.g. `host/db.example.com@EXAMPLE.COM`.
    pub service_principal: String,
    /// Request credential delegation (forwardable TGT). Default false:
    /// delegation spreads the TGT and must be an explicit user choice.
    #[serde(default)]
    pub delegate_credentials: bool,
}

impl KerberosConfig {
    pub fn validate(&self) -> Result<(), String> {
        if self.service_principal.trim().is_empty() {
            return Err("service principal must not be empty".into());
        }
        if !self.service_principal.contains('/') || !self.service_principal.contains('@') {
            return Err(format!(
                "service principal {:?} must look like service/host@REALM",
                self.service_principal
            ));
        }
        Ok(())
    }
}

/// Vehicle decision for Kerberos auth on this platform.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KerberosVehicle {
    /// No native vehicle: negotiate via the Go sidecar agent.
    Sidecar,
}

pub fn vehicle_for_platform() -> KerberosVehicle {
    KerberosVehicle::Sidecar
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vehicle_is_sidecar_until_native_gssapi_exists() {
        assert_eq!(vehicle_for_platform(), KerberosVehicle::Sidecar);
    }

    #[test]
    fn principal_shape_validated() {
        let bad = KerberosConfig {
            service_principal: "not-a-principal".into(),
            delegate_credentials: false,
        };
        assert!(bad.validate().is_err());
        let good = KerberosConfig {
            service_principal: "host/db.example.com@EXAMPLE.COM".into(),
            delegate_credentials: false,
        };
        assert!(good.validate().is_ok());
        assert!(!good.delegate_credentials, "delegation must default to off");
    }
}
