//! The `Planner` trait — the single contract every planner implements. Both the
//! deterministic [`RulePlanner`] and the model-driven [`LlmPlanner`] satisfy it,
//! and the supervisor consumes only the resulting `TaskGraph`, never the planner
//! that made it.

use async_trait::async_trait;

use crate::execution::lifecycle::ExecutionBudget;
use crate::intent::Intent;
use crate::planning::planners::context::PlanContext;
use crate::planning::planners::PlanError;
use crate::planning::TaskGraph;
use friday_core::trace::{ModelUsage, TraceCollector};

/// The result of a planning operation, carrying the validated task graph and
/// optional model usage metadata. When the planner called a reasoning backend,
/// `usage` contains the real token/cost data from the provider. When the rule
/// planner produced the graph, `usage` is `None`.
#[derive(Debug)]
pub struct PlanResult {
    pub graph: TaskGraph,
    /// Real model usage from the reasoning backend (None for rule planner).
    pub usage: Option<ModelUsage>,
    /// Total wall-clock time for the LLM call (None for rule planner).
    pub llm_duration_ms: Option<u64>,
}

/// A planner turns an [`Intent`] (plus grounding [`PlanContext`]) into an
/// executable [`TaskGraph`]. Implementations must be deterministic in their
/// *contract*: given the same inputs they yield a graph the supervisor can run.
#[async_trait]
pub trait Planner: Send + Sync {
    /// Which kind of planner this is (used for telemetry / routing only).
    fn kind(&self) -> crate::planning::planners::PlannerKind;

    /// Produce a validated plan. May use a reasoning backend; must surface
    /// backend/parse/validation failures as [`PlanError`].
    ///
    /// When `tracer` is provided, the planner should emit child spans for
    /// sub-operations (context building, routing, model calls). The `trace_ctx`
    /// provides the hierarchy context for creating child spans.
    async fn create_plan(
        &self,
        intent: &Intent,
        ctx: &PlanContext,
        tracer: &dyn TraceCollector,
        trace_ctx: &friday_core::trace::CognitiveTraceContext,
    ) -> Result<PlanResult, PlanError>;
}

/// Convenience constructor for the default bounded budget used by planners when
/// none is supplied by the host.
pub fn default_budget() -> ExecutionBudget {
    ExecutionBudget::default()
}
