use crate::Action;
use crate::Capability;
use crate::Goal;
use crate::Policy;
use crate::PolicyKind;
use crate::WorldState;
use friday_knowledge::{KnowledgeEngine, KnowledgeRecord};
use serde::{Deserialize, Serialize};

// ── Plan ────────────────────────────────────────────────────────────

/// A plan — a structured representation of how to achieve a goal.
///
/// ```text
/// Plan {
///     id,
///     goal_id,
///     strategy,
///     actions,
///     estimated_cost,
///     estimated_duration_ms,
///     confidence,
///     prerequisites,
/// }
/// ```
///
/// A plan wraps actions with metadata that the evaluator can reason about.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Plan {
    pub id: String,
    pub goal_id: String,
    /// Human-readable strategy description.
    pub strategy: String,
    /// The ordered actions in this plan.
    pub actions: Vec<Action>,
    /// Estimated cost (arbitrary units, lower = cheaper).
    pub estimated_cost: f64,
    /// Estimated duration in milliseconds.
    pub estimated_duration_ms: u64,
    /// Confidence that this plan will succeed [0.0, 1.0].
    pub confidence: f32,
    /// Prerequisites that must be true for this plan to work.
    pub prerequisites: Vec<String>,
    /// The source of this plan (which generator produced it).
    pub source: PlanSource,
}

/// Where a plan came from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum PlanSource {
    /// Direct mapping from goal to actions (default).
    Direct,
    /// Generated from a sequencing policy.
    PolicyDriven,
    /// Generated from knowledge patterns.
    KnowledgeBased,
    /// Generated from simulation.
    Simulated,
    /// Custom source.
    Custom(String),
}

impl Plan {
    /// Create a plan from a single action (simplest case).
    pub fn from_action(action: Action, strategy: impl Into<String>) -> Self {
        let strategy = strategy.into();
        Self {
            id: format!("plan_{}", uuid::Uuid::new_v4()),
            goal_id: action.goal_id.clone(),
            strategy,
            actions: vec![action],
            estimated_cost: 1.0,
            estimated_duration_ms: 1000,
            confidence: 0.5,
            prerequisites: Vec::new(),
            source: PlanSource::Direct,
        }
    }

    /// Create a plan from multiple actions.
    pub fn from_actions(
        actions: Vec<Action>,
        strategy: impl Into<String>,
        source: PlanSource,
    ) -> Self {
        let goal_id = actions.first().map(|a| a.goal_id.clone()).unwrap_or_default();
        let strategy = strategy.into();
        let cost = actions.len() as f64;
        let duration = actions.len() as u64 * 1000;
        Self {
            id: format!("plan_{}", uuid::Uuid::new_v4()),
            goal_id,
            strategy,
            actions,
            estimated_cost: cost,
            estimated_duration_ms: duration,
            confidence: 0.5,
            prerequisites: Vec::new(),
            source,
        }
    }

    pub fn with_confidence(mut self, confidence: f32) -> Self {
        self.confidence = confidence;
        self
    }

    pub fn with_cost(mut self, cost: f64) -> Self {
        self.estimated_cost = cost;
        self
    }

    pub fn with_duration_ms(mut self, ms: u64) -> Self {
        self.estimated_duration_ms = ms;
        self
    }

    pub fn with_prerequisites(mut self, prereqs: Vec<String>) -> Self {
        self.prerequisites = prereqs;
        self
    }

    /// Number of actions in this plan.
    pub fn action_count(&self) -> usize {
        self.actions.len()
    }

    /// Whether this plan has prerequisites.
    pub fn has_prerequisites(&self) -> bool {
        !self.prerequisites.is_empty()
    }
}

// ── Planning Context ────────────────────────────────────────────────

/// Planning context — the complete inputs to a planning decision.
///
/// ```text
/// PlanningContext
///   ├── Goal
///   ├── Knowledge
///   ├── Policies
///   ├── WorldState
///   └── Constraints
/// ```
///
/// Planning is a deterministic transformation:
///
/// ```text
/// PlanningContext → Planning → Vec<PlanCandidate>
/// ```
#[derive(Debug, Clone)]
pub struct PlanningContext {
    pub goal: Goal,
    pub knowledge: Vec<KnowledgeRecord>,
    pub policies: Vec<Policy>,
    pub world_state: WorldState,
    pub constraints: Constraints,
}

/// Constraints on planning.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Constraints {
    pub max_actions: Option<u32>,
    pub timeout_ms: Option<u64>,
    pub required_permissions: Vec<String>,
    pub excluded_capabilities: Vec<Capability>,
}

impl PlanningContext {
    pub fn new(goal: Goal) -> Self {
        Self {
            goal,
            knowledge: Vec::new(),
            policies: Vec::new(),
            world_state: WorldState::new(),
            constraints: Constraints::default(),
        }
    }

    pub fn with_knowledge(mut self, knowledge: Vec<KnowledgeRecord>) -> Self {
        self.knowledge = knowledge;
        self
    }

    pub fn with_policies(mut self, policies: Vec<Policy>) -> Self {
        self.policies = policies;
        self
    }

