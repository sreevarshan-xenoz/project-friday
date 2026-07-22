//! Workflow Replay (M12.4) — planner-mediated execution of stored workflows.
//!
//! Replay is NOT deterministic reproduction. It is a **planning accelerator**:
//! the planner receives a stored workflow as evidence, checks applicability,
//! and decides whether to reuse, adapt, or discard it.
//!
//! Pipeline:
//!
//! ```text
//! Workflow
//!     ↓
//! ApplicabilityCheck  — can this still run?
//!     ↓
//! Planner             — reuse? adapt? discard?
//!     ↓
//! Plan                — fresh plan, possibly informed by workflow
//!     ↓
//! GoalSupervisor → AgentKernel  — existing execution path
//!     ↓
//! WorkflowExecutionReport  — how much was reused vs adapted
//! ```
//!
//! The execution substrate (ADR-0007) is never modified. Nothing below the
//! planner changes.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::miner::Pattern;
use super::{GoalOutcome, Workflow};

// ---------------------------------------------------------------------------
// Applicability
// ---------------------------------------------------------------------------

/// Result of checking whether a workflow can still be executed.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApplicabilityResult {
    /// Whether the workflow is applicable in the current environment.
    pub applicable: bool,
    /// Capabilities required by the workflow that are not registered.
    pub missing_capabilities: Vec<String>,
    /// Artifact paths referenced by the workflow that no longer exist.
    pub missing_artifacts: Vec<String>,
    /// Permissions that would be required (informational).
    pub permission_requirements: Vec<String>,
    /// Human-readable notes about the applicability check.
    pub notes: Vec<String>,
}

/// Checks whether a workflow is still applicable in the current environment.
pub struct ApplicabilityChecker {
    /// Capabilities currently registered in the system.
    registered_capabilities: Vec<String>,
}

impl ApplicabilityChecker {
    pub fn new(registered_capabilities: Vec<String>, _existing_artifacts: Vec<String>) -> Self {
        Self {
            registered_capabilities,
        }
    }

    /// Check whether a workflow's required capabilities are available.
    pub fn check_workflow(&self, workflow: &Workflow) -> ApplicabilityResult {
        self.check_capabilities(&workflow.capabilities)
    }

    /// Check whether a pattern's required capabilities are available.
    pub fn check_pattern(&self, pattern: &Pattern) -> ApplicabilityResult {
        self.check_capabilities(&pattern.capabilities)
    }

    fn check_capabilities(&self, capabilities: &[String]) -> ApplicabilityResult {
        let mut missing_capabilities = Vec::new();
        let mut notes = Vec::new();

        for cap in capabilities {
            // Check if the capability (or its prefix) is registered.
            let base_cap = cap.split('.').next().unwrap_or(cap);
            let found = self.registered_capabilities.iter().any(|registered| {
                registered == cap
                    || registered.starts_with(&format!("{base_cap}."))
                    || cap.starts_with(&format!("{registered}."))
            });
            if !found {
                missing_capabilities.push(cap.clone());
            }
        }

        if missing_capabilities.is_empty() {
            notes.push("all capabilities available".to_string());
        } else {
            notes.push(format!(
                "{} capability(ies) missing: {}",
                missing_capabilities.len(),
                missing_capabilities.join(", ")
            ));
        }

        let applicable = missing_capabilities.is_empty();

        ApplicabilityResult {
            applicable,
            missing_capabilities,
            missing_artifacts: Vec::new(),
            permission_requirements: Vec::new(),
            notes,
        }
    }
}

// ---------------------------------------------------------------------------
// Workflow Execution Report
// ---------------------------------------------------------------------------

