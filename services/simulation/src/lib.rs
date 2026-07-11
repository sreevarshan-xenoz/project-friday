//! # FRIDAY Plan Simulation Engine (M4.7)
//!
//! A **Digital Twin of the agent**: the planner produces a `TaskGraph`, and this
//! engine virtualizes the entire execution against an immutable [`WorldState`]
//! snapshot — validating preconditions, simulating side effects, forecasting
//! resources, deriving a confidence score, generating fallback paths, and emitting
//! a machine-readable [`SimulationReport`] (plus a human-readable render).
//!
//! Deterministic by design: no learning, no ML. Prediction improves later at M4.9
//! (Plan Optimizer) and M9 (Learning) against the stored reports.

pub mod approval;
pub mod estimator;
pub mod optimizer;
pub mod report;
pub mod simulator;
pub mod validator;

pub use approval::{ApprovalDecision, ApprovalRequest, PlanSummary, RiskLevel};
pub use estimator::{aggregate, PlanEstimate};
pub use optimizer::{optimize, OptimizerDiff};
pub use report::render_report;
pub use simulator::{FallbackPath, SimulationEngine, SimulationReport};
pub use validator::{validate, IssueSeverity, ValidationIssue, ValidationResult};
