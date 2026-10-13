//! Sidecar harness: spawn + JSON-lines IPC + method table (DRV-02).
//! Verbatim port of Go `optionalAgentRequest/Response` framing.

pub mod cancel;
pub mod jmx;
pub mod methods;
pub mod ocr;
pub mod protocol;
pub mod spawn;

pub use protocol::{AgentRequest, AgentResponse, RowBudgetOptions};
pub use spawn::{CONTROL_CALL_TIMEOUT, METADATA_PROBE_RETRY_TIMEOUT, METADATA_PROBE_TIMEOUT, SHUTDOWN_TIMEOUT, SidecarClient, validate_executable};
pub use cancel::{CancelHandle, CancelRegistry};
