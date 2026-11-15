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

pub mod browser;
pub mod codeintel;
pub mod desktop;
pub mod docs;
pub mod document;
pub mod filesystem;
pub mod git;
pub mod mcp;
pub mod provider;
pub mod reasoning;
pub mod sim;
pub mod telemetry;
pub mod terminal;
#[cfg(target_os = "linux")]
pub mod ui;
pub mod web;
pub mod webagent;

pub use browser::{browser_capability_specs, register_browser_capabilities, BrowserAdapter};
pub use codeintel::CodeIntelAdapter;
pub use desktop::{
    hypr_signature, risk_of as desktop_risk_of, DesktopAdapter, Exec as DesktopExec,
    Risk as DesktopRisk,
};
pub use docs::DocsAdapter;
pub use document::{
    parse_document, Document, DocumentAsset, DocumentId, DocumentMetadata, DocumentSource,
};
pub use filesystem::FilesystemAdapter;
pub use git::GitAdapter;
pub use mcp::McpAdapter;
pub use provider::{
    ExpertProfile, ProviderCapabilities, ProviderCapability, ProviderEntry, ProviderId,
    ProviderRegistry, Transport,
};
pub use reasoning::{
    reasoning_capability_specs, register_reasoning_capabilities, BrowserReasoningProvider,
    ConfidenceTier, ConsultResult, ExpertRouter, LatencyTier, ReasoningAdapter, ReasoningProvider,
    ReasoningRequest, RoutingPolicy,
};
pub use telemetry::{MeasuredOutcome, ProviderStats, ProviderTelemetry};
pub use terminal::TerminalAdapter;
#[cfg(target_os = "linux")]
pub use ui::{
    is_password_role, label_is_dangerous, ui_risk_of, validate_path, AppInfo, AtspiBackend,
    NodeDetails, ResolvedTarget, UiAdapter, UiBackend, UiRisk,
};
pub use web::{urlencode, WebAdapter};
pub use webagent::{
    agent_for_site, register_webagent_capabilities, webagent_capability_specs, ChatGPTAdapter,
    ClaudeAdapter, GeminiAdapter, GenericWebAgent, GrokAdapter, PerplexityAdapter, WebAgent,
    WebAgentAdapter, WebAgentResponse,
};

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
