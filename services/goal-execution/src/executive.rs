use serde::{Deserialize, Serialize};

use crate::simulation::{DefaultSimulator, Simulator};
use crate::Action;
use crate::AttentionItem;
use crate::AttentionItemKind;
use crate::AttentionManager;
use crate::AttentionScore;
use crate::Capability;
use crate::CognitiveBlackboard;
use crate::CognitiveEpisode;
use crate::CognitiveTrace;
use crate::Confidence;
use crate::EpisodeOutcome;
use crate::EpisodeStore;
use crate::ExecutionContext;
use crate::Expectation;
use crate::FocusTarget;
use crate::Goal;
use crate::GoalEngine;
use crate::Plan;
use crate::PlanEvaluator;
use crate::Planning;
use crate::PlanningContext;
use crate::RankedPlan;
use crate::SemanticMemory;
use crate::Tool;
use crate::ToolRegistry;
use crate::ToolResult;
use crate::TraceNode;
use crate::TraceNodeKind;
use crate::Verification;
use crate::VerifiedResult;
use friday_investigation::{InvestigationEngine, InvestigationResult};
use friday_knowledge::KnowledgeRecord;
use friday_reflection::ReflectionEngine;
use std::sync::Arc;

// ── Executive ─────────────────────────────────────────────────────

