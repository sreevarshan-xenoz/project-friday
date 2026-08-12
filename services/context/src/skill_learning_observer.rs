//! Adapter from the planner's verified completion boundary into skill learning.

use std::sync::Arc;

use friday_planner::execution::{CompletedGoal, GoalCompletionSink};
use friday_skills::{OutcomeSummary, TrajectorySkill, TrajectoryStep};

use crate::SkillLearningService;

/// Production adapter installed on the planner supervisor. The supervisor has
/// already waited for every task's verification event before calling this
/// sink, so this adapter only maps the neutral payload and creates a disabled
/// candidate. It deliberately never performs admission.
pub struct SkillLearningObserver {
    learning: Arc<SkillLearningService>,
}

impl SkillLearningObserver {
    pub fn new(learning: Arc<SkillLearningService>) -> Self {
        Self { learning }
    }
}

impl GoalCompletionSink for SkillLearningObserver {
    fn on_completed_goal(&self, goal: CompletedGoal) {
        if goal.steps.is_empty() {
            return;
        }
        let trajectory = TrajectorySkill {
            goal: goal.goal,
            outcome: OutcomeSummary::Verified {
                evidence_ids: goal
                    .steps
                    .iter()
                    .map(|step| step.task_id.to_string())
                    .collect(),
            },
            steps: goal
                .steps
                .into_iter()
                .map(|step| TrajectoryStep {
                    instruction: step.instruction,
                    capability: step.capability,
                })
                .collect(),
            confidence: 1.0,
            support: 1,
        };
        if let Err(error) = self.learning.learn(&trajectory) {
            tracing::debug!(%error, "verified supervisor trajectory was not compoundable");
        }
    }
}
