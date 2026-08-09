//! The strategy layer's data: what a goal requires, and the traceable view
//! of how FRIDAY will think about it.

use serde::{Deserialize, Serialize};

use crate::channel::InvestigationChannel;

/// One information requirement: a channel, the evidence that triggered it,
/// and its strength.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InformationRequirement {
    pub channel: InvestigationChannel,
    /// Which keyword(s) triggered the requirement — the auditable "why".
    pub rationale: String,
    /// Evidence strength for this channel (`0.0..=1.0`).
    pub confidence: f32,
}

/// What a goal needs to know before it can be executed well.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct InvestigationPlan {
    /// Requirements, highest confidence first.
    pub requirements: Vec<InformationRequirement>,
}

impl InvestigationPlan {
    /// `true` when at least one channel must be consulted before planning.
    pub fn needs_investigation(&self) -> bool {
        !self.requirements.is_empty()
    }

    /// The channels required, in requirement order.
    pub fn channels(&self) -> Vec<InvestigationChannel> {
        self.requirements.iter().map(|r| r.channel).collect()
    }

    /// Human-readable summary — the "watch FRIDAY think" narration consumed
    /// by the executive's thought log, e.g. `web (0.90), git (0.80)`.
    pub fn describe(&self) -> String {
        if self.requirements.is_empty() {
            return "none".to_string();
        }
        self.requirements
            .iter()
            .map(|r| format!("{} ({:.2})", r.channel.label(), r.confidence))
            .collect::<Vec<_>>()
            .join(", ")
    }
}

/// The always-present outcome of running investigation for a goal.
///
/// Downstream consumers (the planner's `PlanningContext`) hold this instead
/// of `Option<InvestigationPlan>`: every goal is either investigated,
/// skipped, or failed — never "unknown". The pipeline stays explicit.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum InvestigationResult {
    /// Investigation ran; `plan` may be empty when the goal is self-contained.
    Complete(InvestigationPlan),
    /// Investigation was deliberately not run for this goal (e.g. a planner
    /// context built without an investigation seam, or a policy that skips it).
    Skipped,
    /// Investigation could not produce a plan. The planner folds this into
    /// the direct plan (a failed investigation is never a failed goal); the
    /// executive narrates the reason. Today no strategy fails — the heuristic
    /// always produces a plan and the LLM strategy falls back to it — so this
    /// is the forward seam for a strategy that can genuinely fail.
    Failed { reason: String },
}

impl InvestigationResult {
    /// The plan, when investigation completed.
    pub fn plan(&self) -> Option<&InvestigationPlan> {
        match self {
            Self::Complete(plan) => Some(plan),
            _ => None,
        }
    }

    /// `true` when investigation completed and at least one channel must be
    /// consulted before planning.
    pub fn needs_investigation(&self) -> bool {
        matches!(self, Self::Complete(plan) if plan.needs_investigation())
    }

    /// Human-readable narration — consumed by the executive's thought log
    /// (`web (0.90), git (0.80)` / `none` / `skipped` / `failed: …`).
    pub fn describe(&self) -> String {
        match self {
            Self::Complete(plan) => plan.describe(),
            Self::Skipped => "skipped".to_string(),
            Self::Failed { reason } => format!("failed: {reason}"),
        }
    }
}

impl From<InvestigationPlan> for InvestigationResult {
    fn from(plan: InvestigationPlan) -> Self {
        Self::Complete(plan)
    }
}

/// The "watch FRIDAY think" view: the goal, its investigation branches, and
/// the phases that follow.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InvestigationGraph {
    pub goal: String,
    /// Information branches, in the order they will be consulted.
    pub requirements: Vec<InformationRequirement>,
    /// The phase chain. A self-contained goal is `plan → execute`; a goal
    /// that needs information first becomes `investigate → plan → execute`.
    pub phases: Vec<String>,
}

