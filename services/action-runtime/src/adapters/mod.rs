//! Capability adapters: the only place that knows how to talk to the outside
//! world.
//!
//! Each adapter implements [`ActionAdapter`] and is registered with the
//! [`crate::runtime::ActionRuntime`] under a `tool` name. The runtime routes an
//! `exec.action` to the adapter whose `tool()` matches, then calls
//! [`ActionAdapter::execute`]. Adapters must never make permission decisions,
//! never publish events, and never talk to other adapters — the runtime and the
//! planner are the only coordinators.

use async_trait::async_trait;

use crate::request::{ActionRequest, ActionResult};

pub mod filesystem;
pub mod git;
pub mod terminal;

pub use filesystem::FilesystemAdapter;
pub use git::GitAdapter;
pub use terminal::TerminalAdapter;

/// A swappable capability. Implementors handle a normalized
/// [`ActionRequest`] and return a normalized [`ActionResult`].
#[async_trait]
pub trait ActionAdapter: Send + Sync {
    /// Routing key, e.g. `filesystem`. Must be unique among registered adapters.
    fn tool(&self) -> &str;

    /// The action verbs this adapter understands (used for discovery/logging).
    fn actions(&self) -> Vec<&str>;

    /// Execute a single normalized request.
    async fn execute(&self, req: ActionRequest) -> ActionResult;
}
