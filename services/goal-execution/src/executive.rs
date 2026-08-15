use serde::{Deserialize, Serialize};

use crate::evidence::{is_evidence_action, parse_evidence_id, EvidenceActionRecord, EvidenceTrail};
use crate::evidence_pipeline::{evidence_episode, EvidenceBody, EvidenceSet, EvidenceVerifier};
use crate::simulation::{DefaultSimulator, Simulator};
use crate::Action;
use crate::AdmissionSignal;
use crate::AdmissionVerdict;
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
use crate::GoalReport;
use crate::GoalResult;
use crate::MemoryAdmissionController;
use crate::Plan;
use crate::PlanEvaluator;
use crate::PlanSource;
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
use crate::WorldState;
use friday_investigation::{InvestigationEngine, InvestigationResult};
use friday_knowledge::KnowledgeRecord;
use friday_reflection::ReflectionEngine;
use std::sync::atomic::{AtomicBool, Ordering};
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

/// Why a cycle stopped before reaching its natural end (S3 human control).
/// `None` = the cycle runs to completion. The runner holds the executive's
/// lock across the whole cycle, so a control request is never a dropped
/// future — it is a cooperative flag the cycle checks at phase/action
/// boundaries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ControlStop {
    /// A human cancelled the running goal (`task.cancel` → `goal.cancelled`).
    Cancelled,
    /// A human paused the running goal (`task.pause` → `goal.paused`).
    Paused,
}

impl std::fmt::Display for ControlStop {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Cancelled => write!(f, "cancelled"),
            Self::Paused => write!(f, "paused"),
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
    /// The memory admission gate (Phase 1b): what the Consolidate phase decided
    /// for the episode — Store / Reject(reason) / Pending(reason). None when
    /// the cycle never reached consolidation.
    pub admission_verdict: Option<AdmissionVerdict>,
    /// The five admission signals (importance/novelty/reliability/sensitivity/
    /// evidence) that produced the verdict — the *why*, recorded for audit.
    pub admission_signal: Option<AdmissionSignal>,
    /// The evidence trail of the executed plan (M7): one Prepared → Observed →
    /// Verified | Failed record per evidence action. Empty for plans with no
    /// evidence actions.
    pub evidence_trail: EvidenceTrail,
    /// The blended, verified research body of an evidence-gated plan
    /// (MasterPlan #6): every evidence action's normalized output merged into
    /// one `Evidence[]` and verified behind the `Verifier` seam. `None` for
    /// plans with no evidence actions or nothing parseable.
    pub evidence: Option<EvidenceBody>,
    /// The goal's canonical result — the actual output the executed plan
    /// produced (Text/Tool/…), or an explicit Empty state when the execution
    /// produced nothing. Sourced from the raw tool output in the Execute
    /// phase, never reconstructed from phase/evidence metadata. `None` only
    /// when the cycle never executed a plan (those goals publish `failed`).
    pub result: Option<GoalResult>,
    /// The cycle stopped before its natural end because a human cancelled the
    /// running goal ([`ControlStop::Cancelled`]). A cancelled cycle records no
    /// terminal episode — the runner publishes `goal.cancelled`.
    pub cancelled: bool,
    /// The cycle stopped because a human paused the running goal
    /// ([`ControlStop::Paused`]). Same cooperative stop as cancellation; the
    /// runner publishes `goal.paused`.
    pub paused: bool,
    /// The id of an episode the Consolidate phase held for human confirmation
    /// (admission `Pending`). This is the handle a `task.approve` command
    /// targets — `None` when nothing was held.
    pub held_episode_id: Option<String>,
    /// The goal's own action recovered (S5): it failed once or more, the
    /// Executive re-observed reality, and a bounded retry then succeeded.
    /// `false` for a first-try success, a no-recovery plan, or an exhausted
    /// recovery.
    pub recovered: bool,
    /// Total execute attempts of the goal's own action (1 = no recovery ran).
    pub recovery_attempts: u32,
    /// The recovery bound was exhausted: the goal action failed every bounded
    /// attempt and the goal is a genuine, retryable failure (S5-B). `false`
    /// for structural failures (no tool resolved), successes, and plans with a
    /// zero retry budget.
    pub recoverable: bool,
    /// Why the goal action ultimately failed, when [`Self::recoverable`] — the
    /// final tool error (never a reconstruction).
    pub failed_reason: Option<String>,
    /// The last world state FRIDAY observed before giving up (S5-B) — the
    /// reality a follow-up goal can re-enter from instead of starting cold.
    pub last_observed_state: Option<WorldState>,
}

