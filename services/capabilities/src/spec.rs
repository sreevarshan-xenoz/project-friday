//! Shared capability *resolution* contract.
//!
//! These types are the workspace-wide agreement for "what can be executed and
//! how it is dispatched". They are deliberately owned by `friday-capabilities`
//! (not the planner) so that native adapters, MCP adapters, and future plugins
//! can all construct a [`CapabilitySpec`] without depending on the planner
//! crate. The planner's [`crate::execution::capability_registry::CapabilityRegistry`]
//! resolves a canonical capability id to an [`ExecutionBinding`] and gates on
//! [`Risk`] — it consumes these types but does not define them.
//!
//! This is the *resolution* model: one capability id → one executable binding.
//! It is intentionally narrower than the rich provider model in [`crate::model`];
//! the two coexist (provider intelligence feeds ranking, resolution feeds
//! dispatch).

use serde::{Deserialize, Serialize};

/// Risk classification of a capability. Drives the permission policy but is
/// declared as metadata on the capability itself (see the architecture note in
/// `docs/`): the engine never special-cases risk by string matching.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum Risk {
    /// Execute immediately, no gate. e.g. `cargo.check`, `git.status`.
    #[default]
    Safe,
    /// Controlled by workspace/project policy, not user interaction.
    /// e.g. `filesystem.write_file`, `git.branch`.
    Medium,
    /// Always pause and ask the user before executing.
    /// e.g. `git.push`, `filesystem.delete`, `terminal.rm_rf`.
    High,
}

impl Risk {
    pub fn as_str(&self) -> &'static str {
        match self {
            Risk::Safe => "safe",
            Risk::Medium => "medium",
            Risk::High => "high",
        }
    }
}

/// Provenance of a capability. Lets the system filter, debug, and apply trust
/// policies ("only native tools", "prefer MCP tools") without special-casing
/// the registry. Replaces the old free-form `source: String`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum CapabilitySource {
    /// Built into FRIDAY (filesystem/git/terminal adapters).
    Native,
    /// A loaded plugin, identified by name.
    Plugin(String),
    /// A Model Context Protocol server, identified by server name.
    Mcp(String),
    /// Reserved for capabilities served by a remote FRIDAY node.
    FutureRemote,
}

impl CapabilitySource {
    /// Stable string tag (e.g. for `CapabilitySpec::source_tag`).
    pub fn tag(&self) -> String {
        match self {
            CapabilitySource::Native => "native".to_string(),
            CapabilitySource::Plugin(name) => format!("plugin:{name}"),
            CapabilitySource::Mcp(name) => format!("mcp:{name}"),
            CapabilitySource::FutureRemote => "remote".to_string(),
        }
    }
}

/// Live availability of a capability for *discovery* purposes. External tools
/// (MCP servers, remote nodes) can go offline; discovery should ignore
/// [`CapabilityHealth::Offline`] capabilities so planning never wastes context
/// on unavailable tools. This is the lightweight discovery-health distinct from
/// the richer provider [`crate::model::Health`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum CapabilityHealth {
    /// Reachable and usable.
    #[default]
    Healthy,
    /// Reachable but degraded (slow / partial). Still offered, ranked lower.
    Degraded,
    /// Unreachable. Excluded from discovery.
    Offline,
}

impl CapabilityHealth {
    /// A capability is discoverable unless it is [`CapabilityHealth::Offline`].
    pub fn discoverable(&self) -> bool {
        !matches!(self, CapabilityHealth::Offline)
    }
}

/// A resolved, executable binding for a capability.
///
/// The `adapter`/`action` pair is what the action runtime dispatches; `schema`
/// documents the accepted parameters; `permissions` lists the policy
/// permission strings required; `risk` gates execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionBinding {
    /// Routing key for the action runtime (the `ActionAdapter::tool` value).
    pub adapter: String,
    /// Verb within the adapter (the `ActionRequest.action` value).
    pub action: String,
    /// JSON Schema (or free-form description) of accepted parameters.
    pub schema: serde_json::Value,
    /// Policy permission strings the binding requires.
    pub permissions: Vec<String>,
    /// Risk classification of executing this binding.
    pub risk: Risk,
}

/// A self-describing, fully-resolved capability registration. Mirrors the shape
/// used by native adapters, MCP servers, and plugins so all three register
/// through one shape.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapabilitySpec {
    /// Canonical id, e.g. `filesystem.write_file`, `git.push`, `mcp:fs.read`.
    pub id: String,
    pub name: String,
    pub description: String,
    /// Provenance: `native`, `mcp:<server>`, `plugin:<name>`, or remote.
    pub source: CapabilitySource,
    pub binding: ExecutionBinding,
}

impl CapabilitySpec {
    /// Build a native capability spec concisely.
    pub fn native(
        id: impl Into<String>,
        name: impl Into<String>,
        description: impl Into<String>,
        adapter: impl Into<String>,
        action: impl Into<String>,
        risk: Risk,
    ) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            description: description.into(),
            source: CapabilitySource::Native,
            binding: ExecutionBinding {
                adapter: adapter.into(),
                action: action.into(),
                schema: serde_json::Value::Null,
                permissions: Vec::new(),
                risk,
            },
        }
    }

    /// Stable provenance tag, e.g. `"native"` / `"mcp:filesystem"`.
    pub fn source_tag(&self) -> String {
        self.source.tag()
    }
}
