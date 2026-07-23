//! The deterministic planner. It reuses the existing rule-based decomposition
//! (`browser.search`, `open.app`, `routine.morning`, generic) so the behavior
//! the system already had is preserved as one of several `Planner`
//! implementations. It does not consult the capability list — it is the
//! "don't pay the model for a file copy" path.

use async_trait::async_trait;

use crate::intent::Intent;
use crate::planning::planners::context::PlanContext;
use crate::planning::planners::planner_trait::Planner;
use crate::planning::planners::PlanError;
use crate::planning::planners::PlannerKind;
use crate::planning::planner::plan as rule_plan;
use crate::planning::TaskGraph;

/// Deterministic, intent-type-driven planner. Always succeeds (it never calls a
/// model) and produces the same graph the rule engine always did.
pub struct RulePlanner;

impl Default for RulePlanner {
    fn default() -> Self {
        Self::new()
    }
}

impl RulePlanner {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Planner for RulePlanner {
    fn kind(&self) -> PlannerKind {
        PlannerKind::Rule
    }

    async fn create_plan(
        &self,
        intent: &Intent,
        _ctx: &PlanContext,
    ) -> Result<TaskGraph, PlanError> {
        Ok(rule_plan(intent))
    }
}