    pub fn with_world_state(mut self, world_state: WorldState) -> Self {
        self.world_state = world_state;
        self
    }

    pub fn with_constraints(mut self, constraints: Constraints) -> Self {
        self.constraints = constraints;
        self
    }
}

// ── Plan Candidate ──────────────────────────────────────────────────

/// A plan candidate — a plan with preliminary evaluation.
///
/// Generated by Planning, consumed by PlanEvaluator.
#[derive(Debug, Clone)]
pub struct PlanCandidate {
    pub plan: Plan,
    /// Preliminary score from the planner [0.0, 1.0].
    pub preliminary_score: f32,
}

// ── Ranked Plan ─────────────────────────────────────────────────────

/// A ranked plan — the output of PlanEvaluator.
///
/// ```text
/// PlanEvaluator
///   input:  Vec<PlanCandidate>
///   output: RankedPlan
/// ```
#[derive(Debug, Clone)]
pub struct RankedPlan {
    /// The chosen plan.
    pub plan: Plan,
    /// Final evaluation score [0.0, 1.0].
    pub score: f32,
    /// All candidates that were considered, sorted by score descending.
    pub all_candidates: Vec<PlanCandidate>,
    /// Why this plan was chosen (human-readable).
    pub rationale: String,
}

// ── Plan Evaluator ──────────────────────────────────────────────────

/// Plan evaluator — scores and ranks candidate plans.
///
/// ```text
/// Vec<PlanCandidate> → PlanEvaluator → RankedPlan
/// ```
///
/// The evaluator considers:
/// - Success probability (confidence)
/// - Cost (lower is better)
/// - Time (shorter is better)
/// - Policy alignment
/// - Prerequisites met
/// - World state compatibility
///
/// The evaluator is deterministic: same candidates → same ranking.
pub struct PlanEvaluator;

impl PlanEvaluator {
    pub fn new() -> Self {
        Self
    }

    /// Evaluate and rank candidate plans. Returns the best plan.
    ///
    /// Panics if `candidates` is empty.
    pub fn evaluate(&self, candidates: &[PlanCandidate], world: &WorldState) -> RankedPlan {
        assert!(!candidates.is_empty(), "Cannot evaluate empty candidates");

        let mut scored: Vec<(PlanCandidate, f32)> = candidates
            .iter()
            .map(|c| {
                let score = self.score(c, world);
                (c.clone(), score)
            })
            .collect();

        scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

        let best = scored.remove(0);
        let rationale = self.explain(&best.0.plan, best.1, world);

        let all_candidates: Vec<PlanCandidate> = scored
            .into_iter()
            .map(|(c, _)| c)
            .collect();

        RankedPlan {
            plan: best.0.plan,
            score: best.1,
            all_candidates,
            rationale,
        }
    }

    /// Score a single candidate against the world state.
    fn score(&self, candidate: &PlanCandidate, world: &WorldState) -> f32 {
        let plan = &candidate.plan;

        // Base score from plan confidence
        let mut score = plan.confidence;

        // Cost penalty: more actions = higher cost
        let cost_factor = 1.0 / (1.0 + plan.actions.len() as f32 * 0.1);
        score *= cost_factor;

        // Prerequisite penalty: unmet prerequisites reduce score
        let prereqs_met = plan
            .prerequisites
            .iter()
            .all(|p| self.prerequisite_met(p, world));
        if !prereqs_met {
            score *= 0.3; // heavy penalty for unmet prerequisites
        }

        // World state bonus: if tools are available
        let tools_available = plan.actions.iter().all(|a| world.has_tool(&a.capability));
        if tools_available {
            score *= 1.1; // slight bonus
        } else {
            score *= 0.5; // penalty for missing tools
        }

        // Clamp to [0.0, 1.0]
        score.clamp(0.0, 1.0)
    }

    /// Check if a prerequisite is met in the world state.
    fn prerequisite_met(&self, prereq: &str, world: &WorldState) -> bool {
        // Simple prefix-based matching
        if let Some(path) = prereq.strip_prefix("file_exists:") {
            return world.file_exists(path);
        }
        if let Some(tool) = prereq.strip_prefix("has_tool:") {
            // Parse capability from string
            return world.available_tools.iter().any(|c| c.to_string() == tool);
        }
        // Default: assume met
        true
    }

    /// Explain why a plan was chosen.
    fn explain(&self, plan: &Plan, score: f32, world: &WorldState) -> String {
        let tools_ok = plan
            .actions
            .iter()
            .all(|a| world.has_tool(&a.capability));
        let prereqs_ok = plan
            .prerequisites
            .iter()
            .all(|p| self.prerequisite_met(p, world));

        format!(
            "Plan '{}' scored {:.2}. Actions: {}. Tools available: {}. Prerequisites met: {}. Source: {:?}.",
            plan.strategy, score, plan.action_count(), tools_ok, prereqs_ok, plan.source
        )
    }
}

impl Default for PlanEvaluator {
    fn default() -> Self {
        Self::new()
    }
}

// ── Planning ────────────────────────────────────────────────────────

