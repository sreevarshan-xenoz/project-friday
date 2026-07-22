//! Structured inputs every planner receives. The intent analysis stage is
//! already done (the `Intent` is passed in); this context carries everything a
//! planner needs to make grounded decisions: what capabilities actually exist,
//! the execution budget, the workspace, and the risk policy.

use crate::execution::capability_registry::Risk;
use crate::execution::lifecycle::ExecutionBudget;
use crate::experience::PlanningContext;

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
    pub experience: Option<PlanningContext>,
}

impl PlanContext {
    /// Render the available capabilities as a compact JSON array for embedding
    /// in the LLM prompt. Sorted by id for stable, diffable prompts.
    pub fn capabilities_json(&self) -> serde_json::Value {
        let mut caps: Vec<&AvailableCapability> = self.capabilities.iter().collect();
        caps.sort_by(|a, b| a.id.cmp(&b.id));
        serde_json::json!(caps
            .iter()
            .map(|c| serde_json::json!({
                "id": c.id,
                "description": c.description,
                "risk": c.risk.as_str(),
                "score": c.score,
            }))
            .collect::<Vec<_>>())
    }
}
