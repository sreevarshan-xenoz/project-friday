//! FRIDAY Recovery subsystem (Reliability Sprint).
//!
//! Models failure and recovery as a deterministic, action-level graph:
//!
//! ```text
//! FailureCategory ──▶ RecoveryPolicy ──▶ RecoveryAction
//! ```
//!
//! See `specs/recovery.md`. The headline metric it exists to move is
//! **Chaos MTBF** (mean tasks between failures under `CHAOS=1`).

pub mod defaults;
pub mod policy;
pub mod taxonomy;

pub use defaults::{default_graph, from_json, to_json};
pub use policy::{RecoveryGraph, RecoveryPolicy, RecoveryResult, APPROVAL_THRESHOLD};
pub use taxonomy::{FailureCategory, RecoveryAction};