/// Record a control stop on a report: which stop fired and the elapsed time.
/// Shared by the cycle's control checkpoints so a cancelled/paused cycle
/// returns the same well-formed shape from every phase.
fn mark_stopped(report: &mut ExecutiveReport, started_ms: u64, stop: ControlStop) {
    report.cancelled = stop == ControlStop::Cancelled;
    report.paused = stop == ControlStop::Paused;
    report.duration_ms = chrono::Utc::now().timestamp_millis() as u64 - started_ms;
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
    /// Evidence verification (MasterPlan #6): the concrete `EvidenceVerifier`
    /// that gates the blended research body behind the frozen `Verifier` seam.
    evidence_verification: EvidenceVerifier,
    reflection_engine: ReflectionEngine,
    semantic_memory: SemanticMemory,
    episode_store: EpisodeStore,
    /// The gate between experience and episodic memory (Phase 1b): every
    /// episode must clear importance/novelty/reliability/evidence gates to be
    /// stored; sensitive episodes are held for human confirmation.
    admission: MemoryAdmissionController,
    attention_manager: AttentionManager,
    /// Tick counter.
    cycle_count: u64,
    /// Human-control flags (S3): the runner flips these on `task.cancel` /
    /// `task.pause`, and the cycle checks them cooperatively at phase/action
    /// boundaries. Both are `Arc<AtomicBool>` so the runner holds its own
    /// clones while the cycle awaits inside the executive lock.
    abort: Arc<AtomicBool>,
    pause: Arc<AtomicBool>,
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

        // Human control: a cancel/pause that arrived during investigation stops
        // the cycle before anything is planned or executed — no plan, no
        // simulated future, no action. The report records the stop and the
        // phases actually completed (never a terminal episode).
        if let Some(stop) = self.control_stop() {
            self.phase = ExecutivePhase::Idle;
            workspace.clear_execution();
            mark_stopped(&mut report, started_ms, stop);
            return report;
        }

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

        // A fresh cycle never inherits a previous cycle's evidence trail: only
        // the evidence-gated path below publishes one, so a direct-plan cycle
        // (or a self-contained goal) leaves the workspace slot empty.
        workspace.clear_evidence_trail();

        let goal_report = if let (Some(g), Some(ref p)) = (Some(&goal), &plan) {
            if let Some(sim) = &simulation_result {
                if !sim.feasible {
                    workspace.push_thought("executive: skipping execution — simulation infeasible");
                    None
                } else if p.source == PlanSource::Investigation {
                    // M7 evidence-gated plan: EVERY evidence action executes
                    // through the evidence lifecycle — each carries its own
                    // Prepared → Observed → Verified | Failed trail — then the
                    // goal's own action executes as the main result. The trail
                    // is published to the report and the workspace.
                    let mut evidence_trail = EvidenceTrail {
                        goal: g.description.clone(),
                        records: Vec::new(),
                    };
                    let mut merged_nodes: Vec<TraceNode> = Vec::new();
                    if let Some(t) = &planning_trace {
                        merged_nodes.extend(t.nodes.iter().cloned());
                    }
                    let mut main_report: Option<GoalReport> = None;
                    // Raw outputs of every evidence action (MasterPlan #6):
                    // blended into one `Evidence[]` and verified below.
                    let mut evidence_outputs: Vec<String> = Vec::new();

                    for (step, action) in p.actions.iter().enumerate() {
                        // Human control checkpoint: a cancel/pause that landed
                        // during a previous action stops the run HERE — no
                        // further evidence or goal actions execute.
                        if let Some(stop) = self.control_stop() {
                            workspace.push_thought(format!(
                                "executive: {stop} by human — skipping remaining actions"
                            ));
                            break;
                        }
                        workspace
                            .push_thought(format!("executive: executing: {}", action.instruction));
                        if is_evidence_action(action) {
                            let expectation = Expectation::success(&action.instruction);
                            let exec_ctx = ExecutionContext::for_goal(&g.id);
                            let gr = self
                                .goal_engine
                                .execute(g, action, &expectation, &exec_ctx)
                                .await;
                            merged_nodes.extend(gr.cognitive_trace.nodes.iter().cloned());
                            // An evidence action — record its own lifecycle.
                            // is_evidence_action guarantees the parse succeeds.
                            let (channel, _verb) = parse_evidence_id(&action.instruction)
                                .expect("evidence action carries a parseable id");
                            evidence_trail.records.push(EvidenceActionRecord {
                                step,
                                instruction: action.instruction.clone(),
                                capability: action.capability.to_string(),
                                channel,
                                execution_state: gr.execution_state,
                                verified: gr.verification.verified,
                                tool_success: gr.verification.tool_success,
                                duration_ms: gr.trace.duration_ms(),
                            });
                            // Keep the observed output for the blended body —
                            // only a genuinely succeeded tool's raw output;
                            // `execution_output()` would fall back to the goal
                            // description for a failed tool, and the goal text
                            // is not evidence.
                            if gr.verification.tool_success {
                                if let Some(tr) = &gr.execution_result {
                                    evidence_outputs.push(tr.output.clone());
                                }
                            }
                        } else {
                            // The goal's own action — the main execution result,
                            // running through the same bounded in-cycle recovery
                            // seam as a direct plan (S5).
                            let (main_gr, main_nodes) = self
                                .execute_action_with_recovery(
                                    g,
                                    action,
                                    p.max_retries,
                                    workspace,
                                    &mut report,
                                )
                                .await;
                            merged_nodes.extend(main_nodes);
                            main_report = Some(main_gr);
                        }
                    }

                    report.evidence_trail = evidence_trail.clone();
                    workspace.set_evidence_trail(evidence_trail);

                    // Evidence pipeline (MasterPlan #6): blend every evidence
                    // action's observed output into one research body and
                    // verify it behind the frozen `Verifier` seam — AnyDoc +
                    // Web → Evidence[] → Verification. The verified body
                    // rides the episode into episodic memory (Consolidate).
                    if !evidence_outputs.is_empty() {
                        let set = EvidenceSet::from_outputs(&g.description, &evidence_outputs);
                        if !set.is_empty() {
                            let action = Action::single(
                                "verify research evidence",
                                Capability::Custom("evidence.verify".into()),
                                &g.id,
                            );
                            let body = serde_json::to_string(&set.items)
                                .unwrap_or_else(|_| "[]".to_string());
                            let result = ToolResult::success(body);
                            let verified = self.evidence_verification.verify(&action, &result);
                            report.evidence = Some(EvidenceBody { set, verified });
                            workspace.push_thought(format!(
                                "executive: blended {} evidence items across {} (verified={})",
                                report.evidence.as_ref().map(|b| b.set.len()).unwrap_or(0),
                                report
                                    .evidence
                                    .as_ref()
                                    .map(|b| b.set.channels().join(", "))
                                    .unwrap_or_default(),
                                report
                                    .evidence
                                    .as_ref()
                                    .map(|b| b.verified.verified)
                                    .unwrap_or(false)
                            ));
                        }
                    }

                    if let Some(gr) = &main_report {
                        // The canonical result is the raw output the tool
                        // produced (Observed) — `execution_result` — never the
                        // goal description or a reconstruction from metadata.
                        // The captured result is present on every real run; the
                        // fallback is purely defensive for hand-built reports.
                        let tool_result =
                            gr.execution_result.clone().unwrap_or_else(|| ToolResult {
                                success: gr.success(),
                                output: gr.execution_output(),
                                error: if gr.success() {
                                    None
                                } else {
                                    Some(gr.outcome.context.clone())
                                },
                                duration_ms: gr.trace.duration_ms(),
                                metadata: serde_json::json!({}),
                            });
                        workspace.set_execution(tool_result.clone());
                        report.result = Some(
                            GoalResult::from_tool_result(&tool_result).with_tool(gr.tool.clone()),
                        );
                        // Full trail top-down: investigation → chosen plan →
                        // every evidence action's lifecycle → the goal action.
                        let mut trace = CognitiveTrace::new(&g.id);
                        trace.nodes = merged_nodes;
                        workspace.set_current_trace(trace);

                        workspace.push_thought(format!(
                            "executive: verified (verified={}, confidence={:.2})",
                            gr.verification.verified,
                            gr.verification.confidence.value()
                        ));
                        report.plan_executed = true;
                    }
                    main_report
                } else if let Some(action) = p.actions.first() {
                    workspace.push_thought(format!("executive: executing: {}", action.instruction));

                    let (gr, exec_nodes) = self
                        .execute_action_with_recovery(
                            g,
                            action,
                            p.max_retries,
                            workspace,
                            &mut report,
                        )
                        .await;

                    let tool_result = gr.execution_result.clone().unwrap_or_else(|| ToolResult {
                        success: gr.success(),
                        output: gr.execution_output(),
                        error: if gr.success() {
                            None
                        } else {
                            Some(gr.outcome.context.clone())
                        },
                        duration_ms: gr.trace.duration_ms(),
                        metadata: serde_json::json!({}),
                    });
                    workspace.set_execution(tool_result.clone());
                    report.result =
                        Some(GoalResult::from_tool_result(&tool_result).with_tool(gr.tool.clone()));
                    // Surface the full decision trail top-down: investigation →
                    // chosen plan (from this cycle) followed by the GoalEngine's
                    // execution → verification → reflection nodes.
                    let mut trace = planning_trace
                        .clone()
                        .unwrap_or_else(|| CognitiveTrace::new(&g.id));
                    trace.nodes.extend(exec_nodes);
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

        // Human control: a cancel/pause that arrived during execution stops the
        // cycle here — the report records exactly what ran before the stop
        // (partial evidence trail, phases), and the cycle skips verification,
        // reflection, consolidation and learning. A cancelled goal produces no
        // terminal episode; the runner publishes `goal.cancelled`/`goal.paused`.
        if let Some(stop) = self.control_stop() {
            self.phase = ExecutivePhase::Idle;
            workspace.clear_execution();
            mark_stopped(&mut report, started_ms, stop);
            return report;
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
        // MasterPlan #6: an evidence-gated plan with a blended research body
        // consolidates the EVIDENCE — its execution result is the blended
        // Evidence[], its verification is the evidence verification, tagged
        // `research`. The admission gate below still decides what enters
        // episodic memory. Plans without evidence keep the generic episode.
        let episode = match &report.evidence {
            Some(body) if !body.set.is_empty() => evidence_episode(goal, &body.set, &body.verified)
                // Preserve the same surfaces the generic path carries — the
                // plan and reflection are part of the episodic record and feed
                // the admission sensitivity scan.
                .with_plan(plan.clone().unwrap_or(Plan::from_action(
                    Action::single("no-op", Capability::ExecuteCommand, "default"),
                    "fallback",
                )))
                .with_reflection(workspace.get_reflection().unwrap_or_default())
                .with_world_before(world_before)
                .with_world_after(world_after),
            _ => {
                let ver = goal_report.as_ref().map(|gr| &gr.verification);
                CognitiveEpisode::begin(goal)
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
                    })
            }
        };

        // Phase 1b admission gate: nothing enters episodic memory without
        // clearing importance / novelty / reliability / evidence (and no
        // sensitive episode is ever silently stored — it is held for a human).
        let (verdict, signal) = self.admission.admit(&episode, &self.episode_store);
        report.admission_verdict = Some(verdict.clone());
        report.admission_signal = Some(signal);
        match verdict {
            AdmissionVerdict::Store => {
                self.episode_store.store(episode.clone());
                workspace.set_current_episode(episode);
                report.episode_stored = true;
                workspace.push_thought("executive: episode admitted to episodic memory");
            }
            AdmissionVerdict::Reject(reason) => {
                report.episode_stored = false;
                workspace.push_thought(format!("executive: episode rejected — {reason}"));
            }
            AdmissionVerdict::Pending(reason) => {
                let held_id = self.admission.hold(episode, reason);
                // Surface the held episode's handle so the runner can offer a
                // `task.approve` path for it (S3).
                report.held_episode_id = Some(held_id);
                report.episode_stored = false;
                workspace.push_thought(format!(
                    "executive: episode held for human confirmation — {reason}"
                ));
            }
        }

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

    pub fn episode_store_mut(&mut self) -> &mut EpisodeStore {
        &mut self.episode_store
    }

    pub fn admission(&self) -> &MemoryAdmissionController {
        &self.admission
    }

    pub fn admission_mut(&mut self) -> &mut MemoryAdmissionController {
        &mut self.admission
    }

    /// A clone of the cancellation flag — the runner flips it on `task.cancel`.
    /// The cycle checks it at phase/action boundaries and stops cooperatively.
    pub fn abort_token(&self) -> Arc<AtomicBool> {
        self.abort.clone()
    }

    /// A clone of the pause flag — the runner flips it on `task.pause`.
    pub fn pause_token(&self) -> Arc<AtomicBool> {
        self.pause.clone()
    }

    /// Cancel the current cycle: the next phase/action boundary stops it and
    /// the report records `cancelled = true`. Safe to call while the cycle is
    /// not running (the next cycle picks it up — the runner clears both flags
    /// when a goal starts).
    pub fn cancel(&self) {
        self.abort.store(true, Ordering::SeqCst);
    }

    /// Pause the current cycle: same cooperative stop as [`Self::cancel`],
    /// reported as `paused = true`.
    pub fn pause(&self) {
        self.pause.store(true, Ordering::SeqCst);
    }

    /// Clear the pause flag so a paused cycle may run again.
    pub fn resume(&self) {
        self.pause.store(false, Ordering::SeqCst);
    }

    /// Whether a human control stop is pending at this checkpoint.
    fn control_stop(&self) -> Option<ControlStop> {
        if self.abort.load(Ordering::SeqCst) {
            Some(ControlStop::Cancelled)
        } else if self.pause.load(Ordering::SeqCst) {
            Some(ControlStop::Paused)
        } else {
            None
        }
    }

    /// Human approves a held episode (admission `Pending`) into episodic
    /// memory. `Err` when no episode is held under that id — the runner
    /// publishes nothing for a miss.
    pub fn approve_pending(&mut self, id: &str) -> Result<(), String> {
        self.admission.approve(id, &mut self.episode_store)
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

    /// In-cycle recovery seam (S5): execute the goal's own action, and when it
    /// fails unexpectedly (tool or verification), **re-observe reality**, then
    /// re-execute and re-verify — up to the plan's `max_retries` bound. Every
    /// attempt's execution → verification → reflection nodes stay in the audit
    /// trail, joined by a [`TraceNodeKind::Recovery`] re-observation per retry.
    /// The report records how many attempts ran, whether the goal recovered,
    /// and — when the bound was exhausted — that it is `recoverable`, why it
    /// failed, and the last observed world state.
    ///
    /// Structural failures are never retried: when no tool resolved for the
    /// capability the registry decision is final. A human control stop landing
    /// mid-recovery halts further retries so the cycle can stop cooperatively.
    async fn execute_action_with_recovery(
        &self,
        g: &Goal,
        action: &Action,
        max_retries: u32,
        workspace: &CognitiveBlackboard,
        report: &mut ExecutiveReport,
    ) -> (GoalReport, Vec<TraceNode>) {
        let expectation = Expectation::success(&action.instruction);
        let exec_ctx = ExecutionContext::for_goal(&g.id);
        let mut nodes: Vec<TraceNode> = Vec::new();
        let mut attempt: u32 = 0;
        let current: Option<GoalReport>;
        let mut stopped = false;

        loop {
            attempt += 1;
            let gr = self
                .goal_engine
                .execute(g, action, &expectation, &exec_ctx)
                .await;
            nodes.extend(gr.cognitive_trace.nodes.iter().cloned());
            report.recovery_attempts = attempt;

            let structural = gr.tool.is_empty();
            if structural || gr.success() || attempt > max_retries {
                current = Some(gr);
                break;
            }

            // About to retry: a cancel/pause that landed during the failed
            // attempt halts further retries — no re-observe, no re-execute.
            // The post-Execute checkpoint reports the cooperative stop.
            if self.control_stop().is_some() {
                stopped = true;
                current = Some(gr);
                break;
            }

            // Re-observe reality before the next attempt: snapshot the current
            // world state and record the recovery decision in the audit trail.
            let observed = workspace.get_world_state();
            report.last_observed_state = observed.clone();
            workspace.push_thought(format!(
                "executive: recovery — re-observing before retry {attempt}/{max_retries}"
            ));
            nodes.push(
                TraceNode::new(
                    TraceNodeKind::Recovery,
                    serde_json::json!({
                        "attempt": attempt,
                        "outcome_kind": format!("{:?}", gr.outcome.kind),
                        "tool_success": gr.verification.tool_success,
                        "verified": gr.verification.verified,
                        "error": gr
                            .execution_result
                            .as_ref()
                            .and_then(|r| r.error.clone())
                            .unwrap_or_else(|| gr.outcome.context.clone()),
                    }),
                    serde_json::json!({
                        "decision": "retry",
                        "observed_state": observed,
                    }),
                )
                .with_world_state(workspace.get_world_state().unwrap_or_default()),
            );
        }

        let gr = current.expect("one attempt always ran");
        report.recovered = report.recovery_attempts > 1 && gr.success();
        report.recoverable = !stopped && !gr.tool.is_empty() && !gr.success() && max_retries > 0;
        if report.recoverable {
            report.failed_reason = Some(
                gr.execution_result
                    .as_ref()
                    .and_then(|r| r.error.clone())
                    .unwrap_or_else(|| gr.outcome.context.clone()),
            );
            // The last observed state at exhaustion — the reality FRIDAY saw
            // before giving up.
            report.last_observed_state = workspace.get_world_state();
        }
        (gr, nodes)
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

    /// Attach a durable memory journal. The journal's history is replayed into
    /// the episode store at startup and every admitted episode is persisted to
    /// it, so memory survives process restarts (S2 memory write + recall).
    pub fn with_episode_journal(mut self, journal: Arc<crate::journal::MemoryJournal>) -> Self {
        self.episode_store = EpisodeStore::with_journal(journal);
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
            evidence_verification: EvidenceVerifier::new(),
            reflection_engine: ReflectionEngine::new(),
            semantic_memory: SemanticMemory::new(),
            episode_store: EpisodeStore::new(),
            admission: MemoryAdmissionController::new(),
            attention_manager: AttentionManager::new(),
            cycle_count: 0,
            abort: Arc::new(AtomicBool::new(false)),
            pause: Arc::new(AtomicBool::new(false)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        AdmissionReason, ExecutionState, GoalResultKind, MockTool, PlanSource, WorldState,
    };
    use async_trait::async_trait;
    use std::sync::Mutex;
    use tokio::sync::{oneshot, watch};

    /// A test tool whose execution parks until the test releases the gate, so a
    /// test can deterministically land a cancel WHILE the action is "running" —
    /// proving the cooperative checkpoint stops the cycle mid-execution.
    struct GatedTool {
        capabilities: Vec<Capability>,
        began: Mutex<Option<oneshot::Sender<()>>>,
        gate_rx: watch::Receiver<bool>,
    }

    #[async_trait]
    impl Tool for GatedTool {
        fn name(&self) -> &str {
            "gated"
        }
        fn description(&self) -> &str {
            "test gate tool"
        }
        fn capabilities(&self) -> Vec<Capability> {
            self.capabilities.clone()
        }
        async fn execute(&self, _action: &Action, _context: &ExecutionContext) -> ToolResult {
            if let Some(sender) = self.began.lock().unwrap().take() {
                let _ = sender.send(());
            }
            if !*self.gate_rx.borrow() {
                let mut rx = self.gate_rx.clone();
                let _ = rx.changed().await;
            }
            ToolResult::success("gated output")
        }
    }

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

    #[tokio::test]
    async fn evidence_plan_runs_every_evidence_action_with_its_own_trail() {
        // A two-channel evidence plan (web + docs) must execute BOTH evidence
        // actions — each carrying its own Prepared → Observed → Verified trail
        // — and then the goal's own action.
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
        let report = executive.cycle(&workspace).await;

        // Every evidence action ran with its own verified trail.
        let trail = &report.evidence_trail;
        assert_eq!(
            trail.records.len(),
            2,
            "both evidence channels ran: {trail:?}"
        );
        assert!(trail.all_verified(), "both verified: {trail:?}");
        let channels: Vec<&str> = trail.records.iter().map(|r| r.channel.as_str()).collect();
        assert!(channels.contains(&"web"), "channels: {channels:?}");
        assert!(channels.contains(&"docs"), "channels: {channels:?}");
        assert!(
            trail
                .records
                .iter()
                .all(|r| r.execution_state == ExecutionState::Verified),
            "every evidence action verified: {trail:?}"
        );

        // The trail is published to the workspace (bridge observability).
        let published = workspace.get_evidence_trail().expect("published trail");
        assert_eq!(published.records.len(), 2);
        assert_eq!(
            published.goal,
            "Search the web for the latest Rust async docs"
        );

        // The goal's own action still executed.
        assert!(report.plan_executed);

        // The merged trace carries BOTH evidence actions' lifecycle nodes
        // (execution → verification per evidence action), not just the first.
        let trace = workspace.get_current_trace().expect("trace");
        let evidence_execs: Vec<_> = trace
            .nodes
            .iter()
            .filter(|n| {
                n.kind == TraceNodeKind::Execution
                    && n.input["instruction"]
                        .as_str()
                        .map(|s| s.starts_with("gather evidence"))
                        .unwrap_or(false)
            })
            .collect();
        assert_eq!(evidence_execs.len(), 2, "two evidence executions in trace");
        // Both evidence actions ended verified in their own trace nodes.
        for node in evidence_execs {
            assert_eq!(node.output["state"], serde_json::json!("observed"));
        }
        let evidence_verifies: Vec<_> = trace
            .nodes
            .iter()
            .filter(|n| n.kind == TraceNodeKind::Verification)
            .collect();
        assert!(
            evidence_verifies.len() >= 2,
            "verification per evidence action"
        );
    }

    #[tokio::test]
    async fn failed_evidence_action_is_recorded_as_failed_not_verified() {
        // One evidence channel fails: its trail record must say Failed (the
        // evidence lifecycle never claims verified on a failed tool), while the
        // healthy channel and the goal action still run.
        let workspace = CognitiveBlackboard::new();
        let mut executive = Executive::new();
        executive.register_tool(Box::new(
            MockTool::failure("web", "search engine unreachable").with_capabilities(vec![
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
        let report = executive.cycle(&workspace).await;

        let trail = &report.evidence_trail;
        assert_eq!(trail.records.len(), 2);
        assert!(!trail.all_verified(), "web failed: {trail:?}");
        assert_eq!(trail.verified_count(), 1, "only docs verified: {trail:?}");
        let web = trail
            .records
            .iter()
            .find(|r| r.channel == "web")
            .expect("web record");
        assert_eq!(web.execution_state, ExecutionState::Failed);
        assert!(!web.verified);
        assert!(!web.tool_success);
        let docs = trail
            .records
            .iter()
            .find(|r| r.channel == "docs")
            .expect("docs record");
        assert_eq!(docs.execution_state, ExecutionState::Verified);
    }

    #[tokio::test]
    async fn direct_plan_has_empty_evidence_trail() {
        // A goal with no evidence requirements takes the direct plan — no
        // evidence actions, so the trail must be empty (never a phantom
        // "no channels verified" trail).
        let workspace = CognitiveBlackboard::new();
        let mut executive = Executive::new();
        executive.add_observation("Open notepad and write a note", 0.9);
        let report = executive.cycle(&workspace).await;

        assert!(report.evidence_trail.is_empty());
        assert!(workspace.get_evidence_trail().is_none());
    }

    #[tokio::test]
    async fn direct_plan_after_evidence_plan_clears_the_stale_trail() {
        // A direct-plan cycle must not render the *previous* goal's evidence
        // trail: the workspace slot is cleared at the start of every Execute
        // phase and only republished by the evidence-gated path.
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

        // Cycle 1: evidence-gated (web + docs fire) → trail published.
        executive.add_observation("Search the web for the latest Rust async docs", 0.9);
        let _r1 = executive.cycle(&workspace).await;
        assert!(
            workspace.get_evidence_trail().is_some(),
            "evidence cycle publishes a trail"
        );

        // Cycle 2: self-contained goal → the stale trail must be gone.
        executive.add_observation("Open notepad", 0.9);
        let r2 = executive.cycle(&workspace).await;
        assert!(r2.evidence_trail.is_empty());
        assert!(
            workspace.get_evidence_trail().is_none(),
            "direct cycle must not render the previous goal's evidence trail"
        );
    }

    #[tokio::test]
    async fn sensitive_episode_is_held_pending_not_stored() {
        // Phase 1b admission gate: an episode whose goal carries sensitive
        // content never enters episodic memory — it is held on the controller
        // for a human to approve or reject.
        let workspace = CognitiveBlackboard::new();
        let mut exec = Executive::new();
        exec.add_goal("rotate my api_key and reset the password", 0.9);
        let report = exec.cycle(&workspace).await;

        assert_eq!(
            report.admission_verdict,
            Some(AdmissionVerdict::Pending(AdmissionReason::Sensitive)),
            "sensitive episode must be held, not stored"
        );
        assert!(
            !report.episode_stored,
            "held episode is not reported as stored"
        );
        assert_eq!(exec.admission().pending_count(), 1);
        assert!(
            workspace.get_current_episode().is_none(),
            "held episode is not published as the current (stored) episode"
        );
        // The signal behind the verdict is recorded for audit.
        let signal = report.admission_signal.expect("signal recorded");
        assert!(
            signal.sensitivity >= exec.admission().sensitivity_pending_at,
            "sensitivity forced the hold: {signal:?}"
        );
        assert!(
            workspace
                .get_thoughts()
                .iter()
                .any(|t| t.contains("held for human confirmation")),
            "executive narrated the hold: {:?}",
            workspace.get_thoughts()
        );
    }

    #[tokio::test]
    async fn rejected_episode_is_not_stored_but_verdict_is_recorded() {
        // A cycle whose execution failed verification produces an unreliable
        // episode: the gate rejects it (no memory pollution), but the report
        // still records the verdict + signal so the decision is auditable.
        let workspace = CognitiveBlackboard::new();
        let mut exec = Executive::new();
        // No tools registered → execution fails → unreliable episode.
        exec.add_goal("flaky unverified step", 0.9);
        let report = exec.cycle(&workspace).await;

        let verdict = report
            .admission_verdict
            .expect("admission verdict recorded");
        assert!(
            matches!(verdict, AdmissionVerdict::Reject(_)),
            "unverified episode must be rejected: {verdict}"
        );
        assert!(!report.episode_stored);
        assert!(exec.episode_store().count() == 0);
        assert!(report.admission_signal.is_some());
    }

    /// A tool that fails the first `fail_times` calls (recording a "partial
    /// side effect") then succeeds — the deterministic shape of a transient,
    /// recoverable failure (S5-A).
    struct FlakyTool {
        capabilities: Vec<Capability>,
        calls: Mutex<u32>,
        fail_times: u32,
    }

    impl FlakyTool {
        fn new(fail_times: u32, capabilities: Vec<Capability>) -> Self {
            Self {
                capabilities,
                calls: Mutex::new(0),
                fail_times,
            }
        }
    }

    #[async_trait]
    impl Tool for FlakyTool {
        fn name(&self) -> &str {
            "flaky"
        }
        fn description(&self) -> &str {
            "test tool that fails transiently then succeeds"
        }
        fn capabilities(&self) -> Vec<Capability> {
            self.capabilities.clone()
        }
        async fn execute(&self, _action: &Action, _context: &ExecutionContext) -> ToolResult {
            let mut calls = self.calls.lock().unwrap();
            *calls += 1;
            if *calls <= self.fail_times {
                ToolResult::error("transient failure after partial side effect")
            } else {
                ToolResult::success("done after retry")
            }
        }
    }

    /// S5-A (kernel level): the goal's action fails once after a partial side
    /// effect, the Executive re-observes reality, retries, verifies, and marks
    /// the goal `recovered` — FRIDAY does not blindly repeat the original
    /// action nor fail it; the re-observation is part of the audit trail.
    #[tokio::test]
    async fn goal_action_recovers_after_a_transient_failure() {
        let workspace = CognitiveBlackboard::new();
        let mut executive = Executive::new();
        executive.register_tool(Box::new(FlakyTool::new(
            1,
            vec![Capability::Custom("windows.open".into())],
        )));
        workspace.set_world_state(
            WorldState::new().with_tool(Capability::Custom("windows.open".into())),
        );

        executive.add_goal("Open notepad", 0.9);
        let report = executive.cycle(&workspace).await;

        assert!(report.plan_executed, "the goal action ran");
        assert!(
            report.recovered,
            "recovered after re-observe + retry: attempts={} result={:?}",
            report.recovery_attempts, report.result
        );
        assert_eq!(report.recovery_attempts, 2, "fail once, succeed once");
        assert!(!report.recoverable, "no exhaustion on a recovered goal");
        assert_eq!(
            report.result.as_ref().map(|r| r.success),
            Some(true),
            "the retried execution succeeded"
        );
        // The re-observation is in the audit trail, not just the outcome.
        let trace = workspace.get_current_trace().expect("trace published");
        assert!(
            trace
                .nodes
                .iter()
                .any(|n| n.kind == TraceNodeKind::Recovery),
            "a Recovery node records the retry: {:?}",
            trace.nodes.iter().map(|n| &n.kind).collect::<Vec<_>>()
        );
        assert!(
            workspace
                .get_thoughts()
                .iter()
                .any(|t| t.contains("recovery — re-observing")),
            "the executive narrated the re-observation: {:?}",
            workspace.get_thoughts()
        );
    }

    /// S5-B (kernel level): a tool that fails every attempt — the recovery loop
    /// hits its hard ceiling (1 + max_retries total attempts), the goal fails,
    /// and the report carries the recovery evidence: why it failed, how many
    /// attempts ran, and the last observed world state.
    #[tokio::test]
    async fn goal_action_exhausts_recovery_and_fails_recoverable() {
        let workspace = CognitiveBlackboard::new();
        let mut executive = Executive::new();
        executive.register_tool(Box::new(
            MockTool::failure("windows", "host unreachable")
                .with_capabilities(vec![Capability::Custom("windows.open".into())]),
        ));
        workspace.set_world_state(
            WorldState::new().with_tool(Capability::Custom("windows.open".into())),
        );

        executive.add_goal("Open notepad", 0.9);
        let report = executive.cycle(&workspace).await;

        assert!(report.plan_executed, "the goal action ran");
        assert!(
            report.recoverable,
            "exhausted recovery is a retryable failure: attempts={}",
            report.recovery_attempts
        );
        assert_eq!(
            report.recovery_attempts,
            1 + crate::DEFAULT_RECOVERY_MAX_ATTEMPTS,
            "one initial attempt plus the bounded retries"
        );
        let reason = report.failed_reason.as_deref().expect("failed_reason set");
        assert!(
            reason.contains("host unreachable"),
            "failed_reason is the final tool error, not a reconstruction: {reason}"
        );
        assert!(
            report.last_observed_state.is_some(),
            "the last observed state rides the report for a follow-up goal"
        );
        assert_eq!(
            report.result.as_ref().map(|r| r.success),
            Some(false),
            "the goal action ultimately failed"
        );
        assert!(!report.recovered, "an exhausted goal never claims recovery");
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

    // ── Evidence pipeline (MasterPlan #6): Web + Docs → Verify → Memory ──

    fn web_evidence_json() -> String {
        serde_json::json!({
            "query": "rust async docs",
            "evidence": [
                {
                    "source": "duckduckgo",
                    "title": "The Rust Book",
                    "url": "https://doc.rust-lang.org/book/",
                    "content": "Learn async Rust with the official book. This page is long enough to count as substance for the pipeline.",
                    "retrieved_at": "2026-08-10T00:00:00Z",
                    "confidence": 1.0,
                },
            ],
        })
        .to_string()
    }

    fn docs_evidence_json() -> String {
        serde_json::json!({
            "id": "d1",
            "source": "/repo/spec.md",
            "title": "spec",
            "mime_type": "text/markdown",
            "markdown": "# FRIDAY\nA trustworthy research pipeline with real content across the document body.",
            "metadata": {
                "filename": "spec.md",
                "size_bytes": 90,
                "format": "md",
                "parser": "passthrough",
                "chars": 70,
                "paragraphs": 2,
                "parsed_at_ms": 1_700_000_000_000u64,
            },
            "assets": [],
        })
        .to_string()
    }

    fn register_research_tools(executive: &mut Executive) {
        executive.register_tool(Box::new(
            MockTool::success("web", web_evidence_json()).with_capabilities(vec![
                Capability::SearchWeb,
                Capability::Custom("web.search".into()),
            ]),
        ));
        executive.register_tool(Box::new(
            MockTool::success("docs", docs_evidence_json()).with_capabilities(vec![
                Capability::ReadFile,
                Capability::Custom("docs.search".into()),
            ]),
        ));
        executive.register_tool(Box::new(
            MockTool::success("terminal", "ok").with_capabilities(vec![Capability::ExecuteCommand]),
        ));
    }

    fn research_world() -> WorldState {
        WorldState::new()
            .with_tool(Capability::SearchWeb)
            .with_tool(Capability::ReadFile)
            .with_tool(Capability::ExecuteCommand)
            .with_tool(Capability::Custom("web.search".into()))
            .with_tool(Capability::Custom("docs.search".into()))
    }

    #[tokio::test]
    async fn evidence_pipeline_blends_web_and_docs_into_verified_memory() {
        // The MasterPlan #6 acceptance: AnyDoc + Web → Evidence[] →
        // Verification → Memory. Both organs run as evidence actions; the
        // executive blends their outputs into one body, verifies it behind the
        // seam, and the verified episode enters episodic memory.
        let workspace = CognitiveBlackboard::new();
        let mut executive = Executive::new();
        register_research_tools(&mut executive);
        workspace.set_world_state(research_world());

        executive.add_observation("Search the web for the latest Rust async docs", 0.9);
        let report = executive.cycle(&workspace).await;

        // Every evidence action ran verified (as before).
        assert!(
            report.evidence_trail.all_verified(),
            "{trail:?}",
            trail = report.evidence_trail
        );
        // The blended body carries BOTH organs, channel-labeled.
        let body = report.evidence.expect("evidence body published");
        assert_eq!(body.set.len(), 2, "web + docs blended: {:?}", body.set);
        assert_eq!(body.set.channels(), vec!["web", "docs"]);
        assert!(body.verified.verified, "checks: {:?}", body.verified.checks);
        // Verified evidence reached episodic memory through the admission gate.
        assert!(report.episode_stored, "verified evidence must be admitted");
        let stored = executive.episode_store().recent(1);
        assert_eq!(stored.len(), 1);
        assert_eq!(stored[0].outcome, crate::EpisodeOutcome::Success);
        assert!(
            stored[0].tags.contains(&"research".to_string()),
            "evidence episode is tagged research: {:?}",
            stored[0].tags
        );
    }

    #[tokio::test]
    async fn unparseable_evidence_yields_no_blended_evidence_body() {
        // An evidence tool that returns garbage (not parseable evidence) is
        // recorded in the trail as tool-success but the blended body is empty
        // → no evidence body is published (nothing to verify or persist as
        // research evidence).
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
        workspace.set_world_state(research_world());

        executive.add_observation("Search the web for the latest Rust async docs", 0.9);
        let report = executive.cycle(&workspace).await;

        assert!(
            report.evidence.is_none(),
            "garbage evidence output yields no evidence body"
        );
        assert!(
            report.evidence_trail.all_verified(),
            "tool-level success still verified (nothing to gate)"
        );
    }

    #[tokio::test]
    async fn report_carries_the_real_execution_result() {
        // The report's `result` is the executed tool's actual output — the
        // answer the portal should show — never the goal text or a
        // reconstruction from phase/evidence metadata.
        let workspace = CognitiveBlackboard::new();
        let mut executive = Executive::new();
        executive.register_tool(Box::new(
            MockTool::success("terminal", "Build complete")
                .with_capabilities(vec![Capability::ExecuteCommand]),
        ));
        workspace.set_world_state(WorldState::new().with_tool(Capability::ExecuteCommand));

        // "run the build" classifies as ExecuteCommand via infer_capability.
        executive.add_observation("run the build", 0.9);
        let report = executive.cycle(&workspace).await;

        assert!(report.plan_executed);
        let result = report.result.expect("a completed execution has a result");
        assert_eq!(result.kind, GoalResultKind::Text);
        assert_eq!(result.text, "Build complete");
        assert!(result.success);
        // The metadata trail survives alongside the result.
        assert!(
            report.phases_completed.len() >= 10,
            "the full cognitive cycle still runs: {:?}",
            report.phases_completed
        );
        assert!(report.evidence_trail.records.is_empty());
    }

    // ── S3 human control ─────────────────────────────────────────

    #[tokio::test]
    async fn cancelled_cycle_stops_before_executing_and_records_no_episode() {
        // A cancel that lands before/at the cycle start stops the cycle at the
        // first checkpoint: nothing executes, no terminal episode is recorded,
        // and the report carries the cancellation.
        let workspace = CognitiveBlackboard::new();
        let mut executive = Executive::new();
        executive.register_tool(Box::new(
            MockTool::success("terminal", "ran")
                .with_capabilities(vec![Capability::ExecuteCommand]),
        ));
        workspace.set_world_state(WorldState::new().with_tool(Capability::ExecuteCommand));

        executive.add_observation("run the build", 0.9);
        executive.cancel();
        let report = executive.cycle(&workspace).await;

        assert!(report.cancelled, "cancel must mark the report cancelled");
        assert!(!report.paused);
        assert!(
            !report.plan_executed,
            "a cancelled goal never executes a plan"
        );
        assert!(
            !report.phases_completed.contains(&ExecutivePhase::Execute),
            "cycle stops before execution: {:?}",
            report.phases_completed
        );
        assert!(!report.episode_stored);
        assert!(
            executive.episode_store().episodes().is_empty(),
            "a cancelled goal records no terminal episode"
        );
    }

    #[tokio::test]
    async fn paused_then_resumed_cycle_runs_to_completion() {
        let workspace = CognitiveBlackboard::new();
        let mut executive = Executive::new();
        executive.register_tool(Box::new(
            MockTool::success("terminal", "Build complete")
                .with_capabilities(vec![Capability::ExecuteCommand]),
        ));
        workspace.set_world_state(WorldState::new().with_tool(Capability::ExecuteCommand));

        executive.add_observation("run the build", 0.9);
        // Pause then resume before the cycle: pause must not stick.
        executive.pause();
        executive.resume();
        let report = executive.cycle(&workspace).await;

        assert!(!report.cancelled && !report.paused, "{report:?}");
        assert!(
            report.plan_executed,
            "a resumed goal runs to completion: {report:?}"
        );
    }

    #[tokio::test]
    async fn cancel_landing_mid_execution_skips_remaining_actions() {
        // A web evidence goal has several actions (web + docs + main). The
        // first action parks on a gate; the test cancels while it is still
        // "running", then releases it — the cooperative checkpoint must skip
        // every remaining action, so only the web evidence record exists.
        let workspace = CognitiveBlackboard::new();
        let mut executive = Executive::new();

        let (gate_tx, gate_rx) = watch::channel(false);
        let (began_tx, mut began_rx) = oneshot::channel();
        executive.register_tool(Box::new(GatedTool {
            capabilities: vec![
                Capability::SearchWeb,
                Capability::Custom("web.search".into()),
            ],
            began: Mutex::new(Some(began_tx)),
            gate_rx,
        }));
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

        let abort = executive.abort_token();
        let workspace_clone = workspace.clone();
        let handle = tokio::spawn(async move { executive.cycle(&workspace_clone).await });

        // Wait until the gated web action is actually executing…
        tokio::time::timeout(std::time::Duration::from_secs(5), &mut began_rx)
            .await
            .expect("the gated web action must start executing")
            .expect("began signal");
        // …land the cancel while it is parked, then release the gate.
        abort.store(true, Ordering::SeqCst);
        let _ = gate_tx.send(true);
        let report = handle.await.expect("cycle task");

        assert!(report.cancelled, "mid-execution cancel marks the report");
        assert!(
            !report.plan_executed,
            "the main (non-evidence) action was skipped: {report:?}"
        );
        assert_eq!(
            report.evidence_trail.records.len(),
            1,
            "only the web evidence action ran; docs + main were skipped: {:?}",
            report.evidence_trail.records
        );
        assert_eq!(report.evidence_trail.records[0].channel, "web");
        assert!(
            workspace
                .get_thoughts()
                .iter()
                .any(|t| t.contains("skipping remaining actions")),
            "{:?}",
            workspace.get_thoughts()
        );
    }

    #[tokio::test]
    async fn sensitive_goal_holds_episode_and_approve_pending_admits_it() {
        let workspace = CognitiveBlackboard::new();
        let mut executive = Executive::new();
        executive.register_tool(Box::new(
            MockTool::success("terminal", "rotated")
                .with_capabilities(vec![Capability::ExecuteCommand]),
        ));
        workspace.set_world_state(WorldState::new().with_tool(Capability::ExecuteCommand));

        // The sensitivity gate fires on "api_key"/"password" — the episode is
        // held for a human, never silently stored, and the report exposes the
        // held id as the approve handle.
        executive.add_goal("rotate my api_key and reset the password", 0.9);
        let report = executive.cycle(&workspace).await;

        assert_eq!(
            report.admission_verdict,
            Some(AdmissionVerdict::Pending(AdmissionReason::Sensitive)),
            "{:?}",
            report.admission_signal
        );
        let held_id = report
            .held_episode_id
            .clone()
            .expect("a held episode exposes its approve handle");
        assert_eq!(executive.admission().pending_count(), 1);
        assert!(
            executive.episode_store().episodes().is_empty(),
            "held, never silently stored"
        );

        // task.approve path: approve_pending moves the episode into memory.
        executive.approve_pending(&held_id).expect("approve");
        assert_eq!(executive.admission().pending_count(), 0);
        assert_eq!(executive.episode_store().episodes().len(), 1);
        // A second approve of the same id is a miss — no state change.
        assert!(
            executive.approve_pending(&held_id).is_err(),
            "approving an already-approved id is a miss"
        );
    }
}
