//! Structured inputs every planner receives. The intent analysis stage is
//! already done (the `Intent` is passed in); this context carries everything a
//! planner needs to make grounded decisions: what capabilities actually exist,
//! the execution budget, the workspace, and the risk policy.

use crate::execution::capability_registry::Risk;
use crate::execution::lifecycle::ExecutionBudget;
use crate::experience::{ExperienceContext, GoalOutcome};
use friday_investigation::InvestigationResult;
use friday_skills::SkillContext;

/// A capability the planner is allowed to use, stripped to what the model needs:
/// its canonical id, a human description, and its risk class.
#[derive(Debug, Clone)]
pub struct AvailableCapability {
    pub id: String,
    pub description: String,
    pub risk: Risk,
    /// Discovery/ranking score (0..1) from `CapabilityRegistry::search`. Higher
    /// means more relevant to the current goal.
    pub score: f64,
    /// JSON Schema describing the tool's expected input parameters.
    pub schema: serde_json::Value,
    /// True when this capability is declared by the skill the planner chose
    /// for the goal (skill-first scoping): these lead the offered list so the
    /// model plans *through* the chosen skill.
    pub preferred: bool,
}

/// Lightweight view of the workspace the plan will run in. Populated by the host
/// (cwd, git branch, detected project). Kept small and serializable-friendly so
/// it can be embedded in the LLM prompt.
#[derive(Debug, Clone, Default)]
pub struct WorkspaceContext {
    pub cwd: Option<String>,
    pub git_branch: Option<String>,
    pub project_kind: Option<String>,
}

/// The risk policy the planner must respect when generating tasks.
#[derive(Debug, Clone)]
pub struct RiskPolicy {
    /// If false, the planner must NOT generate tasks whose capability is
    /// `Risk::High` (they will be flagged by validation instead).
    pub allow_high_risk: bool,
    /// If true, high-risk tasks should still be emitted but marked for approval
    /// (the supervisor pauses on them anyway).
    pub require_approval_for_high: bool,
}

impl Default for RiskPolicy {
    fn default() -> Self {
        Self {
            allow_high_risk: true,
            require_approval_for_high: true,
        }
    }
}

/// Everything a planner needs to produce a grounded `TaskGraph`.
#[derive(Debug, Clone)]
pub struct PlanContext {
    pub intent_raw: String,
    pub capabilities: Vec<AvailableCapability>,
    pub budget: ExecutionBudget,
    pub workspace: WorkspaceContext,
    pub risk_policy: RiskPolicy,
    /// Experience evidence from the Experience Layer. `None` when experience
    /// is unavailable or disabled. Planners may ignore this field entirely —
    /// it carries optional historical evidence, not requirements.
    pub experience: Option<ExperienceContext>,
    /// Skill-aware context from the Skill Engine: skills ranked against the
    /// goal plus the deterministic best usable skill. `None` when no skill
    /// source is wired. Advisory like experience — the planner plans through
    /// skills when present but must keep working without them.
    pub skills: Option<SkillContext>,
    /// Investigation outcome from the Investigation Engine: the channels this
    /// goal requires evidence from before execution, with rationale and
    /// confidence. `None` when no investigation seam is wired. Advisory like
    /// experience and skills — the planner folds weak evidence into the
    /// direct plan and keeps working without a seam.
    pub investigation: Option<InvestigationResult>,
    /// The world-model revision this plan is grounded against, captured by
    /// the host before planning. `None` when no world-model seam is wired.
    /// Advisory to the planner; its authoritative consumer is execution
    /// admission, which refuses a plan whose grounding no longer matches
    /// the live world revision (R2-1 stale-plan rejection).
    pub grounding_revision: Option<u64>,
}