/// Phase of the executive cycle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ExecutivePhase {
    /// Observing the workspace and attention queue.
    Observe,
    /// Selecting the next focus target.
    Attend,
    /// The evidence gate (M7): what the focused goal requires knowing before
    /// planning. Runs investigation and hands the always-present result to
    /// the Plan phase. Only Goal/Observation targets enter this phase;
    /// Idle and continuing states skip it (nothing to investigate).
    Investigate,
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
            Self::Investigate => write!(f, "investigate"),
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
/// Investigate (M7 evidence gate — what must the goal know before acting?)
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
    /// Decides what a goal requires knowing before planning (M7). The
    /// executive runs investigation on every focused goal and hands the plan
    /// to `PlanningContext` so the planner can gate execution on evidence.
    investigation_engine: InvestigationEngine,
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
                workspace
                    .push_thought("executive: previous episode complete, ready to consolidate");
            }
        }

        self.phase = ExecutivePhase::Attend;
        report.phases_completed.push(ExecutivePhase::Attend);

        self.attention_manager.decay_novelty();
        workspace.set_attention(
            self.attention_manager
                .ranked()
                .into_iter()
                .cloned()
                .collect::<Vec<_>>(),
        );

        let focus_item = self.attention_manager.focus();
        let focus_target = match &focus_item {
            Some(item) => {
                workspace.push_thought(format!("executive: focusing on: {}", item.description));
                match &item.kind {
                    AttentionItemKind::Goal => FocusTarget::Goal(item.id.clone()),
                    AttentionItemKind::Observation => {
                        FocusTarget::Observation(item.description.clone())
                    }
                    AttentionItemKind::Belief => FocusTarget::Observation(item.description.clone()),
                    _ => FocusTarget::Planning,
                }
            }
            None => FocusTarget::Idle,
        };

        // M7: the evidence gate is its own cycle phase — think before acting.
        // Resolve the focused goal and run investigation before planning. Only
        // Goal/Observation targets enter the phase (Idle and continuing states
        // have nothing to investigate), so the audit trail records the gate
        // exactly when it fires.
        let focus = match &focus_target {
            FocusTarget::Goal(goal_id) => {
                self.phase = ExecutivePhase::Investigate;
                report.phases_completed.push(ExecutivePhase::Investigate);
                // The focus target carries the attention item's id; resolve the
                // real goal text from the workspace (the item's description) so
                // investigation and planning see the goal, not a uuid.
                let goal = workspace
                    .get_active_goal()
                    .or_else(|| {
                        workspace
                            .get_attention()
                            .into_iter()
                            .find(|i| i.id == *goal_id)
                            .map(|i| Goal::new(i.description))
                    })
                    .unwrap_or_else(|| Goal::new(goal_id.clone()));
                let investigation = self.investigate_for(&goal, workspace).await;
                Some((goal, investigation))
            }
            FocusTarget::Observation(desc) => {
                self.phase = ExecutivePhase::Investigate;
                report.phases_completed.push(ExecutivePhase::Investigate);
                let goal = Goal::new(desc.clone());
                workspace.set_active_goal(goal.clone());
                let investigation = self.investigate_for(&goal, workspace).await;
                Some((goal, investigation))
            }
            _ => None,
        };

        self.phase = ExecutivePhase::Plan;
        report.phases_completed.push(ExecutivePhase::Plan);

        // The third element is the planning segment of the cognitive trace
        // (investigation → chosen plan); the GoalEngine's execution nodes are
        // merged in later so the full trace reads top-down.
        let (goal, plan, planning_trace) = match &focus_target {
            FocusTarget::Idle => {
                self.phase = ExecutivePhase::Idle;
                workspace.push_thought("executive: no focus target, idling");
                report.phases_completed.push(ExecutivePhase::Idle);
                report.duration_ms = chrono::Utc::now().timestamp_millis() as u64 - started_ms;
                return report;
            }
            // Goal carries the attention-item id, Observation the raw text —
            // both resolve to a real goal + evidence gate, so they share one
            // investigate → plan path.
            FocusTarget::Goal(desc) | FocusTarget::Observation(desc) => {
                workspace.push_thought(format!("executive: planning for: {desc}"));
                let (goal, investigation) =
                    focus.expect("goal/observation focus always investigates");
                let world = workspace.get_world_state().unwrap_or_default();
                let ctx = PlanningContext::new(goal.clone()).with_investigation(investigation);
                let candidates = self.planner.think(&ctx);
                let ranked = self.evaluator.evaluate(&candidates, &world);

                let plan = ranked.plan.clone();
                let planning_trace = match &ctx.investigation {
                    InvestigationResult::Complete(inv) => {
                        Some(self.planning_trace(&goal, inv, &ranked))
                    }
                    _ => None,
                };
                workspace.set_active_plan(plan.clone());
                workspace.push_thought(format!(
                    "executive: plan created: {} (score: {:.2})",
                    plan.strategy, ranked.score
                ));
                (goal, Some(plan), planning_trace)
            }
            _ => {
                workspace.push_thought("executive: continuing current cognitive state");
                let goal = workspace
                    .get_active_goal()
                    .unwrap_or_else(|| Goal::new("continue"));
                let plan = workspace.get_active_plan();
                (goal, plan, None)
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
                if sim_result.feasible {
                    "feasible"
                } else {
                    "infeasible"
                },
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
                    let gr = self
                        .goal_engine
                        .execute(g, action, &expectation, &exec_ctx)
                        .await;

                    let tool_result = ToolResult {
                        success: gr.success(),
                        output: gr.outcome.context.clone(),
                        error: if gr.success() {
                            None
                        } else {
                            Some(gr.outcome.context.clone())
                        },
                        duration_ms: gr.trace.duration_ms(),
                        metadata: serde_json::json!({}),
                    };
                    workspace.set_execution(tool_result);
                    // Surface the full decision trail top-down: investigation →
                    // chosen plan (from this cycle) followed by the GoalEngine's
                    // execution → verification → reflection nodes.
                    let mut trace = planning_trace
                        .clone()
                        .unwrap_or_else(|| CognitiveTrace::new(&g.id));
                    trace.nodes.extend(gr.cognitive_trace.nodes.iter().cloned());
                    workspace.set_current_trace(trace);

                    workspace.push_thought(format!(
                        "executive: verified (verified={}, confidence={:.2})",
                        gr.verification.verified,
                        gr.verification.confidence.value()
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

        // Even when execution is skipped (e.g. simulation judged the plan
        // infeasible), the planning decision itself must stay inspectable.
        if goal_report.is_none() {
            if let Some(trace) = planning_trace {
                workspace.set_current_trace(trace);
            }
        }

        self.phase = ExecutivePhase::Verify;
        report.phases_completed.push(ExecutivePhase::Verify);

        if goal_report.is_some() {
            if let Some(ref p) = plan {
                if let Some(action) = p.actions.first() {
                    let tool_result = ToolResult::success("verify ok");
                    let verified = self.verification.verify(action, &tool_result);
                    workspace.push_thought(format!(
                        "executive: verification {} ({} checks)",
                        if verified.verified {
                            "passed"
                        } else {
                            "failed"
                        },
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
            .with_execution_result(ToolResult::success("completed"))
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
                learning_result.patterns_detected, learning_result.policies_proposed,
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

    /// Build the planning segment of the cognitive trace: an Investigation
    /// node (what the goal required knowing first, with the evidence channels)
    /// and a Planning node (the chosen plan's source and score). The
    /// GoalEngine's execution nodes are appended by the caller, so the full
    /// trace reads investigation → planning → execution → verification →
    /// reflection.
    fn planning_trace(
        &self,
        goal: &Goal,
        investigation: &friday_investigation::InvestigationPlan,
        ranked: &RankedPlan,
    ) -> CognitiveTrace {
        let now = chrono::Utc::now().timestamp_millis() as u64;
        let mut trace = CognitiveTrace::new(&goal.id);

        let channels: Vec<&str> = investigation
            .requirements
            .iter()
            .map(|r| r.channel.label())
            .collect();
        let mean_confidence = if investigation.requirements.is_empty() {
            0.0
        } else {
            investigation
                .requirements
                .iter()
                .map(|r| r.confidence)
                .sum::<f32>()
                / investigation.requirements.len() as f32
        };

        let mut investigation_node = TraceNode::new(
            TraceNodeKind::Investigation,
            serde_json::json!({ "goal": goal.description }),
            serde_json::json!({
                "self_contained": !investigation.needs_investigation(),
                "requirement_count": investigation.requirements.len(),
                "channels": channels,
            }),
        )
        .with_timing(now, now);
        // Confidence only carries meaning when requirements exist.
        investigation_node.confidence = if investigation.requirements.is_empty() {
            None
        } else {
            Some(mean_confidence.clamp(0.0, 1.0))
        };
        trace.push(investigation_node);

        let plan_node = TraceNode::new(
            TraceNodeKind::Planning,
            serde_json::json!({ "candidate_count": ranked.all_candidates.len() + 1 }),
            serde_json::json!({
                "source": format!("{:?}", ranked.plan.source),
                "strategy": ranked.plan.strategy,
                "score": ranked.score,
            }),
        )
        .with_confidence(ranked.score)
        .with_timing(now, now);
        trace.push(plan_node);

        trace
    }

    /// Run investigation for a goal and narrate the outcome (M7). Shared by
    /// both focus branches so the "think before acting" story stays consistent.
    /// Returns the always-present [`InvestigationResult`] so the planning
    /// context never carries an `Option`. Async because the strategy seam is
    /// async (an LLM-backed strategy is a drop-in for the deterministic
    /// heuristic).
    async fn investigate_for(
        &self,
        goal: &Goal,
        workspace: &Arc<CognitiveBlackboard>,
    ) -> friday_investigation::InvestigationResult {
        let investigation = self
            .investigation_engine
            .investigate(&goal.description)
            .await;
        let result = InvestigationResult::Complete(investigation);
        // Publish the gate outcome to the workspace so observers (the bridge's
        // live InvestigationGraph, audits) read the exact plan that gated
        // planning — never a re-derivation from the goal string.
        if let InvestigationResult::Complete(plan) = &result {
            workspace.set_investigation(goal.description.clone(), plan.clone());
        }
        if result.needs_investigation() {
            workspace.push_thought(format!(
                "executive: investigation required before planning: {}",
                result.describe()
            ));
        } else {
            // `Skipped`/`Failed` cannot originate from today's engine (the
            // heuristic always produces a plan; the LLM strategy falls back to
            // it), but the narration covers them so a future strategy that
            // can genuinely skip or fail stays observable.
            let note = match &result {
                InvestigationResult::Complete(_) => {
                    "executive: no investigation required — goal is self-contained".to_string()
                }
                InvestigationResult::Skipped => "executive: investigation skipped".to_string(),
                InvestigationResult::Failed { reason } => {
                    format!("executive: investigation failed: {reason}")
                }
            };
            workspace.push_thought(note);
        }
        result
    }

    /// Swap the investigation strategy (e.g. an LLM-backed engine whose model
    /// derives the requirements and the heuristic catches failures). Defaults
    /// to the deterministic heuristic.
    pub fn with_investigation_engine(mut self, engine: InvestigationEngine) -> Self {
        self.investigation_engine = engine;
        self
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
            AttentionScore::builder()
                .importance(importance)
                .urgency(0.5)
                .build(),
        );
        self.attention_manager.add(item);
    }

    /// Directly set an observation in the attention queue.
    pub fn add_observation(&mut self, description: impl Into<String>, importance: f32) {
        let item = AttentionItem::new(
            AttentionItemKind::Observation,
            description,
            AttentionScore::builder()
                .importance(importance)
                .novelty(0.8)
                .build(),
        );
        self.attention_manager.add(item);
    }
}

impl Default for Executive {
    fn default() -> Self {
        Self {
            phase: ExecutivePhase::Idle,
            planner: Planning::new(),
            investigation_engine: InvestigationEngine::heuristic(),
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{MockTool, PlanSource, WorldState};

    #[tokio::test]
    async fn executive_investigates_before_planning_and_executes_evidence() {
        let workspace = CognitiveBlackboard::new();
        let mut executive = Executive::new();
        executive.register_tool(Box::new(
            MockTool::success("web", "search results").with_capabilities(vec![
                Capability::SearchWeb,
                Capability::Custom("web.search".into()),
            ]),
        ));
        executive.register_tool(Box::new(
            MockTool::success("docs", "docs content").with_capabilities(vec![
                Capability::ReadFile,
                Capability::Custom("docs.search".into()),
            ]),
        ));
        // The legacy infer_capability classifies the goal as execute_command
        // ("Rust" contains "run") — give the world that tool too so the
        // evidence-gated plan is fully feasible.
        executive.register_tool(Box::new(
            MockTool::success("terminal", "ok").with_capabilities(vec![Capability::ExecuteCommand]),
        ));
        workspace.set_world_state(
            WorldState::new()
                .with_tool(Capability::SearchWeb)
                .with_tool(Capability::ReadFile)
                .with_tool(Capability::ExecuteCommand)
                .with_tool(Capability::Custom("web.search".into()))
                .with_tool(Capability::Custom("docs.search".into())),
        );

        // "Search the web for the latest Rust async docs" fires web (1.0) and
        // docs (0.6) → the evidence-gated plan must win and its first action
        // (gather evidence: web) must actually execute.
        executive.add_observation("Search the web for the latest Rust async docs", 0.9);
        let report = executive.cycle(&workspace).await;

        let plan = workspace.get_active_plan().expect("a plan was produced");
        assert_eq!(
            plan.source,
            PlanSource::Investigation,
            "evidence-gated plan should win: {}",
            plan.strategy
        );
        assert!(
            plan.actions[0].instruction.contains("web.search"),
            "first action gathers web evidence: {}",
            plan.actions[0].instruction
        );
        assert!(
            report.plan_executed,
            "the evidence action executed — thoughts: {:?} predictions: {:?}",
            workspace.get_thoughts(),
            workspace.get_predictions()
        );
        assert!(
            workspace
                .get_thoughts()
                .iter()
                .any(|t| t.contains("investigation required")),
            "executive narrated the investigation: {:?}",
            workspace.get_thoughts()
        );
    }

    #[tokio::test]
    async fn goal_branch_investigates_the_real_goal_description() {
        let workspace = CognitiveBlackboard::new();
        let mut executive = Executive::new();
        // `add_goal` never sets the active goal — the Goal branch must resolve
        // the real description from the attention item, not the item's uuid.
        executive.add_goal("Search the web for the latest Rust async docs", 0.9);
        let _report = executive.cycle(&workspace).await;

        let plan = workspace.get_active_plan().expect("a plan was produced");
        assert_eq!(
            plan.source,
            PlanSource::Investigation,
            "goal branch must investigate the real description, not the attention id: {}",
            plan.strategy
        );
        assert!(
            workspace
                .get_thoughts()
                .iter()
                .any(|t| t.contains("investigation required")),
            "{:?}",
            workspace.get_thoughts()
        );
    }

    #[tokio::test]
    async fn investigate_phase_is_recorded_in_the_audit_trail_for_goals() {
        // The evidence gate is its own cycle phase: a goal that requires
        // evidence records `Investigate` in phases_completed, before Plan.
        let workspace = CognitiveBlackboard::new();
        let mut executive = Executive::new();
        executive.add_observation("Search the web for the latest Rust async docs", 0.9);
        let report = executive.cycle(&workspace).await;

        assert!(
            report
                .phases_completed
                .contains(&ExecutivePhase::Investigate),
            "evidence goal must record the Investigate phase: {:?}",
            report.phases_completed
        );
        let inv_idx = report
            .phases_completed
            .iter()
            .position(|p| *p == ExecutivePhase::Investigate)
            .unwrap();
        let plan_idx = report
            .phases_completed
            .iter()
            .position(|p| *p == ExecutivePhase::Plan)
            .unwrap();
        assert!(inv_idx < plan_idx, "investigate precedes plan");

        // The cognitive trace carries the Investigation node for the gate.
        let trace = workspace.get_current_trace().expect("trace was recorded");
        assert!(
            trace
                .nodes
                .iter()
                .any(|n| n.kind == TraceNodeKind::Investigation),
            "trace must carry the Investigation node"
        );
    }

    #[tokio::test]
    async fn workspace_records_the_investigation_snapshot_for_evidence_goals() {
        // The evidence gate is published to the workspace: the same plan that
        // gated planning is what the bridge renders as the live graph.
        let workspace = CognitiveBlackboard::new();
        let mut executive = Executive::new();
        executive.add_goal("Search the web for the latest Rust async docs", 0.9);
        let _report = executive.cycle(&workspace).await;

        let snap = workspace
            .get_investigation()
            .expect("the executive must record its investigation decision");
        assert_eq!(snap.goal, "Search the web for the latest Rust async docs");
        assert!(snap.plan.needs_investigation(), "web + docs required");
        let channels: Vec<&str> = snap.plan.channels().iter().map(|c| c.label()).collect();
        assert!(channels.contains(&"web"), "web branch: {channels:?}");
    }

    #[tokio::test]
    async fn workspace_records_an_empty_plan_for_self_contained_goals() {
        // A self-contained goal still records its gate: empty plan, so the
        // rendered graph is `plan → execute` (no investigate phase).
        let workspace = CognitiveBlackboard::new();
        let mut executive = Executive::new();
        executive.add_goal("Open notepad and start writing a note", 0.9);
        let _report = executive.cycle(&workspace).await;

        let snap = workspace
            .get_investigation()
            .expect("the executive must record its investigation decision");
        assert_eq!(snap.goal, "Open notepad and start writing a note");
        assert!(
            !snap.plan.needs_investigation(),
            "self-contained goal has no evidence branches"
        );
    }

    #[tokio::test]
    async fn investigate_phase_is_absent_for_idle_cycles() {
        // An idle cycle has nothing to investigate — the phase must not be
        // recorded (the audit trail never claims a gate that did not fire).
        let workspace = CognitiveBlackboard::new();
        let mut executive = Executive::new();
        let report = executive.cycle(&workspace).await;

        assert!(
            !report
                .phases_completed
                .contains(&ExecutivePhase::Investigate),
            "idle cycles must not record the Investigate phase: {:?}",
            report.phases_completed
        );
        // The cycle's early return must still land on Idle — the internal
        // phase must never be left claiming "plan" after an idle cycle.
        assert_eq!(
            executive.phase(),
            ExecutivePhase::Idle,
            "idle cycle must end in the Idle phase"
        );
    }

    #[tokio::test]
    async fn executive_skips_investigation_for_self_contained_goals() {
        let workspace = CognitiveBlackboard::new();
        let mut executive = Executive::new();
        executive.add_observation("Open notepad", 0.9);
        let _report = executive.cycle(&workspace).await;

        let plan = workspace.get_active_plan().expect("a plan was produced");
        assert_ne!(plan.source, PlanSource::Investigation);
        assert!(
            workspace
                .get_thoughts()
                .iter()
                .any(|t| t.contains("no investigation required")),
            "{:?}",
            workspace.get_thoughts()
        );
    }

    #[tokio::test]
    async fn cognitive_trace_surfaces_investigation_and_planning_decision() {
        let workspace = CognitiveBlackboard::new();
        let mut executive = Executive::new();
        executive.register_tool(Box::new(
            MockTool::success("web", "search results").with_capabilities(vec![
                Capability::SearchWeb,
                Capability::Custom("web.search".into()),
            ]),
        ));
        executive.register_tool(Box::new(
            MockTool::success("docs", "docs content").with_capabilities(vec![
                Capability::ReadFile,
                Capability::Custom("docs.search".into()),
            ]),
        ));
        executive.register_tool(Box::new(
            MockTool::success("terminal", "ok").with_capabilities(vec![Capability::ExecuteCommand]),
        ));
        workspace.set_world_state(
            WorldState::new()
                .with_tool(Capability::SearchWeb)
                .with_tool(Capability::ReadFile)
                .with_tool(Capability::ExecuteCommand)
                .with_tool(Capability::Custom("web.search".into()))
                .with_tool(Capability::Custom("docs.search".into())),
        );

        executive.add_observation("Search the web for the latest Rust async docs", 0.9);
        let _report = executive.cycle(&workspace).await;

        let trace = workspace.get_current_trace().expect("trace was recorded");
        let kinds: Vec<TraceNodeKind> = trace.nodes.iter().map(|n| n.kind.clone()).collect();
        assert!(kinds.contains(&TraceNodeKind::Investigation), "{kinds:?}");
        assert!(kinds.contains(&TraceNodeKind::Planning), "{kinds:?}");

        // Investigation precedes execution in the trace.
        let inv_idx = trace
            .nodes
            .iter()
            .position(|n| n.kind == TraceNodeKind::Investigation)
            .expect("investigation node");
        let exec_idx = trace
            .nodes
            .iter()
            .position(|n| n.kind == TraceNodeKind::Execution)
            .expect("execution node");
        assert!(inv_idx < exec_idx, "investigation precedes execution");

        // The investigation node records the required channels (strongest
        // first): "Search the web for the latest Rust async docs" fires web
        // and docs heuristically.
        let inv = &trace.nodes[inv_idx];
        assert_eq!(inv.output["requirement_count"], serde_json::json!(2));
        assert_eq!(inv.output["channels"][0], serde_json::json!("web"));
        assert_eq!(inv.output["channels"][1], serde_json::json!("docs"));

        // The planning node records the chosen source.
        let plan_idx = trace
            .nodes
            .iter()
            .position(|n| n.kind == TraceNodeKind::Planning)
            .expect("planning node");
        assert_eq!(
            trace.nodes[plan_idx].output["source"],
            serde_json::json!("Investigation")
        );
    }

    /// The LLM-backed strategy is a drop-in for the heuristic: the executive
    /// runs the model-derived requirements through the same plan → simulate →
    /// execute path, and the model-only channel reaches the plan.
    #[tokio::test]
    async fn llm_strategy_is_a_drop_in_and_model_channels_reach_the_plan() {
        use async_trait::async_trait;
        use friday_investigation::{InvestigationEngine, LlmStrategyClient};

        struct ScriptedLlmClient;
        #[async_trait]
        impl LlmStrategyClient for ScriptedLlmClient {
            async fn complete(&self, _prompt: &str) -> Result<String, String> {
                // `memory` would NOT fire for this goal heuristically — its
                // presence in the plan proves the model's judgment was used.
                Ok(r#"{"requirements":[
                    {"channel":"web","rationale":"search the web","confidence":1.0},
                    {"channel":"memory","rationale":"recall prior research","confidence":0.9}
                ]}"#
                .to_string())
            }
        }

        let workspace = CognitiveBlackboard::new();
        let mut executive = Executive::new()
            .with_investigation_engine(InvestigationEngine::llm(Box::new(ScriptedLlmClient)));
        executive.register_tool(Box::new(
            MockTool::success("web", "search results")
                .with_capabilities(vec![Capability::SearchWeb]),
        ));
        executive.register_tool(Box::new(
            MockTool::success("memory", "prior research").with_capabilities(vec![
                Capability::SearchMemory,
                Capability::Custom("memory.recall".into()),
            ]),
        ));
        // infer_capability classifies the goal's own action ("Rust" → the
        // legacy "run" heuristic) as execute_command — cover it for feasibility.
        executive.register_tool(Box::new(
            MockTool::success("terminal", "ok").with_capabilities(vec![Capability::ExecuteCommand]),
        ));
        workspace.set_world_state(
            WorldState::new()
                .with_tool(Capability::SearchWeb)
                .with_tool(Capability::SearchMemory)
                .with_tool(Capability::ExecuteCommand)
                .with_tool(Capability::Custom("web.search".into()))
                .with_tool(Capability::Custom("memory.recall".into())),
        );

        executive.add_observation("Search the web for the latest Rust async docs", 0.9);
        let report = executive.cycle(&workspace).await;

        let plan = workspace.get_active_plan().expect("a plan was produced");
        assert_eq!(
            plan.source,
            PlanSource::Investigation,
            "model-derived plan should win: {}",
            plan.strategy
        );
        assert!(
            plan.actions[0].instruction.contains("web.search"),
            "strongest evidence first: {}",
            plan.actions[0].instruction
        );
        assert!(
            plan.actions
                .iter()
                .any(|a| a.instruction.contains("memory.recall")),
            "model-only channel reaches the plan: {:?}",
            plan.actions
                .iter()
                .map(|a| &a.instruction)
                .collect::<Vec<_>>()
        );
        assert!(
            report.plan_executed,
            "the evidence action executed — thoughts: {:?}",
            workspace.get_thoughts()
        );
    }
}
