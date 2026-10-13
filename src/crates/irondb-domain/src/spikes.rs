//! Wave-0 protocol spikes (Phase 6 Plan 01). Notes only — no implementation.
//!
//! JMX-helper stdout-JSON protocol (verbatim from
//! `tools/jmx-helper/.../JmxHelperMain.java` + `JmxRuntime`):
//! request: arbitrary JSON object on stdin (UTF-8, fully consumed via MiniJson).
//! success: `{...JmxRuntime.handle(request), "ok": true}` on stdout.
//! failure: `{"ok": false, "error": message-or-exception-name,
//! "details": {"exception": class-name}}` — never throws to stderr for control flow.
//! Rust side: spawn java helper, write JSON to stdin, parse single stdout JSON
//! object; reuse Phase-4 sidecar harness. `javac/java 21.0.12.1` present.
//!
//! OCR protocol (`internal/ocr/{manager,server,download,component}.go`):
//! manager + local server shape + model-download/provisioning flow; treat as
//! out-of-process daemon — thin Rust client only, never in-process CV.
//!
//! Crate compile notes (verify with `cargo search` at plan time):
//! `rmcp` 3.5.1 (server + transport-child-process), `russh` 0.64.1
//! (aws-lc-rs feature), `tokio-cron-scheduler` 0.15.1, `async-openai` 0.42.2
//! all version-verified on crates.io 2026-10-12; `odbc-api`, `calamine`,
//! `rust_xlsxwriter`, `totp-rs` are ASSUMED — gate per-engine/plan.

pub const JMX_OK_FIELD: &str = "ok";
pub const JMX_ERROR_FIELD: &str = "error";
