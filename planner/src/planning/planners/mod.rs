//! M6.2 — Planner pipeline.
//!
//! Both the [`RulePlanner`] and the [`LlmPlanner`] implement the same
//! [`Planner`] trait and produce the **same** `TaskGraph`. The supervisor never
//! learns which planner created a plan; it only ever sees a validated graph.
//!
//! The pipeline is staged and each stage is validated before the next runs:
//!
//! ```text
//! Intent → Capability Discovery → Task Generation → Dependency Analysis
//!        → TaskGraph Validation → (PLAN_CREATED)
//! ```
//!
//! Planners are intentionally "just reasoning modules": the [`LlmPlanner`] talks
//! to a [`ReasoningBackend`] (any model client — `InferenceFabric` behind the
//! `intelligence` feature) through a narrow trait, so adding intelligence never
//! requires touching execution.

pub mod backend;
pub mod context;
pub mod llm;
pub mod planner_trait;
pub mod router;
pub mod rule;
pub mod validation;

pub use backend::{CompletionResult, PlanError, ReasoningBackend};
pub use context::{AvailableCapability, PlanContext, RiskPolicy, WorkspaceContext};
pub use llm::LlmPlanner;
pub use planner_trait::{Planner, PlanResult};
pub use router::{PlannerKind, PlannerRouter};
pub use rule::RulePlanner;
pub use validation::validate_task_graph;