/// Tracks how a stored workflow was used during planning.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowExecutionReport {
    /// The workflow that was considered.
    pub workflow_id: Option<Uuid>,
    /// The pattern that was considered.
    pub pattern_id: Option<Uuid>,
    /// Steps that were reused exactly as stored.
    pub reused_steps: Vec<String>,
    /// Steps that were modified by the planner.
    pub adapted_steps: Vec<String>,
    /// Steps that were skipped (not needed in current context).
    pub skipped_steps: Vec<String>,
    /// Steps that were added by the planner (not in original workflow).
    pub new_steps: Vec<String>,
    /// When this execution was completed.
    pub completed_at: DateTime<Utc>,
    /// Outcome of the execution.
    pub outcome: GoalOutcome,
}

impl WorkflowExecutionReport {
    /// Create a new empty report.
    pub fn new() -> Self {
        Self {
            workflow_id: None,
            pattern_id: None,
            reused_steps: Vec::new(),
            adapted_steps: Vec::new(),
            skipped_steps: Vec::new(),
            new_steps: Vec::new(),
            completed_at: Utc::now(),
            outcome: GoalOutcome::Unknown,
        }
    }

    /// Total steps in the original workflow.
    pub fn total_original_steps(&self) -> usize {
        self.reused_steps.len() + self.adapted_steps.len() + self.skipped_steps.len()
    }

    /// Total steps in the final plan.
    pub fn total_final_steps(&self) -> usize {
        self.reused_steps.len() + self.adapted_steps.len() + self.new_steps.len()
    }

    /// Reuse ratio: how much of the original workflow was kept.
    /// 1.0 = exact replay, 0.0 = completely rewritten.
    pub fn reuse_ratio(&self) -> f64 {
        let original = self.total_original_steps();
        if original == 0 {
            return 0.0;
        }
        let kept = self.reused_steps.len() + self.adapted_steps.len();
        kept as f64 / original as f64
    }

    /// Adaptation ratio: how much was modified vs kept.
    pub fn adaptation_ratio(&self) -> f64 {
        let kept = self.reused_steps.len() + self.adapted_steps.len();
        if kept == 0 {
            return 0.0;
        }
        self.adapted_steps.len() as f64 / kept as f64
    }
}

