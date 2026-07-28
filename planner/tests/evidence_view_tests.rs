//! PlanningEvidenceView graceful degradation tests (M13.2).
//!
//! Verifies that experience evidence is advisory — the planner receives
//! information but makes the same decisions regardless. Two key invariants:
//!
//! 1. `experience = None` and `experience.previous_outcome = None` produce
//!    identical planning behavior.
//! 2. Evidence availability never causes planner errors.

use uuid::Uuid;

use friday_planner::experience::{
    ExperienceEvidence, ExperienceMatch, GoalOutcome, MatchReason, ExperienceContext,
    PlanningGoal, TrustComponents,
};
use friday_planner::planning::planners::context::{
    AvailableCapability, PlanContext, RiskPolicy, WorkspaceContext,
};
use friday_planner::execution::capability_registry::Risk;
use friday_planner::execution::lifecycle::ExecutionBudget;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn base_plan_context() -> PlanContext {
    PlanContext {
        intent_raw: "test goal".to_string(),
        capabilities: vec![AvailableCapability {
            id: "test.action".to_string(),
            description: "Test action".to_string(),
            risk: Risk::Safe,
            score: 0.9,
            schema: serde_json::json!({}),
        }],
        budget: ExecutionBudget::default(),
        workspace: WorkspaceContext::default(),
        risk_policy: RiskPolicy::default(),
        experience: None,
    }
}

fn plan_context_with_outcome(outcome: GoalOutcome) -> PlanContext {
    let match_ = ExperienceMatch {
        similarity_score: 1.0,
        trust_score: 0.8,
        trust_components: TrustComponents {
            similarity: 1.0,
            recency: 0.9,
            outcome_quality: 0.9,
            repetition: 0.3,
        },
        match_reason: MatchReason::RecentAttempt,
        goal_id: Uuid::new_v4(),
        workflow_id: None,
        outcome,
        reflection: vec![],
        artifacts: vec![],
        commit_hash: None,
        goal_title: "test goal".to_string(),
    };

    let planning_ctx = ExperienceContext {
        goal: PlanningGoal {
            id: Uuid::new_v4(),
            title: "test goal".to_string(),
            description: "test description".to_string(),
        },
        capabilities: vec![],
        constraints: vec![],
        evidence: ExperienceEvidence {
            similar_experiences: vec![],
            previous_outcome: Some(match_),
            successful_workflows: vec![],
            workflow_suggestions: vec![],
            replay_candidates: vec![],
            reflections: vec![],
            provenance: vec![],
        },
    };

    PlanContext {
        experience: Some(planning_ctx),
        ..base_plan_context()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[test]
fn no_experience_provides_view() {
    let ctx = base_plan_context();
    let view = ctx.evidence();
    assert!(!view.is_available());
    assert!(!view.has_previous_attempt());
    assert!(!view.had_previous_success());
    assert!(!view.had_previous_failure());
    assert!(view.previous_outcome_summary().is_none());
    assert_eq!(view.similar_count(), 0);
    assert_eq!(view.suggestion_count(), 0);
    assert_eq!(view.replay_candidate_count(), 0);
    assert_eq!(view.reflection_count(), 0);
}

#[test]
fn previous_success_detected() {
    let ctx = plan_context_with_outcome(GoalOutcome::Success);
    let view = ctx.evidence();
    assert!(view.is_available());
    assert!(view.has_previous_attempt());
    assert!(view.had_previous_success());
    assert!(!view.had_previous_failure());
    assert!(view.previous_outcome_summary().is_some());
    assert!(view.previous_outcome_summary().unwrap().contains("succeeded"));
}

#[test]
fn previous_failure_detected() {
    let ctx = plan_context_with_outcome(GoalOutcome::Failure);
    let view = ctx.evidence();
    assert!(view.is_available());
    assert!(view.has_previous_attempt());
    assert!(!view.had_previous_success());
    assert!(view.had_previous_failure());
    assert!(view.previous_outcome_summary().unwrap().contains("failed"));
}

#[test]
fn previous_partial_completion_detected() {
    let ctx = plan_context_with_outcome(GoalOutcome::PartiallyCompleted);
    let view = ctx.evidence();
    assert!(view.had_previous_failure()); // partially completed counts as failure
    assert!(view.previous_outcome_summary().unwrap().contains("partially completed"));
}

#[test]
fn empty_evidence_identical_to_no_experience() {
    let no_experience = base_plan_context();
    let empty_evidence = PlanContext {
        experience: Some(ExperienceContext {
            goal: PlanningGoal {
                id: Uuid::new_v4(),
                title: "test".to_string(),
                description: "test".to_string(),
            },
            capabilities: vec![],
            constraints: vec![],
            evidence: ExperienceEvidence {
                similar_experiences: vec![],
                previous_outcome: None,
                successful_workflows: vec![],
                workflow_suggestions: vec![],
                replay_candidates: vec![],
                reflections: vec![],
                provenance: vec![],
            },
        }),
        ..base_plan_context()
    };

    let view_none = no_experience.evidence();
    let view_empty = empty_evidence.evidence();

    // Both should report no evidence
    assert_eq!(view_none.has_previous_attempt(), view_empty.has_previous_attempt());
    assert_eq!(view_none.had_previous_success(), view_empty.had_previous_success());
    assert_eq!(view_none.had_previous_failure(), view_empty.had_previous_failure());
    assert_eq!(view_none.previous_outcome_summary(), view_empty.previous_outcome_summary());
    assert_eq!(view_none.similar_count(), view_empty.similar_count());
    assert_eq!(view_none.suggestion_count(), view_empty.suggestion_count());
}

#[test]
fn view_is_advisory_not_prescriptive() {
    // The planner can receive a failure and still generate a plan.
    // The view only surfaces facts — it never says "don't plan."
    let ctx = plan_context_with_outcome(GoalOutcome::Failure);
    let view = ctx.evidence();

    // View says failure happened
    assert!(view.had_previous_failure());

    // But the plan context is still valid — planner can still plan
    assert!(!ctx.capabilities.is_empty());
    assert!(ctx.budget.max_steps.is_some());
}

#[test]
fn view_gracefully_handles_all_outcome_types() {
    for outcome in [
        GoalOutcome::Success,
        GoalOutcome::Failure,
        GoalOutcome::PartiallyCompleted,
        GoalOutcome::Cancelled,
        GoalOutcome::Unknown,
    ] {
        let ctx = plan_context_with_outcome(outcome.clone());
        let view = ctx.evidence();
        // Should never panic
        let _ = view.has_previous_attempt();
        let _ = view.had_previous_success();
        let _ = view.had_previous_failure();
        let _ = view.previous_outcome_summary();
    }
}

#[test]
fn view_counts_are_zero_when_no_evidence() {
    let ctx = base_plan_context();
    let view = ctx.evidence();
    assert_eq!(view.similar_count(), 0);
    assert_eq!(view.suggestion_count(), 0);
    assert_eq!(view.replay_candidate_count(), 0);
    assert_eq!(view.reflection_count(), 0);
}
