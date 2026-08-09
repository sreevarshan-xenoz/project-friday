//! The planner router. It decides *which* planner produces a given goal's graph
//! so the system doesn't pay an LLM round-trip for a deterministic task it could
//! plan in O(1). The decision is a pure function of the intent + whether an LLM
//! backend is available.
//!
//! ```text
//! trivial / known intent type, no backend  → RulePlanner
//! unknown / complex intent, backend present → LlmPlanner
//! "hybrid" request, backend present         → LlmPlanner (rule seed optional)
//! ```
//!
//! The supervisor is oblivious to this choice.

use std::sync::Arc;

use crate::intent::classifier::IntentType;
use crate::intent::Intent;
use crate::planning::planners::planner_trait::Planner;

/// Which planner family produced (or should produce) a plan.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlannerKind {
    Rule,
    Llm,
    Hybrid,
}

/// Selects a planner per goal. Holds the rule planner always and an optional LLM
/// planner; `select` returns the right one.
pub struct PlannerRouter {
    rule: Arc<dyn Planner>,
    llm: Option<Arc<dyn Planner>>,
}

impl PlannerRouter {
    pub fn new(rule: Arc<dyn Planner>, llm: Option<Arc<dyn Planner>>) -> Self {
        Self { rule, llm }
    }

    /// Build a router from a rule planner only (LLM disabled).
    pub fn rule_only(rule: Arc<dyn Planner>) -> Self {
        Self::new(rule, None)
    }

    /// Whether an LLM planner is wired in.
    pub fn has_llm(&self) -> bool {
        self.llm.is_some()
    }

    /// Pick the planner for this intent.
    pub fn select(&self, intent: &Intent) -> Arc<dyn Planner> {
        if self.should_use_llm(intent) {
            self.llm.clone().expect("llm present when should_use_llm")
        } else {
            self.rule.clone()
        }
    }

    /// The chosen kind for telemetry.
    pub fn kind_for(&self, intent: &Intent) -> PlannerKind {
        if self.should_use_llm(intent) {
            PlannerKind::Llm
        } else {
            PlannerKind::Rule
        }
    }

    fn should_use_llm(&self, intent: &Intent) -> bool {
        let llm = match &self.llm {
            Some(l) => l,
            None => return false,
        };
        let _ = llm; // presence already checked
                     // Deterministic, well-known intent types are cheap to plan by rule.
        let trivial = matches!(
            IntentType::classify(&intent.raw),
            IntentType::BrowserSearch | IntentType::OpenApp | IntentType::RoutineMorning
        );
        if trivial {
            return false;
        }
        // Anything else (Unknown, or "refactor/deploy/build/implement/complex")
        // goes to the model.
        let raw = intent.raw.to_lowercase();
        let complex_keywords = [
            "refactor",
            "deploy",
            "build",
            "implement",
            "complex",
            "research",
            "analyze",
            "migrate",
        ];
        !trivial
            && (matches!(IntentType::classify(&intent.raw), IntentType::Unknown)
                || complex_keywords.iter().any(|k| raw.contains(k)))
    }
}