impl Default for WorkflowExecutionReport {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Workflow Replay Engine
// ---------------------------------------------------------------------------

/// The Replay Engine: orchestrates applicability checking and report
/// generation. It does NOT execute anything — execution goes through
/// GoalSupervisor → AgentKernel.
pub struct ReplayEngine;

impl ReplayEngine {
    /// Compare a planned step sequence against a stored workflow and
    /// produce an execution report.
    ///
    /// This is called **after** planning, when the planner has produced
    /// a final plan that may have been influenced by a stored workflow.
    pub fn report(
        workflow_id: Option<Uuid>,
        pattern_id: Option<Uuid>,
        original_steps: &[String],
        final_steps: &[String],
        outcome: GoalOutcome,
    ) -> WorkflowExecutionReport {
        let mut reused = Vec::new();
        let mut adapted = Vec::new();
        let mut skipped = Vec::new();

        // Steps in original but not in final = skipped.
        let final_set: std::collections::HashSet<&String> = final_steps.iter().collect();

        for step in original_steps {
            if final_set.contains(step) {
                reused.push(step.clone());
            } else {
                skipped.push(step.clone());
            }
        }

        // Steps in final but not in original = new.
        let original_set: std::collections::HashSet<&String> = original_steps.iter().collect();
        let new_steps: Vec<String> = final_steps
            .iter()
            .filter(|s| !original_set.contains(s))
            .cloned()
            .collect();

        // For now, all reused steps are "reused exactly" (no adaptation tracking
        // at the step level — that would require step-level provenance).
        // Adapted steps = steps that appear in both but in different positions.
        for (i, step) in final_steps.iter().enumerate() {
            if original_set.contains(step) {
                // Check if position changed.
                if let Some(orig_pos) = original_steps.iter().position(|s| s == step) {
                    if orig_pos != i {
                        // This step was moved — count as adapted.
                        reused.retain(|s| s != step);
                        adapted.push(step.clone());
                    }
                }
            }
        }

        WorkflowExecutionReport {
            workflow_id,
            pattern_id,
            reused_steps: reused,
            adapted_steps: adapted,
            skipped_steps: skipped,
            new_steps,
            completed_at: Utc::now(),
            outcome,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn applicability_passes_when_capabilities_exist() {
        let checker = ApplicabilityChecker::new(
            vec!["mcp:filesystem.write_file".into(), "mcp:git.git_commit".into()],
            vec![],
        );
        let workflow = Workflow {
            id: Uuid::new_v4(),
            name: "test".into(),
            actions: vec!["write_file".into(), "git_commit".into()],
            capabilities: vec!["mcp:filesystem.write_file".into(), "mcp:git.git_commit".into()],
            created_at: Utc::now(),
            source_goal_id: Uuid::new_v4(),
        };
        let result = checker.check_workflow(&workflow);
        assert!(result.applicable);
        assert!(result.missing_capabilities.is_empty());
    }

    #[test]
    fn applicability_fails_when_capability_missing() {
        let checker = ApplicabilityChecker::new(
            vec!["mcp:filesystem.write_file".into()],
            vec![],
        );
        let workflow = Workflow {
            id: Uuid::new_v4(),
            name: "test".into(),
            actions: vec!["write_file".into(), "git_commit".into()],
            capabilities: vec!["mcp:filesystem.write_file".into(), "mcp:git.git_commit".into()],
            created_at: Utc::now(),
            source_goal_id: Uuid::new_v4(),
        };
        let result = checker.check_workflow(&workflow);
        assert!(!result.applicable);
        assert_eq!(result.missing_capabilities, vec!["mcp:git.git_commit"]);
    }

    #[test]
    fn execution_report_reuse_ratio() {
        let report = WorkflowExecutionReport {
            workflow_id: Some(Uuid::new_v4()),
            pattern_id: None,
            reused_steps: vec!["a".into(), "b".into()],
            adapted_steps: vec!["c".into()],
            skipped_steps: vec!["d".into()],
            new_steps: vec!["e".into()],
            completed_at: Utc::now(),
            outcome: GoalOutcome::Success,
        };
        // Original: a, b, c, d (4 steps). Kept: a, b, c (3). reuse = 3/4 = 0.75.
        assert!((report.reuse_ratio() - 0.75).abs() < 0.01);
        // Adapted: 1 out of 3 kept = 0.333.
        assert!((report.adaptation_ratio() - 1.0 / 3.0).abs() < 0.01);
    }

    #[test]
    fn execution_report_all_new() {
        let report = WorkflowExecutionReport {
            workflow_id: Some(Uuid::new_v4()),
            pattern_id: None,
            reused_steps: vec![],
            adapted_steps: vec![],
            skipped_steps: vec!["a".into(), "b".into()],
            new_steps: vec!["x".into(), "y".into(), "z".into()],
            completed_at: Utc::now(),
            outcome: GoalOutcome::Success,
        };
        assert!((report.reuse_ratio()).abs() < 0.01);
        assert_eq!(report.total_final_steps(), 3);
    }

    #[test]
    fn replay_engine_classifies_steps() {
        let original = vec!["a".into(), "b".into(), "c".into()];
        // Planner kept a, c (c moved position), added d, dropped b.
        let final_steps = vec!["a".into(), "c".into(), "d".into()];

        let report = ReplayEngine::report(
            Some(Uuid::new_v4()),
            None,
            &original,
            &final_steps,
            GoalOutcome::Success,
        );

        // "a" stayed at position 0 → reused. "c" moved from pos 2 to pos 1 → adapted.
        assert_eq!(report.reused_steps, vec!["a"]);
        assert_eq!(report.adapted_steps, vec!["c"]);
        assert!(report.skipped_steps.contains(&"b".to_string()));
        assert!(report.new_steps.contains(&"d".to_string()));
    }
}