/// Planning — generates candidate plans from a PlanningContext.
///
/// ```text
/// PlanningContext → Planning → Vec<PlanCandidate>
/// ```
///
/// Planning generates multiple hypotheses. PlanEvaluator chooses the best.
/// Planning is declarative: it produces candidates, never executes.
pub struct Planning {
    #[allow(dead_code)]
    knowledge_engine: KnowledgeEngine,
}

impl Planning {
    pub fn new() -> Self {
        Self {
            knowledge_engine: KnowledgeEngine::new(),
        }
    }

    /// Generate candidate plans from a planning context.
    ///
    /// This produces multiple candidates for the evaluator to rank.
    pub fn think(&self, context: &PlanningContext) -> Vec<PlanCandidate> {
        let mut candidates = Vec::new();

        // Candidate 1: Direct plan (single action)
        candidates.push(self.plan_direct(context));

        // Candidate 2: Policy-driven plan (if sequencing policy exists)
        if let Some(policy_candidate) = self.plan_from_policy(context) {
            candidates.push(policy_candidate);
        }

        // Candidate 3: Knowledge-aware plan (if knowledge suggests a pattern)
        if let Some(knowledge_candidate) = self.plan_from_knowledge(context) {
            candidates.push(knowledge_candidate);
        }

        candidates
    }

    /// Simple single-action plan.
    fn plan_direct(&self, context: &PlanningContext) -> PlanCandidate {
        let capability = self.infer_capability(&context.goal.description, &context.knowledge);
        let action = Action::single(&context.goal.description, capability, &context.goal.id);
        let plan = Plan::from_action(action, "Direct action from goal description");
        PlanCandidate {
            preliminary_score: 0.5,
            plan,
        }
    }

    /// Plan driven by a sequencing policy.
    fn plan_from_policy(&self, context: &PlanningContext) -> Option<PlanCandidate> {
        let seq_policy = context.policies.iter().find(|p| p.kind == PolicyKind::Sequence)?;

        let from_cap = seq_policy.capability.clone().unwrap_or(Capability::ExecuteCommand);
        let then_cap: Capability = serde_json::from_value(
            seq_policy.config.get("then").cloned().unwrap_or(serde_json::json!("execute_command")),
        )
        .unwrap_or(Capability::ExecuteCommand);

        let seq_label = format!("Sequence: {} then {}", from_cap, then_cap);

        let actions = vec![
            Action::step(&context.goal.description, from_cap, &context.goal.id, 0, 2),
            Action::step(&context.goal.description, then_cap, &context.goal.id, 1, 2),
        ];

        let mut plan = Plan::from_actions(
            actions,
            seq_label,
            PlanSource::PolicyDriven,
        );
        plan.confidence = seq_policy.confidence;

        Some(PlanCandidate {
            preliminary_score: seq_policy.confidence,
            plan,
        })
    }

    /// Plan derived from knowledge patterns.
    fn plan_from_knowledge(&self, context: &PlanningContext) -> Option<PlanCandidate> {
        if context.knowledge.is_empty() {
            return None;
        }

        // Find the most supported knowledge record
        let best = context
            .knowledge
            .iter()
            .max_by_key(|k| k.support)?;

        if best.support < 2 {
            return None; // need at least 2 observations
        }

        let capability = self.infer_capability(&best.label, &context.knowledge);
        let action = Action::single(&best.label, capability, &context.goal.id);
        let mut plan = Plan::from_action(action, format!("Based on knowledge: {}", best.label));
        plan.source = PlanSource::KnowledgeBased;
        plan.confidence = best.confidence;
        plan.estimated_cost = 0.5; // knowledge-derived plans are cheaper

        Some(PlanCandidate {
            preliminary_score: best.confidence,
            plan,
        })
    }

    /// Legacy entry point: single plan (backward compatible).
    pub fn plan(&self, context: &PlanningContext) -> Vec<Action> {
        let candidates = self.think(context);
        candidates
            .into_iter()
            .next()
            .map(|c| c.plan.actions)
            .unwrap_or_default()
    }

    fn infer_capability(&self, description: &str, _knowledge: &[KnowledgeRecord]) -> Capability {
        let desc_lower = description.to_lowercase();
        if desc_lower.contains("cargo")
            || desc_lower.contains("build")
            || desc_lower.contains("test")
            || desc_lower.contains("run")
        {
            Capability::ExecuteCommand
        } else if desc_lower.contains("read") || desc_lower.contains("open") {
            Capability::ReadFile
        } else if desc_lower.contains("write") || desc_lower.contains("create") {
            Capability::WriteFile
        } else if desc_lower.contains("list") || desc_lower.contains("ls") {
            Capability::ListDirectory
        } else if desc_lower.contains("delete") || desc_lower.contains("remove") {
            Capability::DeleteFile
        } else if desc_lower.contains("search") || desc_lower.contains("find") {
            Capability::SearchWeb
        } else {
            Capability::ExecuteCommand
        }
    }
}

impl Default for Planning {
    fn default() -> Self {
        Self::new()
    }
}
