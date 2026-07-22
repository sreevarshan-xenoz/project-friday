//! Planning Context (M13).
//!
//! `PlanningContext` is the single value object the planner receives.
//! Everything the planner needs to make a decision lives here.
//! Nothing the planner doesn't need lives here.
//!
//! The context is assembled by `PlanningContextBuilder` from an
//! `ExperienceProvider` and a planning request. It is immutable
//! after construction — a snapshot of evidence for one planning session.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::provider::ExperienceProvider;
use super::replay::{ApplicabilityChecker, ApplicabilityResult};
use super::{
    ExperienceMatch, GoalOutcome, Pattern, ReflectionRecord, WorkflowSuggestion,
};

// ---------------------------------------------------------------------------
// Core types
// ---------------------------------------------------------------------------

/// Immutable snapshot of everything the planner needs.
/// No setters. No mutation after construction.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanningContext {
    /// The goal the planner must produce a plan for.
    pub goal: PlanningGoal,
    /// Available capabilities. Caller provides.
    pub capabilities: Vec<String>,
    /// Policy, risk, budget constraints from the user or system.
    pub constraints: Vec<String>,
    /// All experience-derived evidence, grouped together.
    pub evidence: ExperienceEvidence,
}

/// The goal being planned for. Simplified from the full Goal type
/// to avoid pulling in planner internals.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanningGoal {
    pub id: Uuid,
    pub title: String,
    pub description: String,
}

/// Experience-derived evidence. Separated from goal/constraints to make
/// obvious which parts originate from the current request vs. history.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExperienceEvidence {
    /// Similar past goals with outcomes, trust, and reflection.
    pub similar_experiences: Vec<ExperienceMatch>,
    /// What happened the last time this exact goal was attempted.
    pub previous_outcome: Option<ExperienceMatch>,
    /// Workflows that succeeded for similar goals.
    pub successful_workflows: Vec<SuccessfulWorkflow>,
    /// Suggested workflows from pattern mining.
    pub workflow_suggestions: Vec<WorkflowSuggestion>,
    /// Workflows eligible for replay, with applicability checks.
    pub replay_candidates: Vec<ReplayCandidate>,
    /// Reflections from similar past goals.
    pub reflections: Vec<ReflectionRecord>,
    /// Reserved for future diagnostics and explainability.
    /// Empty in initial implementation.
    pub provenance: Vec<EvidenceProvenance>,
}

/// A successful workflow simplified for the planner (no internal GoalKey).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SuccessfulWorkflow {
    pub title: String,
    pub capabilities: Vec<String>,
    pub outcome: GoalOutcome,
}

/// A replay candidate with its applicability result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplayCandidate {
    pub pattern: Pattern,
    pub applicability: ApplicabilityResult,
    /// Pre-computed reuse ratio for ranking (0.0–1.0).
    pub reuse_ratio: f64,
}

/// Provenance metadata for a single evidence item. Reserved for future use.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvidenceProvenance {
    /// Which subsystem produced this evidence.
    pub source: EvidenceSource,
    /// Optional human-readable reason for inclusion.
    pub reason: Option<String>,
}

/// Source of evidence in the experience layer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum EvidenceSource {
    Retrieval,
    Replay,
    PatternMining,
    Reflection,
}

// ---------------------------------------------------------------------------
// Builder (stateless)
// ---------------------------------------------------------------------------

/// Stateless builder. Assembles a PlanningContext from a provider and request.
pub struct PlanningContextBuilder;

impl PlanningContextBuilder {
    /// Build a PlanningContext from an experience provider and planning request.
    ///
    /// The builder queries the provider, normalizes results, filters duplicates,
    /// and packages evidence. It does NOT rank, choose, or discard evidence —
    /// those are planner decisions.
    pub fn build(
        provider: &dyn ExperienceProvider,
        goal: PlanningGoal,
        capabilities: Vec<String>,
        constraints: Vec<String>,
        registered_capabilities: &[String],
    ) -> PlanningContext {
        let similar = provider.similar(&goal.title, 5);
        let previous = provider.last_attempt(goal.id);
        let workflows = provider.successful_workflows(&goal.title);
        let suggestions = provider.suggestions();

        // Build replay candidates from suggestions
        let checker = ApplicabilityChecker::new(
            registered_capabilities.to_vec(),
            vec![], // no existing artifacts in initial impl
        );
        let replay_candidates = suggestions
            .iter()
            .map(|s| {
                // Find the pattern that matches this suggestion's capabilities
                // In a full impl, we'd look up by pattern_id. For now, build
                // a minimal pattern from the suggestion.
                let pattern = Pattern {
                    id: s.pattern_id,
                    capabilities: s.capabilities.clone(),
                    frequency: s.frequency,
                    success_rate: s.success_rate,
                    last_seen: s.last_seen,
                    average_duration_secs: None,
                    source_goal_ids: vec![],
                };
                let applicability = checker.check_pattern(&pattern);
                let reuse_ratio = if applicability.applicable {
                    1.0 // all steps reusable if applicable
                } else {
                    let blocked = applicability.missing_capabilities.len() as f64;
                    let total = pattern.capabilities.len() as f64;
                    if total > 0.0 {
                        (total - blocked) / total
                    } else {
                        0.0
                    }
                };
                ReplayCandidate {
                    pattern,
                    applicability,
                    reuse_ratio,
                }
            })
            .collect();

        // Collect reflections from similar experiences
        let reflections: Vec<ReflectionRecord> = similar
            .iter()
            .flat_map(|m| m.reflection.clone())
            .collect();

        // Simplify successful workflows for the planner
        let successful_workflows: Vec<SuccessfulWorkflow> = workflows
            .into_iter()
            .map(|r| SuccessfulWorkflow {
                title: r.title,
                capabilities: r
                    .workflows
                    .first()
                    .map(|w| w.capabilities.clone())
                    .unwrap_or_default(),
                outcome: r.outcome,
            })
            .collect();

        PlanningContext {
            goal,
            capabilities,
            constraints,
            evidence: ExperienceEvidence {
                similar_experiences: similar,
                previous_outcome: previous,
                successful_workflows,
                workflow_suggestions: suggestions,
                replay_candidates,
                reflections,
                provenance: vec![], // reserved for future use
            },
        }
    }
}