impl InvestigationGraph {
    pub fn new(goal: &str, plan: &InvestigationPlan) -> Self {
        let mut phases = vec!["plan".to_string(), "execute".to_string()];
        if plan.needs_investigation() {
            phases.insert(0, "investigate".to_string());
        }
        Self {
            goal: goal.to_string(),
            requirements: plan.requirements.clone(),
            phases,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_plan_never_needs_investigation() {
        let p = InvestigationPlan::default();
        assert!(!p.needs_investigation());
        assert!(p.channels().is_empty());
    }

    #[test]
    fn graph_inserts_investigate_phase_only_when_needed() {
        let goal = "open notepad";
        let plain = InvestigationGraph::new(goal, &InvestigationPlan::default());
        assert_eq!(plain.phases, vec!["plan", "execute"]);

        let req = InformationRequirement {
            channel: InvestigationChannel::Web,
            rationale: "goal mentions 'search the web'".into(),
            confidence: 0.9,
        };
        let plan = InvestigationPlan {
            requirements: vec![req],
        };
        let with_investigation = InvestigationGraph::new(goal, &plan);
        assert_eq!(
            with_investigation.phases,
            vec!["investigate", "plan", "execute"]
        );
        assert_eq!(with_investigation.requirements.len(), 1);
    }

    #[test]
    fn plan_round_trips_json() {
        let p = InvestigationPlan {
            requirements: vec![InformationRequirement {
                channel: InvestigationChannel::Git,
                rationale: "git".into(),
                confidence: 0.8,
            }],
        };
        let json = serde_json::to_string(&p).unwrap();
        let back: InvestigationPlan = serde_json::from_str(&json).unwrap();
        assert_eq!(back, p);
        assert_eq!(back.channels(), vec![InvestigationChannel::Git]);
    }

    #[test]
    fn describe_renders_channels_with_strength() {
        let p = InvestigationPlan {
            requirements: vec![
                InformationRequirement {
                    channel: InvestigationChannel::Web,
                    rationale: "search the web".into(),
                    confidence: 0.9,
                },
                InformationRequirement {
                    channel: InvestigationChannel::Git,
                    rationale: "git".into(),
                    confidence: 0.8,
                },
            ],
        };
        let desc = p.describe();
        assert!(desc.contains("web (0.90)"), "{desc}");
        assert!(desc.contains("git (0.80)"), "{desc}");
        assert!(desc.contains(", "), "{desc}");
    }

    #[test]
    fn describe_empty_plan_is_none() {
        assert_eq!(InvestigationPlan::default().describe(), "none");
    }

    #[test]
    fn result_complete_exposes_the_plan() {
        let plan = InvestigationPlan {
            requirements: vec![InformationRequirement {
                channel: InvestigationChannel::Web,
                rationale: "search the web".into(),
                confidence: 0.9,
            }],
        };
        let result = InvestigationResult::Complete(plan.clone());
        assert_eq!(result.plan(), Some(&plan));
        assert!(result.needs_investigation());
        assert!(
            result.describe().contains("web (0.90)"),
            "{}",
            result.describe()
        );
    }

    #[test]
    fn result_skipped_and_failed_have_no_plan() {
        assert_eq!(InvestigationResult::Skipped.plan(), None);
        assert!(!InvestigationResult::Skipped.needs_investigation());
        assert_eq!(InvestigationResult::Skipped.describe(), "skipped");

        let failed = InvestigationResult::Failed {
            reason: "strategy unavailable".into(),
        };
        assert_eq!(failed.plan(), None);
        assert!(!failed.needs_investigation());
        assert_eq!(failed.describe(), "failed: strategy unavailable");
    }

    #[test]
    fn result_complete_empty_is_self_contained() {
        let result = InvestigationResult::Complete(InvestigationPlan::default());
        assert!(!result.needs_investigation());
        assert_eq!(result.describe(), "none");
    }

    #[test]
    fn plan_converts_into_complete() {
        let result: InvestigationResult = InvestigationPlan::default().into();
        assert_eq!(
            result,
            InvestigationResult::Complete(InvestigationPlan::default())
        );
    }

    #[test]
    fn result_round_trips_json() {
        let plan = InvestigationPlan {
            requirements: vec![InformationRequirement {
                channel: InvestigationChannel::Git,
                rationale: "repo".into(),
                confidence: 0.8,
            }],
        };
        for result in [
            InvestigationResult::Complete(plan),
            InvestigationResult::Skipped,
            InvestigationResult::Failed {
                reason: "down".into(),
            },
        ] {
            let json = serde_json::to_string(&result).unwrap();
            let back: InvestigationResult = serde_json::from_str(&json).unwrap();
            assert_eq!(back, result);
        }
    }
}
