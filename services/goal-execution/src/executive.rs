use serde::{Deserialize, Serialize};

use crate::Goal;
use crate::Confidence;
use crate::Action;
use crate::Capability;
use crate::Expectation;
use crate::ExecutionContext;
use crate::Tool;
use crate::ToolResult;
use crate::VerifiedResult;
use crate::Verification;
use crate::FocusTarget;
use crate::CognitiveEpisode;
use crate::EpisodeOutcome;
use crate::EpisodeStore;
use crate::AttentionScore;
use crate::SemanticMemory;
use crate::AttentionManager;
use crate::AttentionItem;
use crate::AttentionItemKind;
use crate::CognitiveBlackboard;
use crate::Plan;
use crate::Planning;
use crate::PlanEvaluator;
use crate::PlanningContext;
use crate::GoalEngine;
use crate::ToolRegistry;
use crate::simulation::{DefaultSimulator, Simulator};
use std::sync::Arc;
use friday_reflection::{ReflectionEngine};
use friday_knowledge::KnowledgeRecord;

// ── Executive ─────────────────────────────────────────────────────

/// Phase of the executive cycle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ExecutivePhase {
    /// Observing the workspace and attention queue.
    Observe,
    /// Selecting the next focus target.
    Attend,
    /// Generating and evaluating plans.
    Plan,
    /// Simulating candidate plans.
    Simulate,
    /// Executing the chosen plan.
    Execute,
    /// Verifying the execution result.
    Verify,
    /// Reflecting on the outcome.
    Reflect,
    /// Storing as an episode.
    Consolidate,
    /// Deriving patterns and knowledge.
    Learn,
    /// Cycle complete, waiting for next tick.
    Idle,
}

impl std::fmt::Display for ExecutivePhase {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Observe => write!(f, "observe"),
            Self::Attend => write!(f, "attend"),
            Self::Plan => write!(f, "plan"),
            Self::Simulate => write!(f, "simulate"),
            Self::Execute => write!(f, "execute"),
            Self::Verify => write!(f, "verify"),
            Self::Reflect => write!(f, "reflect"),
            Self::Consolidate => write!(f, "consolidate"),
            Self::Learn => write!(f, "learn"),
            Self::Idle => write!(f, "idle"),
        }
    }
}

/// Result of one executive cycle.
#[derive(Debug, Clone, Default)]
pub struct ExecutiveReport {
    pub phases_completed: Vec<ExecutivePhase>,
    pub goal_created: bool,
    pub plan_executed: bool,
    pub episode_stored: bool,
    pub patterns_promoted: usize,
    pub duration_ms: u64,
    pub error: Option<String>,
}

/// The Executive — orchestrates the cognitive cycle.
///
/// The Executive does not implement planning, execution, or learning.
/// It **coordinates** existing components in the correct order.
///
/// ```text
/// Blackboard
///     │
///     ▼
/// Observe
///     │
///     ▼
/// Attend (AttentionManager → FocusTarget)
///     │
///     ▼
/// Plan (Planning → PlanEvaluator → RankedPlan)
///     │
///     ▼
/// Simulate (Simulator → SimulationResult)
///     │
///     ▼
/// Execute (GoalEngine → GoalReport)
///     │
///     ▼
/// Verify (Verification → VerifiedResult)
///     │
///     ▼
/// Reflect (ReflectionEngine → Insight)
///     │
///     ▼
/// Consolidate (EpisodeStore)
///     │
///     ▼
/// Learn (SemanticMemory → Knowledge/Policies)
///     │
///     ▼
/// Blackboard (updated)
/// ```
pub struct Executive {
    phase: ExecutivePhase,
    planner: Planning,
    evaluator: PlanEvaluator,
    simulator: DefaultSimulator,
    goal_engine: GoalEngine,
    verification: Verification,
    reflection_engine: ReflectionEngine,
    semantic_memory: SemanticMemory,
    episode_store: EpisodeStore,
    attention_manager: AttentionManager,
    /// Tick counter.
    cycle_count: u64,
}

