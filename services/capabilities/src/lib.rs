//! FRIDAY Capability Intelligence (M4.6) — the "Knowledge" subsystem.
//!
//! Single source of truth for *what the platform can do, who can do it, and how
//! reliable/expensive each option is*. The planner reasons over capabilities and
//! ranked providers instead of hardcoded plugin names. See
//! `docs/CapabilityIntelligence.md`.
//!
//! Modules:
//! - [`model`]   — rich `Capability` + `Health`/`Platform`
//! - [`ranking`]
//! - [`registry`]— `CapabilityRegistry` (provider → capabilities)
//! - [`health`]  — health states, probes, and the self-healing `Monitor`
//! - [`manifest`]
//! - [`graph`]   — hierarchical `CapabilityGraph` for LLM traversal

pub mod cost;
pub mod graph;
pub mod health;
pub mod manifest;
pub mod model;
pub mod orchestrator;
pub mod ranking;
pub mod registry;

pub use cost::{CostEstimator, PlanCost};
pub use graph::{CapabilityGraph, GraphNode};
pub use health::{HealthProbe, Monitor};
pub use manifest::ServiceManifest;
pub use model::{Capability, Health, Platform};
pub use orchestrator::{
    RecoveryPolicy, RecoveryStep, ServiceOrchestrator, ServiceProbe, ServiceState, ServiceStatus,
    StaticRecoveryPolicy, Telemetry,
};
pub use ranking::score;
pub use registry::CapabilityRegistry;
