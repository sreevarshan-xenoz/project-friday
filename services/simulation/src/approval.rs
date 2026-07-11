//! Plan-level approval. The user approves **plans, not actions**.

use friday_capabilities::sim::WorldState;
use serde::{Deserialize, Serialize};

/// Coarse risk band derived from a plan's failure probability.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RiskLevel {
    Low,
    Medium,
    High,
}

impl RiskLevel {
    pub fn from_risk(r: f64) -> Self {
        if r < 0.34 {
            RiskLevel::Low
        } else if r < 0.67 {
            RiskLevel::Medium
        } else {
            RiskLevel::High
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            RiskLevel::Low => "Low",
            RiskLevel::Medium => "Medium",
            RiskLevel::High => "High",
        }
    }
}

/// Human-facing summary rendered for approval.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanSummary {
    pub actions: usize,
    pub windows: usize,
    pub browsers: usize,
    pub files_affected: usize,
    pub estimated_seconds: u64,
    pub risk: RiskLevel,
}

/// Decision returned by the approval UI.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ApprovalDecision {
    Approved,
    Rejected,
    ApprovedWith(Vec<String>),
}

/// A request for plan-level approval.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApprovalRequest {
    pub summary: PlanSummary,
    pub world_before: WorldState,
    pub world_after: WorldState,
}

/// Render the user-facing plan summary.
pub fn render_summary(s: &PlanSummary) -> String {
    format!(
        "Plan Summary\n──────────────\n{} actions\n{} windows\n{} browser\n{} files\nEstimated: {} seconds\nRisk: {}",
        s.actions,
        s.windows,
        s.browsers,
        s.files_affected,
        s.estimated_seconds,
        s.risk.as_str()
    )
}