impl Executive {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn cycle(&mut self, workspace: &Arc<CognitiveBlackboard>) -> ExecutiveReport {
        let started_ms = chrono::Utc::now().timestamp_millis() as u64;
        let mut report = ExecutiveReport::default();
        self.cycle_count += 1;

        self.phase = ExecutivePhase::Observe;
        report.phases_completed.push(ExecutivePhase::Observe);
        workspace.push_thought("executive: observing workspace");

        if let Some(world) = workspace.get_world_state() {
            if world.confidence < Confidence::medium() {
                let item = AttentionItem::new(
                    AttentionItemKind::Observation,
                    "world state confidence is low",
                    AttentionScore::builder()
                        .importance(0.5)
                        .urgency(0.4)
                        .confidence(world.confidence.into())
                        .build(),
                );
                self.attention_manager.add(item);
            }
        }

        if let Some(episode) = workspace.get_current_episode() {
            if episode.is_complete() {
                workspace.push_thought("executive: previous episode complete, ready to consolidate");
            }
        }

        self.phase = ExecutivePhase::Attend;
        report.phases_completed.push(ExecutivePhase::Attend);

        self.attention_manager.decay_novelty();
        workspace.set_attention(self.attention_manager.ranked().into_iter().cloned().collect::<Vec<_>>());

        let focus_item = self.attention_manager.focus();
        let focus_target = match &focus_item {
            Some(item) => {
                workspace.push_thought(format!("executive: focusing on: {}", item.description));
                match &item.kind {
                    AttentionItemKind::Goal => FocusTarget::Goal(item.id.clone()),
                    AttentionItemKind::Observation => FocusTarget::Observation(item.description.clone()),
                    AttentionItemKind::Belief => FocusTarget::Observation(item.description.clone()),
                    _ => FocusTarget::Planning,
                }
            }
            None => FocusTarget::Idle,
        };

        self.phase = ExecutivePhase::Plan;
        report.phases_completed.push(ExecutivePhase::Plan);

        let (goal, plan) = match &focus_target {
            FocusTarget::Idle => {
                workspace.push_thought("executive: no focus target, idling");
                report.phases_completed.push(ExecutivePhase::Idle);
                report.duration_ms = chrono::Utc::now().timestamp_millis() as u64 - started_ms;
                return report;
            }
            FocusTarget::Goal(goal_id) => {
                workspace.push_thought(format!("executive: planning for goal {}", goal_id));
                let goal = workspace.get_active_goal()
                    .unwrap_or_else(|| Goal::new(goal_id.clone()));
                let world = workspace.get_world_state().unwrap_or_default();
                let candidates = self.planner.think(&PlanningContext::new(goal.clone()));
                let ranked = self.evaluator.evaluate(&candidates, &world);
                let plan = ranked.plan.clone();
                workspace.set_active_plan(plan.clone());
                workspace.push_thought(format!("executive: plan created: {} (score: {:.2})", plan.strategy, ranked.score));
                (goal, Some(plan))
            }
            FocusTarget::Observation(desc) => {
                workspace.push_thought(format!("executive: investigating observation: {}", desc));
                let goal = Goal::new(desc.clone());
                workspace.set_active_goal(goal.clone());
                let world = workspace.get_world_state().unwrap_or_default();
                let candidates = self.planner.think(&PlanningContext::new(goal.clone()));
                let ranked = self.evaluator.evaluate(&candidates, &world);
                let plan = ranked.plan.clone();
                workspace.set_active_plan(plan.clone());
                (goal, Some(plan))
            }
            _ => {
                workspace.push_thought("executive: continuing current cognitive state");
                let goal = workspace.get_active_goal().unwrap_or_else(|| Goal::new("continue"));
                let plan = workspace.get_active_plan();
                (goal, plan)
            }
        };

        self.phase = ExecutivePhase::Simulate;
        report.phases_completed.push(ExecutivePhase::Simulate);

