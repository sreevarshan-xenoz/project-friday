//! The FRIDAY execution **kernel**: the stable substrate every capability,
//! session, and resource depends on.
//!
//! This module is intentionally free of any specific capability (terminal,
//! filesystem, browser). It defines the four invariants the platform is built
//! on after M4.4.5:
//!
//! * [`context`] — an immutable [`ExecutionContext`] carried by every action,
//!   with parent/child inheritance and merge for parallel DAG branches.
//! * [`lifecycle`] — a single [`LifecycleManager`] state machine shared by
//!   tasks, sessions, resources, and workflows.
//! * [`resource`] — a [`ResourceManager`] that is the *sole* owner of long-lived
//!   resources; plugins never hold them directly.
//! * [`session`] — a [`SessionManager`] that preserves continuity (cwd, env,
//!   history, auth) so work feels like it continues rather than restarts.
//! * [`metrics`] — automatic per-execution measurement so the platform can be
//!   reasoned about with data, not guesses.
//!
//! Once M4.4.5 lands these contracts are declared **stable**; M4.5, M4.6 and
//! M5+ build on top without changing them. See `docs/Roadmap.md`.

pub mod context;
pub mod lifecycle;
pub mod metrics;
pub mod resource;
pub mod session;

/// The bundled kernel services. The Action Runtime owns one instance; it is the
/// single place these cross-cutting concerns live so they stay consistent and
/// auditable.
pub struct Kernel {
    pub lifecycle: LifecycleManager,
    pub metrics: Metrics,
    pub sessions: SessionManager,
    pub resources: ResourceManager,
}

impl Default for Kernel {
    fn default() -> Self {
        Self::new()
    }
}

impl Kernel {
    pub fn new() -> Self {
        Self {
            lifecycle: LifecycleManager::new(),
            metrics: Metrics::new(4096),
            sessions: SessionManager::new(),
            resources: ResourceManager::new(),
        }
    }
}

pub use context::{CancellationToken, ExecutionContext, ExecutionContextBuilder};
pub use lifecycle::{LifecycleError, LifecycleManager, LifecycleState};
pub use metrics::{MetricSample, Metrics};
pub use resource::{ManagedResource, ResourceId, ResourceManager};
pub use session::{Session, SessionManager};