impl PlanContext {
    /// Render the available capabilities as a compact JSON array for embedding
    /// in the LLM prompt. Sorted by id for stable, diffable prompts.
    pub fn capabilities_json(&self) -> serde_json::Value {
        let mut caps: Vec<&AvailableCapability> = self.capabilities.iter().collect();
        caps.sort_by(|a, b| a.id.cmp(&b.id));
        serde_json::json!(caps
            .iter()
            .map(|c| {
                let mut obj = serde_json::json!({
                    "id": c.id,
                    "description": c.description,
                    "risk": c.risk.as_str(),
                    "score": c.score,
                });
                if c.preferred {
                    obj["preferred"] = serde_json::json!(true);
                }
                if !c.schema.is_null() && !c.schema.as_object().is_none_or(|o| o.is_empty()) {
                    obj["params"] = c.schema.clone();
                }
                obj
            })
            .collect::<Vec<_>>())
    }

    /// Get a read-only view of experience evidence. Returns a view that
    /// gracefully degrades when experience is `None` or evidence is empty.
    pub fn evidence(&self) -> PlanningEvidenceView<'_> {
        PlanningEvidenceView::new(self.experience.as_ref())
    }
}

/// Read-only view into experience evidence. Provides convenience methods
/// so the planner doesn't need to know the internal structure of
/// `ExperienceContext` or `ExperienceEvidence`.
///
/// This view is advisory — it surfaces facts, never prescribes actions.
/// The planner decides what to do with the information.
#[derive(Debug, Clone)]
pub struct PlanningEvidenceView<'a> {
    inner: Option<&'a ExperienceContext>,
}

impl<'a> PlanningEvidenceView<'a> {
    fn new(ctx: Option<&'a ExperienceContext>) -> Self {
        Self { inner: ctx }
    }

    /// Whether any experience evidence is available.
    pub fn is_available(&self) -> bool {
        self.inner.is_some()
    }

    /// Whether there's a previous attempt at this exact goal.
    pub fn has_previous_attempt(&self) -> bool {
        self.inner
            .and_then(|ctx| ctx.evidence.previous_outcome.as_ref())
            .is_some()
    }

    /// Whether the previous attempt succeeded.
    pub fn had_previous_success(&self) -> bool {
        self.inner
            .and_then(|ctx| ctx.evidence.previous_outcome.as_ref())
            .map(|m| m.outcome == GoalOutcome::Success)
            .unwrap_or(false)
    }

    /// Whether the previous attempt failed.
    pub fn had_previous_failure(&self) -> bool {
        self.inner
            .and_then(|ctx| ctx.evidence.previous_outcome.as_ref())
            .map(|m| {
                matches!(
                    m.outcome,
                    GoalOutcome::Failure | GoalOutcome::PartiallyCompleted
                )
            })
            .unwrap_or(false)
    }

    /// Human-readable summary of the previous outcome, if any.
    pub fn previous_outcome_summary(&self) -> Option<String> {
        let m = self.inner?.evidence.previous_outcome.as_ref()?;
        let outcome_str = match m.outcome {
            GoalOutcome::Success => "succeeded",
            GoalOutcome::Failure => "failed",
            GoalOutcome::PartiallyCompleted => "partially completed",
            GoalOutcome::Cancelled => "was cancelled",
            GoalOutcome::Unknown => "had unknown outcome",
        };
        Some(format!(
            "Previous attempt at '{}' {}.",
            m.goal_title, outcome_str
        ))
    }

    /// The goal title from the experience context, if available.
    pub fn goal_title(&self) -> Option<&str> {
        self.inner.map(|ctx| ctx.goal.title.as_str())
    }

    /// Number of similar experiences found.
    pub fn similar_count(&self) -> usize {
        self.inner
            .map(|ctx| ctx.evidence.similar_experiences.len())
            .unwrap_or(0)
    }

    /// Number of workflow suggestions available.
    pub fn suggestion_count(&self) -> usize {
        self.inner
            .map(|ctx| ctx.evidence.workflow_suggestions.len())
            .unwrap_or(0)
    }

    /// Number of replay candidates available.
    pub fn replay_candidate_count(&self) -> usize {
        self.inner
            .map(|ctx| ctx.evidence.replay_candidates.len())
            .unwrap_or(0)
    }

    /// Number of reflections available.
    pub fn reflection_count(&self) -> usize {
        self.inner
            .map(|ctx| ctx.evidence.reflections.len())
            .unwrap_or(0)
    }
}
