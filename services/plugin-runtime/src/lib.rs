//! # FRIDAY Plugin Runtime
//!
//! The capability execution platform for FRIDAY's cognitive core.
//!
//! ## Architecture
//!
//! ```text
//! CapabilityRequest
//!        │
//!        ▼
//! CapabilityPlanner
//!        │
//!        ▼
//!   ExecutionPlan
//!        │
//!        ▼
//! PluginRuntime (execution only, no planning)
//!        │
//! ┌──────┴──────┐
//! ▼             ▼
//! Plugin    HealthMonitor
//!        │
//!        ▼
//!   PlanOutcome
//!        │
//!        ▼
//!   DecisionLedger (append-only, every execution recorded)
//!        │
//!        ▼
//!   CapabilityResult
//! ```
//!
//! ## Contracts
//!
//! - Every plugin implements the [`Plugin`] trait.
//! - Every execution enters the runtime as a [`CapabilityRequest`].
//! - Every execution returns a [`CapabilityResult`] with telemetry.
//! - The runtime owns health. The planner consumes health.
//! - No plugin bypasses the runtime.
//! - The cognitive core never knows which plugin was chosen.
//! - Only the planner may create an [`ExecutionPlan`].
//! - Only the runtime may execute an [`ExecutionPlan`].
//! - The runtime never modifies an [`ExecutionPlan`].
//! - The planner never executes a plan.
//! - Every execution produces exactly one [`DecisionRecord`].
//! - No outcome exists without a plan.
//! - No executed plan exists without a record.

mod utils;

pub mod bridge;
pub mod catalog;
pub mod contract;
pub mod filesystem;
pub mod health;
pub mod ledger;
pub mod policy;
pub mod registry;
pub mod planner;
pub mod runtime;
pub mod telemetry;

pub use bridge::{CognitiveBridge, ObservationKind, RuntimeObservation};
pub use catalog::CapabilityCatalog;
pub use contract::*;
pub use filesystem::FilesystemPlugin;
pub use health::HealthMonitor;
pub use ledger::DecisionLedger;
pub use policy::{
    DefaultPolicyEngine, LayeredPolicyEngine, OrganizationPolicy, PolicyEngine, ResourcePolicy,
    RuleBasedPolicyEngine, SecurityPolicy, UserPolicy,
};
pub use registry::CapabilityRegistry;
pub use planner::CapabilityPlanner;
pub use runtime::PluginRuntime;
pub use telemetry::{TelemetryCollector, TelemetryObserver, TelemetryStore};
