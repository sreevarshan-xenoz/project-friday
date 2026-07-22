//! ExperienceProvider trait (M13).
//!
//! The trait decouples the planning context builder from the concrete
//! `ExperienceService`. This enables:
//!
//! - Unit testing with mock providers
//! - Alternative evidence sources (cached, hybrid, remote)
//! - Swapping retrieval engines without touching the builder or planner

use crate::experience::{
    ExperienceMatch, ReflectionRecord, WorkflowSuggestion,
};
use crate::experience::indexer::GoalRecord;
use uuid::Uuid;

/// Abstraction over evidence retrieval. The builder depends on this,
/// not the concrete ExperienceService.
pub trait ExperienceProvider {
    /// Find similar past goals.
    fn similar(&self, goal_title: &str, max_results: usize) -> Vec<ExperienceMatch>;

    /// Most recent attempt for a specific goal.
    fn last_attempt(&self, goal_id: Uuid) -> Option<ExperienceMatch>;

    /// Workflows that succeeded for similar goals.
    fn successful_workflows(&self, goal_title: &str) -> Vec<GoalRecord>;

    /// Workflow suggestions from pattern mining.
    fn suggestions(&self) -> Vec<WorkflowSuggestion>;

    /// Load reflections for specific goals.
    fn load_reflections(&self, goal_ids: &[Uuid]) -> Vec<ReflectionRecord>;
}
