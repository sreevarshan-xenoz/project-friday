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

/// A planner turns an [`Intent`] (plus grounding [`PlanContext`]) into an
/// executable [`TaskGraph`]. Implementations must be deterministic in their
/// *contract*: given the same inputs they yield a graph the supervisor can run.
#[async_trait]
pub trait Planner: Send + Sync {
    /// Which kind of planner this is (used for telemetry / routing only).
    fn kind(&self) -> crate::planning::planners::PlannerKind;

    /// Produce a validated plan. May use a reasoning backend; must surface
    /// backend/parse/validation failures as [`PlanError`].
    async fn create_plan(
        &self,
        intent: &Intent,
        ctx: &PlanContext,
    ) -> Result<TaskGraph, PlanError>;
}

/// Convenience constructor for the default bounded budget used by planners when
/// none is supplied by the host.
pub fn default_budget() -> ExecutionBudget {
    ExecutionBudget::default()
}
