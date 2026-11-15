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
pub mod agent_driver;
pub mod audit;
pub mod egress;
pub mod kernel;
pub mod permission;
pub mod request;
pub mod runtime;

pub use agent_driver::SkillAgentDriver;

pub use adapters::sim::sim_registry;
pub use adapters::{
    agent_for_site, browser_capability_specs, desktop_risk_of, hypr_signature,
    reasoning_capability_specs, register_browser_capabilities, register_reasoning_capabilities,
    register_webagent_capabilities, urlencode, webagent_capability_specs, ActionAdapter,
    BrowserAdapter, BrowserReasoningProvider, ChatGPTAdapter, ClaudeAdapter, CodeIntelAdapter,
    ConsultResult, DesktopAdapter, DesktopExec, DesktopRisk, DocsAdapter, Document, DocumentAsset,
    DocumentId, DocumentMetadata, DocumentSource, ExpertProfile, ExpertRouter, FilesystemAdapter,
    GeminiAdapter, GenericWebAgent, GitAdapter, GrokAdapter, MeasuredOutcome, PerplexityAdapter,
    ProviderCapabilities, ProviderCapability, ProviderEntry, ProviderId, ProviderRegistry,
    ProviderStats, ProviderTelemetry, ReasoningAdapter, ReasoningProvider, TerminalAdapter,
    Transport, WebAdapter, WebAgent, WebAgentAdapter, WebAgentResponse,
};
pub use audit::{AuditEngine, AuditEntry};
pub use egress::{EgressDecision, EgressDenyReason, EgressPolicy};
pub use kernel::{
    CancellationToken, ExecutionContext, ExecutionContextBuilder, Kernel, LifecycleError,
    LifecycleManager, LifecycleState, ManagedResource, MetricSample, Metrics, ResourceId,
    ResourceManager, Session, SessionManager,
};
pub use permission::{PermissionDecision, PermissionEngine};
pub use request::{ActionRequest, ActionResult};
pub use runtime::{AgentKernel, DEFAULT_MAX_RETRIES, DEFAULT_TIMEOUT};