        let simulation_result = if let Some(ref p) = plan {
            let world = workspace.get_world_state().unwrap_or_default();
            let sim_result = self.simulator.simulate(p, &world);
            workspace.set_predictions(vec![sim_result.clone()]);
            workspace.push_thought(format!(
                "executive: simulation {} (confidence: {:.2}, risks: {})",
                if sim_result.feasible { "feasible" } else { "infeasible" },
                sim_result.confidence,
                sim_result.risks.len(),
            ));
            Some(sim_result)
        } else {
            None
        };

        self.phase = ExecutivePhase::Execute;
        report.phases_completed.push(ExecutivePhase::Execute);

        let goal_report = if let (Some(g), Some(ref p)) = (Some(&goal), &plan) {
            if let Some(sim) = &simulation_result {
                if !sim.feasible {
                    workspace.push_thought("executive: skipping execution — simulation infeasible");
                    None
                } else if let Some(action) = p.actions.first() {
                    workspace.push_thought(format!("executive: executing: {}", action.instruction));

                    let expectation = Expectation::success(&action.instruction);
                    let exec_ctx = ExecutionContext::for_goal(&g.id);
                    let gr = self.goal_engine.execute(g, action, &expectation, &exec_ctx).await;

                    let tool_result = ToolResult {
                        success: gr.success(),
                        output: gr.outcome.context.clone(),
                        error: if gr.success() { None } else { Some(gr.outcome.context.clone()) },
                        duration_ms: gr.trace.duration_ms(),
                        metadata: serde_json::json!({}),
                    };
                    workspace.set_execution(tool_result);
                    workspace.set_current_trace(gr.cognitive_trace.clone());

                    workspace.push_thought(format!(
                        "executive: verified (verified={}, confidence={:.2})",
                        gr.verification.verified, gr.verification.confidence.value()
                    ));

                    report.plan_executed = true;
                    Some(gr)
                } else {
                    None
                }
            } else {
                None
            }
        } else {
            None
        };

        self.phase = ExecutivePhase::Verify;
        report.phases_completed.push(ExecutivePhase::Verify);

        if goal_report.is_some() {
            if let Some(ref p) = plan {
                if let Some(action) = p.actions.first() {
                    let tool_result = ToolResult::success("verify ok");
                    let verified = self.verification.verify(action, &tool_result);
                    workspace.push_thought(format!(
                        "executive: verification {} ({} checks)",
                        if verified.verified { "passed" } else { "failed" },
                        verified.checks.len()
                    ));
                }
            }
        }

        self.phase = ExecutivePhase::Reflect;
        report.phases_completed.push(ExecutivePhase::Reflect);

        if let Some(ref gr) = goal_report {
            let outcome_kind = if gr.success() && gr.verification.verified {
                friday_reflection::OutcomeKind::TaskCompleted
            } else if !gr.success() && gr.verification.verified {
                friday_reflection::OutcomeKind::TaskAbandoned
            } else if gr.success() && !gr.verification.verified {
                friday_reflection::OutcomeKind::UnexpectedSuccess
            } else {
                friday_reflection::OutcomeKind::UnexpectedFailure
            };

            let outcome = friday_reflection::Outcome {
                id: format!("outcome-{}", started_ms),
                kind: outcome_kind,
                context: goal.description.clone(),
                detail: serde_json::json!({}),
                occurred_at_ms: chrono::Utc::now().timestamp_millis() as u64,
            };

            let dummy_knowledge = KnowledgeRecord {
                id: "builtin".into(),
                kind: friday_knowledge::KnowledgeKind::Workflow,
                label: "default".into(),
                description: "default knowledge".into(),
                pattern: vec![],
                support: 0,
                confidence: Confidence::medium().into(),
                derived_at_ms: 0,
            };
            let insights = self.reflection_engine.reflect(&dummy_knowledge, &outcome);

            if let Some(ref insight) = insights {
                workspace.push_thought(format!(
                    "executive: reflection — {} (confidence: {:.2})",
                    insight.statement, insight.confidence
                ));
                workspace.set_reflection(insight.statement.clone());
            }
        }

        self.phase = ExecutivePhase::Consolidate;
        report.phases_completed.push(ExecutivePhase::Consolidate);

