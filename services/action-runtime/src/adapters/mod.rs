//! Capability adapters: the only place that knows how to talk to the outside
//! world.
//!
//! Each adapter implements [`ActionAdapter`] and is registered with the
//! [`crate::runtime::AgentKernel`] under a `tool` name. The kernel routes an
//! `exec.action` to the adapter whose `tool()` matches, then calls
//! [`ActionAdapter::execute`]. Adapters must never make permission decisions,
//! never publish events, and never talk to other adapters — the runtime and the
//! planner are the only coordinators.

use async_trait::async_trait;

use crate::request::{ActionRequest, ActionResult};

pub mod filesystem;
pub mod git;
pub mod mcp;
pub mod sim;
pub mod terminal;

pub use filesystem::FilesystemAdapter;
pub use git::GitAdapter;
pub use mcp::McpAdapter;
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

/// Aggregate the capabilities advertised by every built-in service adapter.
/// Consumed by the Capability Intelligence registry at startup. The canonical
/// definitions live in `friday_capabilities::builtins` so the metadata is
/// defined once and shared with the planner.
pub fn all_capabilities() -> Vec<friday_capabilities::model::Capability> {
    friday_capabilities::builtins::all()
}
