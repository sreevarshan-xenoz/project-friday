use crate::Action;
use crate::Capability;
use crate::Goal;
use crate::Policy;
use crate::PolicyKind;
use crate::WorldState;
use friday_investigation::{InvestigationChannel, InvestigationResult};
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
    /// Generated from an investigation plan: evidence gathering precedes
    /// execution (M7 — the planner consumes the Investigation Engine).
    Investigation,
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
        let goal_id = actions
            .first()
            .map(|a| a.goal_id.clone())
            .unwrap_or_default();
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
///   ├── Constraints
///   └── Investigation (M7: what the goal requires knowing first)
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
    /// What this goal requires knowing before execution, produced by the
    /// Investigation Engine. Always present (never an `Option`): the outcome
    /// is either [`InvestigationResult::Complete`] (empty plan = the goal is
    /// self-contained), [`InvestigationResult::Skipped`], or
    /// [`InvestigationResult::Failed`] — the pipeline stays explicit.
    pub investigation: InvestigationResult,
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
            investigation: InvestigationResult::Skipped,
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

    /// Attach the Investigation Engine's outcome for this goal (M7 seam).
    /// Accepts either an [`InvestigationResult`] or a plain
    /// [`InvestigationPlan`] (which becomes [`InvestigationResult::Complete`]).
    pub fn with_investigation(mut self, investigation: impl Into<InvestigationResult>) -> Self {
        self.investigation = investigation.into();
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

        let all_candidates: Vec<PlanCandidate> = scored.into_iter().map(|(c, _)| c).collect();

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
        let tools_ok = plan.actions.iter().all(|a| world.has_tool(&a.capability));
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

        // Candidate 4: Investigation-driven plan (M7) — when the goal requires
        // information first, evidence gathering precedes execution.
        if let Some(investigation_candidate) = self.plan_from_investigation(context) {
            candidates.push(investigation_candidate);
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
        let seq_policy = context
            .policies
            .iter()
            .find(|p| p.kind == PolicyKind::Sequence)?;

        let from_cap = seq_policy
            .capability
            .clone()
            .unwrap_or(Capability::ExecuteCommand);
        let then_cap: Capability = serde_json::from_value(
            seq_policy
                .config
                .get("then")
                .cloned()
                .unwrap_or(serde_json::json!("execute_command")),
        )
        .unwrap_or(Capability::ExecuteCommand);

        let seq_label = format!("Sequence: {} then {}", from_cap, then_cap);

        let actions = vec![
            Action::step(&context.goal.description, from_cap, &context.goal.id, 0, 2),
            Action::step(&context.goal.description, then_cap, &context.goal.id, 1, 2),
        ];

        let mut plan = Plan::from_actions(actions, seq_label, PlanSource::PolicyDriven);
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
        let best = context.knowledge.iter().max_by_key(|k| k.support)?;

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

    /// Plan that gathers the required evidence before executing — the M7 seam:
    /// the planner consumes the Investigation Engine's plan, so a goal that
    /// needs information becomes `investigate → execute` rather than a blind
    /// direct action.
    ///
    /// Each required channel becomes one evidence-gathering action carrying the
    /// real skill capability id in its instruction (`web.search`, `git.status`,
    /// …) mapped to the closest built-in [`Capability`] for the runtime, and a
    /// final action executes the goal itself. Confidence = mean evidence
    /// strength, so strong requirements win evaluation over the direct plan
    /// while weak ones fold into it (the planner's judgment: not every hint
    /// warrants a separate evidence phase).
    fn plan_from_investigation(&self, context: &PlanningContext) -> Option<PlanCandidate> {
        let InvestigationResult::Complete(investigation) = &context.investigation else {
            // Skipped or Failed — no evidence candidate. The goal's own plan
            // stands; a failed investigation is never a failed goal.
            return None;
        };
        if !investigation.needs_investigation() {
            return None;
        }

        let requirements = &investigation.requirements;
        let total = requirements.len();
        let goal_id = &context.goal.id;

        let mut actions = Vec::with_capacity(total + 1);
        for (i, req) in requirements.iter().enumerate() {
            // The first suggested capability id is the honest audit trail
            // (e.g. `git.status`); the Capability enum is the runtime model.
            let cap_id = req.channel.suggested_capabilities()[0];
            // The query the evidence tool should gather: the goal itself.
            // (e.g. `web.search` needs to know what to search for).
            actions.push(
                Action::step(
                    format!("gather evidence: {} ({})", req.channel.label(), cap_id),
                    Self::capability_for_channel(req.channel),
                    goal_id,
                    i as u32,
                    (total + 1) as u32,
                )
                .with_parameters(serde_json::json!({ "query": context.goal.description })),
            );
        }
        actions.push(Action::step(
            &context.goal.description,
            self.infer_capability(&context.goal.description, &context.knowledge),
            goal_id,
            total as u32,
            (total + 1) as u32,
        ));

        let labels = requirements
            .iter()
            .map(|r| format!("{} ({:.2})", r.channel.label(), r.confidence))
            .collect::<Vec<_>>()
            .join(", ");
        let evidence = requirements.iter().map(|r| r.confidence).sum::<f32>() / total as f32;

        let plan = Plan::from_actions(
            actions,
            format!("Evidence-gated execution: {labels}"),
            PlanSource::Investigation,
        )
        .with_confidence(evidence);

        Some(PlanCandidate {
            preliminary_score: evidence,
            plan,
        })
    }

    /// The evidence action's capability IS the real skill capability id
    /// (`git.status`, `web.search`, …) carried as a [`Capability::Custom`]. An
    /// evidence action is *not* an arbitrary `execute_command`, so it must not
    /// collapse into a built-in capability where a git-backed tool would
    /// hijack every command action in the registry. The id is the same one the
    /// instruction carries, so the audit trail and the registry key agree.
    fn capability_for_channel(channel: InvestigationChannel) -> Capability {
        Capability::Custom(channel.suggested_capabilities()[0].to_string())
    }

    /// Legacy entry point: single plan (backward compatible).
    ///
    /// Returns the best candidate's actions by the planner's own preliminary
    /// judgment — so when the Investigation Engine produced strong evidence
    /// requirements, the evidence-gated plan (`PlanSource::Investigation`)
    /// wins here too, instead of the blind direct action. Weak requirements
    /// still fold back into the direct plan.
    pub fn plan(&self, context: &PlanningContext) -> Vec<Action> {
        let candidates = self.think(context);
        candidates
            .into_iter()
            .max_by(|a, b| {
                a.preliminary_score
                    .partial_cmp(&b.preliminary_score)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
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

#[cfg(test)]
mod tests {
    use super::*;
    use friday_investigation::{InformationRequirement, InvestigationPlan};

    /// Deterministic investigation plan helper: channel → confidence.
    fn plan_with_requirements(reqs: Vec<(InvestigationChannel, f32)>) -> InvestigationPlan {
        InvestigationPlan {
            requirements: reqs
                .into_iter()
                .map(|(channel, confidence)| InformationRequirement {
                    channel,
                    rationale: channel.label().to_string(),
                    confidence,
                })
                .collect(),
        }
    }

    #[test]
    fn investigation_candidate_generated_when_goal_requires_evidence() {
        let planning = Planning::new();
        let ctx = PlanningContext::new(Goal::new("Search the web for a guide")).with_investigation(
            plan_with_requirements(vec![(InvestigationChannel::Web, 0.9)]),
        );
        let candidates = planning.think(&ctx);
        assert!(candidates
            .iter()
            .any(|c| c.plan.source == PlanSource::Investigation));
    }

    #[test]
    fn no_investigation_candidate_when_nothing_required() {
        let planning = Planning::new();
        let ctx = PlanningContext::new(Goal::new("open notepad"))
            .with_investigation(InvestigationPlan::default());
        let candidates = planning.think(&ctx);
        assert!(!candidates
            .iter()
            .any(|c| c.plan.source == PlanSource::Investigation));
    }

    #[test]
    fn strong_evidence_wins_evaluation_over_direct_plan() {
        let planning = Planning::new();
        let evaluator = PlanEvaluator::new();
        let world = WorldState::new()
            .with_tool(Capability::SearchWeb)
            .with_tool(Capability::ExecuteCommand)
            .with_tool(Capability::Custom("web.search".into()))
            .with_tool(Capability::Custom("git.status".into()));
        let ctx = PlanningContext::new(Goal::new("Search the web and check the git repo"))
            .with_world_state(world.clone())
            .with_investigation(plan_with_requirements(vec![
                (InvestigationChannel::Web, 0.9),
                (InvestigationChannel::Git, 0.8),
            ]));

        let candidates = planning.think(&ctx);
        let ranked = evaluator.evaluate(&candidates, &world);
        assert_eq!(ranked.plan.source, PlanSource::Investigation);
        assert!(
            ranked.plan.strategy.contains("web"),
            "{}",
            ranked.plan.strategy
        );
        assert!(
            ranked.plan.strategy.contains("git"),
            "{}",
            ranked.plan.strategy
        );
    }

    #[test]
    fn weak_evidence_folds_into_direct_plan() {
        let planning = Planning::new();
        let evaluator = PlanEvaluator::new();
        let world = WorldState::new()
            .with_tool(Capability::ReadFile)
            .with_tool(Capability::ListDirectory)
            .with_tool(Capability::ExecuteCommand);
        let ctx = PlanningContext::new(Goal::new("Show me the file"))
            .with_world_state(world.clone())
            // A lone 0.4 signal: offered as a candidate, but the planner's
            // judgment folds it into the direct plan.
            .with_investigation(plan_with_requirements(vec![(
                InvestigationChannel::Filesystem,
                0.4,
            )]));

        let candidates = planning.think(&ctx);
        assert!(candidates
            .iter()
            .any(|c| c.plan.source == PlanSource::Investigation));
        let ranked = evaluator.evaluate(&candidates, &world);
        assert_eq!(ranked.plan.source, PlanSource::Direct);
    }

    #[test]
    fn investigation_plan_orders_evidence_before_execution() {
        let planning = Planning::new();
        let ctx = PlanningContext::new(Goal::new("Check git status of the workspace repository"))
            .with_investigation(plan_with_requirements(vec![
                (InvestigationChannel::Git, 0.8),
                (InvestigationChannel::CodeIntel, 0.7),
            ]));
        let candidates = planning.think(&ctx);
        let plan = candidates
            .iter()
            .find(|c| c.plan.source == PlanSource::Investigation)
            .expect("investigation plan")
            .plan
            .clone();

        assert_eq!(plan.actions.len(), 3, "2 evidence + 1 execute");
        // Evidence actions carry the real skill capability ids.
        assert!(
            plan.actions[0].instruction.contains("git.status"),
            "{}",
            plan.actions[0].instruction
        );
        assert!(
            plan.actions[1].instruction.contains("codeintel.understand"),
            "{}",
            plan.actions[1].instruction
        );
        assert_eq!(
            plan.actions[0].capability,
            Capability::Custom("git.status".into()),
            "evidence capability is the real skill id"
        );
        assert_eq!(
            plan.actions[1].capability,
            Capability::Custom("codeintel.understand".into())
        );
        // The goal itself executes last.
        assert_eq!(
            plan.actions[2].instruction,
            "Check git status of the workspace repository"
        );
        assert!(plan.actions[0].is_first());
        assert!(plan.actions[2].is_last());
        assert!(plan.actions[0].is_multi_step());
    }

    #[test]
    fn context_round_trips_investigation_plan() {
        let plan = plan_with_requirements(vec![(InvestigationChannel::Memory, 0.6)]);
        let ctx = PlanningContext::new(Goal::new("remember the preference"))
            .with_investigation(plan.clone());
        assert_eq!(ctx.investigation, InvestigationResult::Complete(plan));
    }

    #[test]
    fn skipped_and_failed_investigations_produce_no_evidence_candidate() {
        let planning = Planning::new();

        let skipped = PlanningContext::new(Goal::new("Search the web for a guide"))
            .with_investigation(InvestigationResult::Skipped);
        let candidates = planning.think(&skipped);
        assert!(!candidates
            .iter()
            .any(|c| c.plan.source == PlanSource::Investigation));

        let failed = PlanningContext::new(Goal::new("Search the web for a guide"))
            .with_investigation(InvestigationResult::Failed {
                reason: "strategy unavailable".into(),
            });
        let candidates = planning.think(&failed);
        assert!(!candidates
            .iter()
            .any(|c| c.plan.source == PlanSource::Investigation));
        // The direct plan still stands for the goal.
        assert_eq!(planning.plan(&failed).len(), 1);
    }

    #[test]
    fn legacy_plan_consumes_the_investigation_plan() {
        let planning = Planning::new();
        // Strong evidence → the evidence-gated plan wins the legacy entry point
        // too, so `plan()` never produces a blind direct action.
        let strong =
            PlanningContext::new(Goal::new("Search the web for a guide")).with_investigation(
                plan_with_requirements(vec![(InvestigationChannel::Web, 0.9)]),
            );
        let actions = planning.plan(&strong);
        assert_eq!(actions.len(), 2, "evidence + execute actions");
        assert!(
            actions[0].instruction.contains("web.search"),
            "first action gathers evidence: {}",
            actions[0].instruction
        );

        // Weak evidence folds back into the direct plan (planner's judgment).
        let weak = PlanningContext::new(Goal::new("Show me the file")).with_investigation(
            plan_with_requirements(vec![(InvestigationChannel::Filesystem, 0.4)]),
        );
        let actions = planning.plan(&weak);
        assert_eq!(actions.len(), 1);
    }

    /// `plan()` now returns the planner's best candidate, not the first — a
    /// high-confidence sequencing policy also wins the legacy entry point.
    /// Locks in the intended semantics so a future caller isn't surprised.
    #[test]
    fn legacy_plan_prefers_high_confidence_policy_candidate() {
        let planning = Planning::new();
        let policy = Policy::sequence(Capability::ExecuteCommand, Capability::ReadFile, 0.8, 5);
        let ctx = PlanningContext::new(Goal::new("build project")).with_policies(vec![policy]);
        let actions = planning.plan(&ctx);
        assert_eq!(actions.len(), 2, "policy plan (2 actions) wins over direct");
        assert!(
            actions[0].capability == Capability::ExecuteCommand
                && actions[1].capability == Capability::ReadFile
        );
    }
}