        let world_before = workspace.get_world_state().unwrap_or_default();
        let world_after = workspace.get_world_state().unwrap_or_default();
        let ver = goal_report.as_ref().map(|gr| &gr.verification);
        let episode = CognitiveEpisode::begin(goal)
            .with_world_before(world_before)
            .with_plan(plan.clone().unwrap_or(Plan::from_action(
                Action::single("no-op", Capability::ExecuteCommand, "default"),
                "fallback",
            )))
            .with_execution_result(
                ToolResult::success("completed")
            )
            .with_verification(VerifiedResult {
                verified: ver.is_some_and(|v| v.verified),
                tool_success: ver.is_some_and(|v| v.tool_success),
                checks: ver.map_or(vec![], |v| v.checks.clone()),
                confidence: ver.map_or(Confidence::low(), |v| v.confidence),
            })
            .with_reflection(workspace.get_reflection().unwrap_or_default())
            .with_world_after(world_after)
            .finish(if goal_report.as_ref().is_some_and(|gr| gr.success()) {
                EpisodeOutcome::Success
            } else {
                EpisodeOutcome::Failure
            });

        self.episode_store.store(episode.clone());
        workspace.set_current_episode(episode);
        report.episode_stored = true;

        self.phase = ExecutivePhase::Learn;
        report.phases_completed.push(ExecutivePhase::Learn);

        let learning_result = self.semantic_memory.process_episodes(&self.episode_store);
        report.patterns_promoted = learning_result.patterns_promoted;
        if learning_result.patterns_promoted > 0 {
            workspace.push_thought(format!(
                "executive: learned {} new patterns, {} policies proposed",
                learning_result.patterns_detected,
                learning_result.policies_proposed,
            ));
        }

        self.phase = ExecutivePhase::Idle;
        workspace.clear_execution();

        report.duration_ms = chrono::Utc::now().timestamp_millis() as u64 - started_ms;
        report
    }

    // ── Accessors ────────────────────────────────────────────────

    pub fn phase(&self) -> ExecutivePhase {
        self.phase
    }

    pub fn cycle_count(&self) -> u64 {
        self.cycle_count
    }

    pub fn episode_store(&self) -> &EpisodeStore {
        &self.episode_store
    }

    pub fn semantic_memory(&self) -> &SemanticMemory {
        &self.semantic_memory
    }

    pub fn attention_manager(&self) -> &AttentionManager {
        &self.attention_manager
    }

    pub fn attention_manager_mut(&mut self) -> &mut AttentionManager {
        &mut self.attention_manager
    }

    /// Register a tool with the executive's goal engine.
    pub fn register_tool(&mut self, tool: Box<dyn Tool>) {
        self.goal_engine.registry.register(tool);
    }

    /// Manually inject a goal into the attention queue.
    pub fn add_goal(&mut self, description: impl Into<String>, importance: f32) {
        let goal = Goal::new(description);
        let item = AttentionItem::new(
            AttentionItemKind::Goal,
            goal.description.clone(),
            AttentionScore::builder().importance(importance).urgency(0.5).build(),
        );
        self.attention_manager.add(item);
    }

    /// Directly set an observation in the attention queue.
    pub fn add_observation(&mut self, description: impl Into<String>, importance: f32) {
        let item = AttentionItem::new(
            AttentionItemKind::Observation,
            description,
            AttentionScore::builder().importance(importance).novelty(0.8).build(),
        );
        self.attention_manager.add(item);
    }
}

impl Default for Executive {
    fn default() -> Self {
        Self {
            phase: ExecutivePhase::Idle,
            planner: Planning::new(),
            evaluator: PlanEvaluator::new(),
            simulator: DefaultSimulator::new(),
            goal_engine: GoalEngine::new()
                .with_registry(ToolRegistry::new())
                .with_verification(Verification::new()),
            verification: Verification::new(),
            reflection_engine: ReflectionEngine::new(),
            semantic_memory: SemanticMemory::new(),
            episode_store: EpisodeStore::new(),
            attention_manager: AttentionManager::new(),
            cycle_count: 0,
        }
    }
}
