//! # FRIDAY Action Runtime
//!
//! The decoupled dispatcher that every capability plugin flows through. It sits
//! between the planner's Execution Engine (which decides *what* to do) and the
//! capability adapters (which know *how* to do it), and is responsible only for
//! permission gating, dispatch with timeout + retries, telemetry, and audit.
//!
//! The runtime is deliberately ignorant of terminals, files, the browser, or
//! Windows — those concerns live entirely in [`adapters`]. See `docs/Execution.md`.

pub mod adapters;
pub mod audit;
pub mod kernel;
pub mod permission;
pub mod request;
pub mod runtime;

pub use adapters::{ActionAdapter, FilesystemAdapter, GitAdapter, TerminalAdapter};
pub use audit::{AuditEngine, AuditEntry};
pub use kernel::{
    CancellationToken, ExecutionContext, ExecutionContextBuilder, Kernel, LifecycleError,
    LifecycleManager, LifecycleState, ManagedResource, MetricSample, Metrics, ResourceId,
    ResourceManager, Session, SessionManager,
};
pub use permission::{PermissionDecision, PermissionEngine};
pub use request::{ActionRequest, ActionResult};
pub use runtime::{AgentKernel, DEFAULT_MAX_RETRIES, DEFAULT_TIMEOUT};
