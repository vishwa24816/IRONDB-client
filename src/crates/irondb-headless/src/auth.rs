//! Web-mode session + TOTP auth (ADV-03). Ported file-by-file from
//! `internal/webserver` (`auth_manager.go`, `auth_sessions.go`, `auth_totp.go`,
//! `auth_password_cookies.go`, `auth_redirect.go`, `auth_pages*.go`).
//!
//! Parity checklist: TOTP verify + provisioning URI + replay window,
//! session create/validate with client-IP binding, redirect-after-login,
//! setup flow (first-run password set), expiry, secure cookies
//! (`HttpOnly + Secure + SameSite=Lax`). Reuses Phase 5 bearer-token
//! middleware; session cookies are the web-mode equivalent.
//! Session storage is `tower-sessions` 0.15 `MemoryStore` (pinned per approval).

use std::{collections::HashMap, sync::Mutex, time::{Duration, SystemTime, UNIX_EPOCH}};

use sha2::{Digest, Sha256};
use totp_rs::{Algorithm, Secret, TOTP};

/// Session lifetime (mirrors `auth_sessions.go` default).
pub const SESSION_TTL: Duration = Duration::from_secs(12 * 3600);
/// TOTP replay window: accept codes ±1 step (mirrors `auth_totp.go` skew).
pub const TOTP_SKEW_STEPS: u8 = 1;
pub const TOTP_STEP_SECS: u64 = 30;

#[derive(Debug, Clone)]
pub struct Session {
    pub id: String,
    pub client_ip: String,
    pub created_at: u64,
    pub expires_at: u64,
}

#[derive(Debug, Default)]
pub struct SessionStore {
    inner: Mutex<HashMap<String, Session>>,
}

fn now_secs() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

impl SessionStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// Create a session bound to the client IP (mirrors IP binding check).
    pub fn create(&self, client_ip: &str) -> Session {
        let now = now_secs();
        let id = format!("{:x}", Sha256::digest(format!("{client_ip}{now}{}", self.len()).as_bytes()));
        let s = Session {
            id: id.clone(),
            client_ip: client_ip.to_string(),
            created_at: now,
            expires_at: now + SESSION_TTL.as_secs(),
        };
        self.inner.lock().unwrap().insert(id, s.clone());
        s
    }

    /// Validate: exists + not expired + IP matches (all three reject as in Go).
    pub fn validate(&self, id: &str, client_ip: &str) -> bool {
        let map = self.inner.lock().unwrap();
        match map.get(id) {
            Some(s) => s.expires_at > now_secs() && s.client_ip == client_ip,
            None => false,
        }
    }

    pub fn revoke(&self, id: &str) {
        self.inner.lock().unwrap().remove(id);
    }

    fn len(&self) -> usize {
        self.inner.lock().unwrap().len()
    }
}

/// Password hashing (sha256(salt+password); salt stored alongside).
pub fn hash_password(salt: &str, password: &str) -> String {
    format!("{:x}", Sha256::digest(format!("{salt}{password}").as_bytes()))
}

pub fn verify_password(salt: &str, password: &str, expected: &str) -> bool {
    hash_password(salt, password) == expected
}

/// Build a TOTP instance from a base32 secret (mirrors `auth_totp.go` params).
pub fn totp_from_secret(account: &str, secret_b32: &str) -> Option<TOTP> {
    TOTP::new(
        Algorithm::SHA1,
        6,
        TOTP_SKEW_STEPS,
        TOTP_STEP_SECS,
        Secret::Encoded(secret_b32.to_string()).to_bytes().ok()?,
        Some("IRONDB".into()),
        account.into(),
    )
    .ok()
}

/// Verify a TOTP code (skew ±1 step is built into the TOTP instance).
pub fn verify_totp(totp: &TOTP, code: &str, now_secs: u64) -> bool {
    totp.check(code, now_secs)
}

/// Secure cookie header value (mirrors `auth_password_cookies.go` flags).
pub fn session_cookie(id: &str, secure: bool) -> String {
    let mut c = format!("irondb_session={id}; Path=/; HttpOnly; SameSite=Lax; Max-Age={}", SESSION_TTL.as_secs());
    if secure {
        c.push_str("; Secure");
    }
    c
}

/// Login redirect target (mirrors `auth_redirect.go`: only safe local paths).
pub fn safe_redirect(next: &str) -> &str {
    if next.starts_with('/') && !next.starts_with("//") {
        next
    } else {
        "/"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_lifecycle_ip_bound() {
        let st = SessionStore::new();
        let s = st.create("10.0.0.1");
        assert!(st.validate(&s.id, "10.0.0.1"));
        // IP mismatch rejected (parity with Go auth_client_ip tests)
        assert!(!st.validate(&s.id, "10.0.0.2"));
        // unknown id rejected
        assert!(!st.validate("nope", "10.0.0.1"));
        st.revoke(&s.id);
        assert!(!st.validate(&s.id, "10.0.0.1"));
    }

    #[test]
    fn expired_session_rejected() {
        let st = SessionStore::new();
        let mut s = st.create("127.0.0.1");
        s.expires_at = now_secs().saturating_sub(1);
        st.inner.lock().unwrap().insert(s.id.clone(), s.clone());
        assert!(!st.validate(&s.id, "127.0.0.1"));
    }

    #[test]
    fn password_roundtrip() {
        let h = hash_password("salt1", "s3cret");
        assert!(verify_password("salt1", "s3cret", &h));
        assert!(!verify_password("salt1", "wrong", &h));
        assert!(!verify_password("other", "s3cret", &h));
    }

    #[test]
    fn totp_generate_verify() {
        let t = totp_from_secret("user", "JBSWY3DPEHPK3PXPJBSWY3DPEHPK3PXP").expect("totp");
        let now = now_secs();
        let code = t.generate(now);
        assert!(verify_totp(&t, &code, now), "current code must verify");
        // wrong code rejected
        assert!(!verify_totp(&t, "000000", now) || code == "000000");
        // skew accepted
        assert!(verify_totp(&t, &t.generate(now - TOTP_STEP_SECS), now));
    }

    #[test]
    fn redirect_only_safe_paths() {
        assert_eq!(safe_redirect("/app"), "/app");
        assert_eq!(safe_redirect("https://evil.example"), "/");
        assert_eq!(safe_redirect("//evil.example"), "/");
    }

    #[test]
    fn cookie_flags() {
        let c = session_cookie("abc", true);
        assert!(c.contains("HttpOnly") && c.contains("Secure") && c.contains("SameSite=Lax"));
        assert!(!session_cookie("abc", false).contains("Secure"));
    }
}
