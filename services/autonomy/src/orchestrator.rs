//! 7A orchestrator — drives one bounded cognitive tick.
//!
//! `AutonomousTick::run` performs exactly one
//! `observe → (goal) → plan → predict → execute → verify → world_model.update → memory.writeback → STOP`
//! cycle and returns a `CognitiveTick`. It does not loop, does not schedule, and
//! does not spawn further ticks. The caller may invoke `run` again later (a future
//! scheduler, 7E), but each call creates its own `trace_id` — there is no hidden
//! recursion.
//!
//! Stage behaviors are injected as traits so the real planner/director/verifier
//! (7B+) plug in without changing the bounded control flow proven here. The
//! optional world-model collaborator (7B) lets the tick predict the post-action
//! state, re-observe reality, and update its belief — recording only the
//! assessment (Correct / Mismatch / Unknown) as a `world_model.update` span. The
//! optional memory store (7C) turns that outcome into a selective `Experience` →
//! `Lesson` writeback (`memory.writeback` span). No world-model domain state is
//! ever written into `CognitiveTrace`.

use chrono::Utc;
use friday_core::trace::{
    CognitiveTraceContext, GoalId, ResultType, SessionId, SpanId, TraceCollector, TraceId,
    VerificationStatus,
};

use crate::memory::{Experience, ExperienceExtractor, LessonCondition, MemoryStore};
use crate::skill::{
    ApplicableSkill, SkillAdmission, SkillContext, SkillEvaluator, SkillProvider, SkillStore,
};
use crate::tick::{
    CognitiveTick, ExecutionResult, NextAction, Observation, Plan, TickOutcome, TickTrigger,
};
use crate::world::{PredictionOutcome, WorldModel, WorldSnapshot};

/// What the tick observes about the world before deciding.
pub trait ObservationSource: Send + Sync {
    fn observe(&self) -> Observation;
}

/// Turns an observation into at most one bounded goal. `None` ⇒ no-op tick.
pub trait GoalSelector: Send + Sync {
    fn select_goal(&self, obs: &Observation) -> Option<String>;
}

/// Produces a plan for a chosen goal. `skills` are applicable skills surfaced by the
/// `SkillProvider` — the planner MAY consider them but still owns the decision; they
/// never bypass planning, permissions, or verification.
pub trait Planner: Send + Sync {
    fn plan(&self, goal: &str, skills: &[ApplicableSkill]) -> Plan;
}

/// Executes a plan. Records its own internal spans (agent / tool) through the
/// supplied `collector` under `ctx`, so the episode hierarchy stays intact.
pub trait Executor: Send + Sync {
    fn execute(
        &self,
        ctx: &CognitiveTraceContext,
        collector: &dyn TraceCollector,
        plan: &Plan,
    ) -> ExecutionResult;
}

/// Verifies an execution, returning an explicit verdict.
pub trait Verifier: Send + Sync {
    fn verify(&self, goal: &str, result: &ExecutionResult) -> VerificationStatus;
}

/// The single bounded autonomous tick. Construct with injected stage behaviors and
/// call [`AutonomousTick::run`] once per tick.
pub struct AutonomousTick {
    tick_id: u64,
    trigger: TickTrigger,
    observation: Box<dyn ObservationSource>,
    goal_selector: Box<dyn GoalSelector>,
    planner: Box<dyn Planner>,
    executor: Box<dyn Executor>,
    verifier: Box<dyn Verifier>,
    /// Optional world-model collaborator (7B). When present, the tick predicts the
    /// post-action state, re-observes reality, and asks the model to update itself.
    /// `None` keeps the tick hermetic (7A behavior).
    world_model: Option<Box<dyn WorldModel>>,
    /// Optional memory store (7C). When present alongside a world model, the tick
    /// extracts an `Experience` and runs it through the `ExperienceExtractor`'s
    /// selective admission policy. `None` skips memory writeback.
    memory: Option<Box<dyn MemoryStore>>,
    /// Admission policy (7C). Held across ticks so corroboration accumulates.
    extractor: ExperienceExtractor,
    /// Optional skill store (7D). When present, the tick attempts to promote any
    /// eligible lesson into a skill via the `SkillEvaluator`. This only *learns*
    /// skills — it never executes them or changes the planner.
    skill_store: Option<Box<dyn SkillStore>>,
    /// Skill promotion policy (7D). Deterministic thresholds + known-good trajectories.
    skill_evaluator: SkillEvaluator,
    /// Optional skill provider (7E seam 1). When present, the tick asks it for
    /// applicable skills and surfaces them to the planner. It never bypasses
    /// planning, permissions, or verification — the planner still owns the decision.
    skill_provider: Option<Box<dyn SkillProvider>>,
}

impl AutonomousTick {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        tick_id: u64,
        trigger: TickTrigger,
        observation: Box<dyn ObservationSource>,
        goal_selector: Box<dyn GoalSelector>,
        planner: Box<dyn Planner>,
        executor: Box<dyn Executor>,
        verifier: Box<dyn Verifier>,
    ) -> Self {
        Self {
            tick_id,
            trigger,
            observation,
            goal_selector,
            planner,
            executor,
            verifier,
            world_model: None,
            memory: None,
            extractor: ExperienceExtractor::default(),
            skill_store: None,
            skill_evaluator: SkillEvaluator::default(),
            skill_provider: None,
        }
    }

    /// Attach a world-model collaborator (7B). The tick stays bounded and `&self`;
    /// the model uses interior mutability.
    pub fn with_world_model(mut self, world_model: Box<dyn WorldModel>) -> Self {
        self.world_model = Some(world_model);
        self
    }

    /// Attach a memory store (7C). Memory writeback requires a world model too.
    pub fn with_memory_store(mut self, memory: Box<dyn MemoryStore>) -> Self {
        self.memory = Some(memory);
        self
    }

    /// Attach a skill store (7D). Skill promotion only *learns*; it never executes.
    pub fn with_skill_store(mut self, skill_store: Box<dyn SkillStore>) -> Self {
        self.skill_store = Some(skill_store);
        self
    }

    /// Attach a skill provider (7E seam 1). The tick consults it for applicable
    /// skills and surfaces them to the planner; it never bypasses planning,
    /// permissions, or verification.
    pub fn with_skill_provider(mut self, provider: Box<dyn SkillProvider>) -> Self {
        self.skill_provider = Some(provider);
        self
    }

    /// Run exactly one bounded tick and return its record. Writes the episode(s)
    /// to `collector`. Every branch (including no-work) produces a trace.
    pub fn run(&self, collector: &dyn TraceCollector) -> CognitiveTick {
        let session = SessionId::default();
        let goal_id = GoalId::default();
        let trace_id = TraceId::default();

        let observation = self.observation.observe();
        let selected = self.goal_selector.select_goal(&observation);

        match selected {
            // 7A.1 — cheapest safe path: observe, find no work, record, stop.
            None => self.run_no_work(collector, session, goal_id, trace_id, observation),
            Some(goal) => {
                self.run_episode(collector, session, goal_id, trace_id, observation, goal)
            }
        }
    }

    fn run_no_work(
        &self,
        collector: &dyn TraceCollector,
        session: SessionId,
        goal_id: GoalId,
        trace_id: TraceId,
        observation: Observation,
    ) -> CognitiveTick {
        let root = SpanId::default();
        let ctx = CognitiveTraceContext::for_goal(trace_id, session, goal_id);
        collector.record(
            ctx.trace(root, "autonomy", "observe")
                .with_timing(Utc::now(), 1)
                .with_result(ResultType::Skipped),
        );
        let child = ctx.child(root);
        collector.record(
            child
                .trace(SpanId::default(), "autonomy", "no_work")
                .with_timing(Utc::now(), 0)
                .with_result(ResultType::Skipped),
        );

        CognitiveTick {
            tick_id: self.tick_id,
            trigger: self.trigger,
            input_state: observation,
            selected_goal: None,
            trace_id,
            outcome: TickOutcome::NoWork,
            next_action: NextAction::Stop,
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn run_episode(
        &self,
        collector: &dyn TraceCollector,
        session: SessionId,
        goal_id: GoalId,
        trace_id: TraceId,
        observation: Observation,
        goal: String,
    ) -> CognitiveTick {
        let root = SpanId::default();
        let planner_span = SpanId::default();
        let planner_ctx = CognitiveTraceContext::for_goal(trace_id, session, goal_id).child(root);

        // 7E seam 1 — Skill consultation. The tick asks the provider for skills
        // applicable to THIS goal/observation, then surfaces them to the planner.
        // The planner still owns the decision; skills never bypass planning,
        // permissions, or verification.
        let applicable: Vec<ApplicableSkill> = self
            .skill_provider
            .as_ref()
            .map(|p| {
                p.applicable_skills(&SkillContext {
                    action: goal.clone(),
                    signals: observation.signals.clone(),
                })
            })
            .unwrap_or_default();

        // Planner span (child of the goal root).
        collector.record(
            planner_ctx
                .trace(planner_span, "planner", "create_plan")
                .with_timing(Utc::now(), 10)
                .with_result(ResultType::Success),
        );

        // Record the consultation as a normal `skills.consult` operation so it is
        // observable without modifying the frozen trace schema. Only emitted when a
        // provider is actually attached — hermetic 7A–7D ticks stay at their counts.
        if self.skill_provider.is_some() {
            collector.record(
                planner_ctx
                    .child(planner_span)
                    .trace(SpanId::default(), "skills", "consult")
                    .with_timing(Utc::now(), 1)
                    .with_result(ResultType::Success),
            );
        }

        // R2-1 — capture the world-model revision the plan will be grounded
        // against BEFORE planning, so execution can be refused when reality
        // has moved past this plan's grounding.
        let planned_revision: Option<u64> = self
            .world_model
            .as_ref()
            .and_then(|wm| wm.observe())
            .map(|s| s.revision);

        // Execute — the executor records its own agent/tool spans under the planner.
        let plan = self.planner.plan(&goal, &applicable);

        // R2-1 — stale-plan admission gate. If the world advanced between
        // grounding and execution, the plan is refused as `stale_plan_rejected`
        // BEFORE any executor side effect. This is a refusal, not an execution
        // failure: nothing ran, nothing is journaled, and no verification or
        // world update is synthesized for work that never happened.
        if let Some(wm) = &self.world_model {
            let current_revision: Option<u64> = wm.observe().map(|s| s.revision);
            if let (Some(planned), Some(current)) = (planned_revision, current_revision) {
                if planned != current {
                    collector.record(
                        planner_ctx
                            .child(planner_span)
                            .trace(SpanId::default(), "execution", "stale_plan_rejected")
                            .with_timing(Utc::now(), 0)
                            .with_result(ResultType::Failure),
                    );
                    return CognitiveTick {
                        tick_id: self.tick_id,
                        trigger: self.trigger,
                        input_state: observation,
                        selected_goal: Some(goal),
                        trace_id,
                        outcome: TickOutcome::Failure,
                        next_action: NextAction::Stop,
                    };
                }
            }
        }
        // L1 canonical learning identity — derived BEFORE execution from the
        // world model's own prediction against the pre-action belief, so it
        // is identical whether the run later succeeds or fails. The SAME
        // predicted snapshot is reused for the post-run assessment (never
        // re-predicted).
        let pre_belief: Option<WorldSnapshot> =
            self.world_model.as_ref().and_then(|wm| wm.observe());
        let predicted: Option<WorldSnapshot> =
            self.world_model.as_ref().map(|wm| wm.predict(&goal));
        let learning_condition: Option<LessonCondition> = match (&predicted, &pre_belief) {
            (Some(pred), Some(pre)) => Some(
                crate::memory::ExperienceExtractor::predicted_delta_condition(&goal, pred, pre),
            ),
            _ => None,
        };

        let exec_ctx = planner_ctx.child(planner_span);
        let exec_result = self.executor.execute(&exec_ctx, collector, &plan);

        // Verify — record the verification gate explicitly with its verdict.
        let verdict = self.verifier.verify(&goal, &exec_result);
        collector.record(
            planner_ctx
                .child(planner_span)
                .trace(SpanId::default(), "verification", "verification_gate")
                .with_timing(Utc::now(), 5)
                .with_verification(verdict.clone())
                .with_result(if verdict.is_passed() {
                    ResultType::Success
                } else {
                    ResultType::Failure
                }),
        );

        // 7B — Re-observe reality and ask the world model to update itself.
        // Records ONE `world_model.update` span carrying the assessment
        // (Correct / Mismatch / Unknown). Domain state is NEVER written into the
        // trace — only the operation and its verdict.
        let mut observed_actual: Option<WorldSnapshot> = None;
        let mut assessment: Option<PredictionOutcome> = None;
        let mut experience: Option<Experience> = None;
        if let (Some(wm), Some(pred)) = (&self.world_model, &predicted) {
            let actual = wm.observe();
            let assmt = wm.update(pred, actual.clone());
            observed_actual = actual;
            assessment = Some(assmt.clone());

            let mut span = planner_ctx
                .child(planner_span)
                .trace(SpanId::default(), "world_model", "update")
                .with_timing(Utc::now(), 3)
                .with_result(assmt.result());
            if let Some(v) = assmt.verification() {
                span = span.with_verification(v);
            }
            collector.record(span);
        }

        // L1 — verified-successful executions earn promotion evidence against
        // the canonical condition. Only Passed counts; unverified/failed
        // executions never touch the ledger. Correct/Unknown assessments are
        // still discarded by the extractor — verification evidence is a
        // separate ledger, never a lesson.
        if verdict.is_passed() {
            if let (Some(mem), Some(cond)) = (&self.memory, &learning_condition) {
                mem.record_verification(cond);
            }
        }

        // 7C — Experience extraction -> selective memory writeback.
        // Builds one `Experience` from the world-model outcome and runs it through
        // the `ExperienceExtractor` admission policy (corroboration + high-surprise).
        // Records a normal `memory.writeback` operation; the frozen `CognitiveTrace`
        // schema is never modified.
        if let (Some(mem), Some(assmt)) = (&self.memory, &assessment) {
            let exp = Experience {
                action: goal.clone(),
                predicted: predicted.clone().unwrap_or_default(),
                actual: observed_actual.clone(),
                prediction_error: match assmt {
                    PredictionOutcome::Mismatch { diff } => Some(diff.clone()),
                    _ => None,
                },
                assessment: assmt.clone(),
                observed_at_ms: Utc::now().timestamp_millis() as u64,
            };
            // L1 — the canonical prediction-derived condition, when present,
            // is THE learning identity for this tick: lesson keys, skill
            // triggers, and verification evidence all share it. Legacy
            // fallback (no world model) keeps the mismatch-derived condition.
            match &learning_condition {
                Some(cond) => self.extractor.process_with_condition(&**mem, &exp, cond),
                None => self.extractor.process(&**mem, &exp),
            };
            experience = Some(exp);
            collector.record(
                planner_ctx
                    .child(planner_span)
                    .trace(SpanId::default(), "memory", "writeback")
                    .with_timing(Utc::now(), 2)
                    .with_result(ResultType::Success),
            );
        }

        // 7D — Skill formation (learning only, never execution / planner change).
        // If a skill store is attached, attempt to promote the experience's lesson
        // into a skill: look up the lesson, find a known-good trajectory, and let the
        // evaluator decide. The planner is deliberately NOT modified here.
        if let Some(skills) = &self.skill_store {
            if let (Some(mem), Some(exp)) = (&self.memory, &experience) {
                let cond = learning_condition
                    .clone()
                    .unwrap_or_else(|| ExperienceExtractor::condition_of(exp));
                if let (Some(lesson), Some(traj)) = (
                    mem.find_similar_lesson(&cond),
                    self.skill_evaluator.trajectory_for(&cond),
                ) {
                    let candidate = self.skill_evaluator.form_candidate(&lesson, &cond, traj);
                    if let SkillAdmission::Promote(skill) =
                        self.skill_evaluator.evaluate(&candidate)
                    {
                        skills.admit(skill);
                    }
                }
            }
            collector.record(
                planner_ctx
                    .child(planner_span)
                    .trace(SpanId::default(), "skill", "formation")
                    .with_timing(Utc::now(), 2)
                    .with_result(ResultType::Success),
            );
        }

        // Root goal span recorded LAST, carrying the final outcome + verdict.
        let (outcome, root_result) = if verdict.is_passed() {
            (TickOutcome::Success, ResultType::Success)
        } else {
            (TickOutcome::Failure, ResultType::Failure)
        };
        let root_ctx = CognitiveTraceContext::for_goal(trace_id, session, goal_id);
        collector.record(
            root_ctx
                .trace(root, "goal", "resolve")
                .with_timing(Utc::now(), 20)
                .with_result(root_result)
                .with_verification(verdict),
        );

        CognitiveTick {
            tick_id: self.tick_id,
            trigger: self.trigger,
            input_state: observation,
            selected_goal: Some(goal),
            trace_id,
            outcome,
            next_action: NextAction::Stop,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::SimpleMemory;
    use crate::scheduler::{Clock, FakeClock, Scheduler, SchedulerDecision, SchedulerPolicy};
    use crate::skill::SimpleSkillStore;
    use crate::world::{SimpleWorldModel, WorldSnapshot};
    use friday_core::trace::{
        reconstruct_trace_tree, CognitiveTrace, InMemoryTraceCollector, ResultType, TraceCollector,
        VerificationStatus,
    };
    use friday_observability::metrics::CognitiveMetrics;
    use friday_observability::viewer::ViewerModel;
    use serde_json::json;
    use std::collections::{HashMap, HashSet};
    use std::sync::Arc;

    // ── Fakes ────────────────────────────────────────────────────────────

    struct ObserveWork;
    impl ObservationSource for ObserveWork {
        fn observe(&self) -> Observation {
            Observation {
                summary: "inbox has actionable tasks".into(),
                signals: vec!["task:research".into()],
            }
        }
    }

    struct ObserveIdle;
    impl ObservationSource for ObserveIdle {
        fn observe(&self) -> Observation {
            Observation {
                summary: "nothing actionable".into(),
                signals: vec![],
            }
        }
    }

    struct SelectGoal;
    impl GoalSelector for SelectGoal {
        fn select_goal(&self, _obs: &Observation) -> Option<String> {
            Some("research project".into())
        }
    }

    struct SelectNone;
    impl GoalSelector for SelectNone {
        fn select_goal(&self, _obs: &Observation) -> Option<String> {
            None
        }
    }

    struct FakePlanner;
    impl Planner for FakePlanner {
        fn plan(&self, goal: &str, _skills: &[ApplicableSkill]) -> Plan {
            Plan {
                goal: goal.into(),
                steps: vec!["gather".into(), "synthesize".into()],
            }
        }
    }

    /// Records a realistic agent → tool subtree under the supplied context.
    struct FakeExecutor;
    impl Executor for FakeExecutor {
        fn execute(
            &self,
            ctx: &CognitiveTraceContext,
            collector: &dyn TraceCollector,
            _plan: &Plan,
        ) -> ExecutionResult {
            let agent_span = SpanId::default();
            collector.record(
                ctx.trace(agent_span, "agents", "execute")
                    .with_timing(Utc::now(), 8)
                    .with_result(ResultType::Success),
            );
            let tool_ctx = ctx.child(agent_span);
            collector.record(
                tool_ctx
                    .trace(SpanId::default(), "web", "tool_call")
                    .with_timing(Utc::now(), 4)
                    .with_result(ResultType::Success),
            );
            ExecutionResult {
                success: true,
                detail: "completed".into(),
            }
        }
    }

    struct VerifyPass;
    impl Verifier for VerifyPass {
        fn verify(&self, _goal: &str, _result: &ExecutionResult) -> VerificationStatus {
            VerificationStatus::Passed
        }
    }

    struct VerifyFail;
    impl Verifier for VerifyFail {
        fn verify(&self, _goal: &str, _result: &ExecutionResult) -> VerificationStatus {
            VerificationStatus::Failed {
                reason: "evidence mismatch".into(),
            }
        }
    }

    // ── R2-1 fixtures ────────────────────────────────────────────────────

    /// Counts executions across ticks; shared through `Arc`.
    #[derive(Default)]
    struct CountingExecutor(std::sync::atomic::AtomicUsize);
    impl Executor for CountingExecutor {
        fn execute(
            &self,
            _ctx: &CognitiveTraceContext,
            collector: &dyn TraceCollector,
            _plan: &Plan,
        ) -> ExecutionResult {
            self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            collector.record(
                CognitiveTraceContext::for_goal(
                    TraceId::default(),
                    SessionId::default(),
                    GoalId::default(),
                )
                .trace(SpanId::default(), "agents", "execute")
                .with_timing(Utc::now(), 1)
                .with_result(ResultType::Success),
            );
            ExecutionResult {
                success: true,
                detail: "executed".into(),
            }
        }
    }

    /// `Arc`-shared counting executor usable as a `Box<dyn Executor>`.
    struct SharedCountingExecutor(Arc<CountingExecutor>);
    impl Executor for SharedCountingExecutor {
        fn execute(
            &self,
            ctx: &CognitiveTraceContext,
            collector: &dyn TraceCollector,
            plan: &Plan,
        ) -> ExecutionResult {
            self.0.execute(ctx, collector, plan)
        }
    }

    /// Lets several ticks share one `SimpleWorldModel`.
    struct SharedModel(Arc<SimpleWorldModel>);
    impl WorldModel for SharedModel {
        fn predict(&self, action: &str) -> WorldSnapshot {
            self.0.predict(action)
        }
        fn observe(&self) -> Option<WorldSnapshot> {
            self.0.observe()
        }
        fn update(
            &self,
            predicted: &WorldSnapshot,
            actual: Option<WorldSnapshot>,
        ) -> PredictionOutcome {
            self.0.update(predicted, actual)
        }
    }

    /// A world whose observation advances on every look: the plan is always
    /// grounded against an older revision than the one seen at admission time.
    struct AdvancingWorldModel(std::sync::atomic::AtomicUsize);
    impl WorldModel for AdvancingWorldModel {
        fn predict(&self, _action: &str) -> WorldSnapshot {
            WorldSnapshot::new()
        }
        fn observe(&self) -> Option<WorldSnapshot> {
            let n = self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            let mut snap = WorldSnapshot::new().with_variable("step", serde_json::json!(n));
            snap.revision = n as u64;
            Some(snap)
        }
        fn update(
            &self,
            _predicted: &WorldSnapshot,
            _actual: Option<WorldSnapshot>,
        ) -> PredictionOutcome {
            PredictionOutcome::Unknown
        }
    }

    fn traces(collector: &InMemoryTraceCollector) -> Vec<CognitiveTrace> {
        collector.traces().clone()
    }

    // ── 7A.1: No-op tick ────────────────────────────────────────────────

    #[test]
    fn no_op_tick_records_no_work_and_stops() {
        let collector = InMemoryTraceCollector::new();
        let tick = AutonomousTick::new(
            1,
            TickTrigger::Startup,
            Box::new(ObserveIdle),
            Box::new(SelectNone),
            Box::new(FakePlanner),
            Box::new(FakeExecutor),
            Box::new(VerifyPass),
        )
        .run(&collector);

        assert_eq!(tick.outcome, TickOutcome::NoWork);
        assert_eq!(tick.selected_goal, None);
        assert_eq!(tick.next_action, NextAction::Stop);

        let t = traces(&collector);
        // observe + no_work, exactly one episode.
        assert_eq!(t.len(), 2);
        let ids: HashSet<TraceId> = t.iter().map(|x| x.trace_id).collect();
        assert_eq!(ids.len(), 1, "no-work tick still gets exactly one episode");
        assert!(t.iter().any(|x| x.operation == "no_work"));
    }

    // ── 7A main: one complete bounded episode ───────────────────────────

    #[test]
    fn autonomous_tick_executes_one_complete_episode() {
        let collector = InMemoryTraceCollector::new();
        let tick = AutonomousTick::new(
            42,
            TickTrigger::Manual,
            Box::new(ObserveWork),
            Box::new(SelectGoal),
            Box::new(FakePlanner),
            Box::new(FakeExecutor),
            Box::new(VerifyPass),
        )
        .run(&collector);

        // Tick terminates with a definitive outcome and does NOT schedule another.
        assert_eq!(tick.outcome, TickOutcome::Success);
        assert_eq!(tick.selected_goal.as_deref(), Some("research project"));
        assert_eq!(tick.next_action, NextAction::Stop);

        let t = traces(&collector);

        // Exactly one cognitive episode.
        let ids: HashSet<TraceId> = t.iter().map(|x| x.trace_id).collect();
        assert_eq!(ids.len(), 1, "exactly one cognitive episode per tick");
        assert_eq!(ids.iter().next().copied(), Some(tick.trace_id));

        // Exactly the spans of one episode — no second tick silently started.
        assert_eq!(
            t.len(),
            5,
            "one episode: root+planner+agent+tool+verification"
        );

        // The trace reconstructs into a valid single-rooted tree.
        let tree = reconstruct_trace_tree(&t);
        assert_eq!(tree.len(), 1, "exactly one root");
        let root = &tree[0];
        assert_eq!(root.trace.operation, "resolve");

        // Planner under the goal.
        assert_eq!(root.children.len(), 1);
        let planner = &root.children[0];
        assert_eq!(planner.trace.operation, "create_plan");

        // Under planner: one agent and one explicit verification gate.
        let agents: Vec<&_> = planner
            .children
            .iter()
            .filter(|c| c.trace.operation == "execute")
            .collect();
        let gates: Vec<&_> = planner
            .children
            .iter()
            .filter(|c| c.trace.operation == "verification_gate")
            .collect();
        assert_eq!(agents.len(), 1, "one agent span");
        assert_eq!(gates.len(), 1, "one verification gate");

        // Agent has a tool child.
        assert_eq!(agents[0].children.len(), 1);
        assert_eq!(agents[0].children[0].trace.operation, "tool_call");

        // Verification is explicit (positive verdict recorded on the span).
        assert!(gates[0].trace.verification_info.is_some());
        assert!(gates[0].trace.verification().is_passed());

        // Same single source feeds the 6C metrics + 6D viewer unchanged.
        let metrics = CognitiveMetrics::new();
        for span in &t {
            metrics.record(span);
        }
        let snap = metrics.snapshot();
        assert_eq!(snap.goals, 1);
        assert_eq!(snap.successes, 1);
        assert_eq!(snap.agent_runs, 1);
        assert_eq!(snap.tool_calls, 1);
        assert_eq!(snap.verification_attempts, 1);
        assert_eq!(snap.verification_failures, 0);

        let view = ViewerModel::build(&t);
        assert_eq!(view.episodes.len(), 1);
        assert_eq!(view.episodes[0].status(), ResultType::Success);
    }

    // ── Failure branch stays observable and stops ────────────────────────

    #[test]
    fn failed_verification_yields_failure_outcome_and_stops() {
        let collector = InMemoryTraceCollector::new();
        let tick = AutonomousTick::new(
            7,
            TickTrigger::Manual,
            Box::new(ObserveWork),
            Box::new(SelectGoal),
            Box::new(FakePlanner),
            Box::new(FakeExecutor),
            Box::new(VerifyFail),
        )
        .run(&collector);

        assert_eq!(tick.outcome, TickOutcome::Failure);
        assert_eq!(tick.next_action, NextAction::Stop);

        let t = traces(&collector);
        let ids: HashSet<TraceId> = t.iter().map(|x| x.trace_id).collect();
        assert_eq!(ids.len(), 1);

        let metrics = CognitiveMetrics::new();
        for span in &t {
            metrics.record(span);
        }
        let snap = metrics.snapshot();
        assert_eq!(snap.goals, 1);
        assert_eq!(snap.failures, 1);
        assert_eq!(snap.verification_failures, 1);
    }

    // ── Each tick is independent (no shared trace_id) ───────────────────

    #[test]
    fn separate_ticks_use_separate_trace_ids() {
        let collector = InMemoryTraceCollector::new();
        let build = || {
            AutonomousTick::new(
                0,
                TickTrigger::Manual,
                Box::new(ObserveWork),
                Box::new(SelectGoal),
                Box::new(FakePlanner),
                Box::new(FakeExecutor),
                Box::new(VerifyPass),
            )
        };
        let a = build().run(&collector);
        let b = build().run(&collector);

        // Two distinct episodes, both recorded in the same collector.
        assert_ne!(a.trace_id, b.trace_id, "ticks must not reuse a trace_id");
        let t = traces(&collector);
        let ids: HashSet<TraceId> = t.iter().map(|x| x.trace_id).collect();
        assert_eq!(ids.len(), 2);
        // Each episode is exactly one root.
        let mut per_id: HashMap<TraceId, usize> = HashMap::new();
        for span in &t {
            *per_id.entry(span.trace_id).or_insert(0) += 1;
        }
        for count in per_id.values() {
            assert_eq!(*count, 5, "each tick is exactly one bounded episode");
        }
    }

    // ── 7B: world-model feedback is recorded as one bounded span ──────────

    #[test]
    fn autonomous_tick_records_world_model_update() {
        let predicted = WorldSnapshot::new().with_variable("version", json!(1));
        let actual = WorldSnapshot::new().with_variable("version", json!(1));
        let wm = SimpleWorldModel::new(predicted, Some(actual));

        let collector = InMemoryTraceCollector::new();
        let tick = AutonomousTick::new(
            1,
            TickTrigger::Manual,
            Box::new(ObserveWork),
            Box::new(SelectGoal),
            Box::new(FakePlanner),
            Box::new(FakeExecutor),
            Box::new(VerifyPass),
        )
        .with_world_model(Box::new(wm))
        .run(&collector);

        // Still exactly one bounded episode, still stops.
        assert_eq!(tick.next_action, NextAction::Stop);
        let t = traces(&collector);
        let ids: HashSet<TraceId> = t.iter().map(|x| x.trace_id).collect();
        assert_eq!(ids.len(), 1, "one episode even with world model");

        // Exactly one world_model.update span, with a Correct assessment.
        let wm_spans: Vec<&CognitiveTrace> = t
            .iter()
            .filter(|x| x.operation == "update" && x.organ == "world_model")
            .collect();
        assert_eq!(wm_spans.len(), 1);
        let span = wm_spans[0];
        assert_eq!(span.result, ResultType::Success);
        assert!(span.verification().is_passed());

        // 6 spans total: root+planner+agent+tool+verification+world_model.update.
        assert_eq!(t.len(), 6);
    }

    // ── 7B.3 integration: unknown actual → deferred, no invented outcome ──

    #[test]
    fn unknown_actual_defers_world_model_update() {
        let predicted = WorldSnapshot::new().with_variable("version", json!(1));
        let wm = SimpleWorldModel::new(predicted, None); // observe() ⇒ unknown

        let collector = InMemoryTraceCollector::new();
        let _tick = AutonomousTick::new(
            2,
            TickTrigger::Manual,
            Box::new(ObserveWork),
            Box::new(SelectGoal),
            Box::new(FakePlanner),
            Box::new(FakeExecutor),
            Box::new(VerifyPass),
        )
        .with_world_model(Box::new(wm))
        .run(&collector);

        let t = traces(&collector);
        let span = t
            .iter()
            .find(|x| x.operation == "update" && x.organ == "world_model")
            .expect("world_model.update span recorded");

        // The model does NOT invent an outcome: Pending result, no verification.
        assert_eq!(span.result, ResultType::Pending);
        assert!(span.verification_info.is_none());
    }

    // ── 7C.7: tick remains bounded; memory writeback does not spawn a tick ──

    #[test]
    fn autonomous_tick_memory_writeback_stops_and_is_bounded() {
        let wm = SimpleWorldModel::new(
            WorldSnapshot::new().with_variable("x", json!(1)),
            Some(WorldSnapshot::new().with_variable("x", json!(1))),
        );
        let mem = SimpleMemory::new();

        let collector = InMemoryTraceCollector::new();
        let _tick = AutonomousTick::new(
            1,
            TickTrigger::Manual,
            Box::new(ObserveWork),
            Box::new(SelectGoal),
            Box::new(FakePlanner),
            Box::new(FakeExecutor),
            Box::new(VerifyPass),
        )
        .with_world_model(Box::new(wm))
        .with_memory_store(Box::new(mem))
        .run(&collector);

        // One bounded episode, still stops — memory never triggers another tick.
        let t = traces(&collector);
        let ids: HashSet<TraceId> = t.iter().map(|x| x.trace_id).collect();
        assert_eq!(ids.len(), 1);

        // Exactly one memory.writeback span (operation), alongside the rest.
        let mem_spans: Vec<&CognitiveTrace> = t
            .iter()
            .filter(|x| x.operation == "writeback" && x.organ == "memory")
            .collect();
        assert_eq!(mem_spans.len(), 1);
        assert_eq!(mem_spans[0].result, ResultType::Success);

        // 7 spans: root+planner+agent+tool+verification+world_model.update+memory.writeback.
        assert_eq!(t.len(), 7);
    }

    // ── 7C.8: trace schema unchanged — only a normal operation is emitted ──

    #[test]
    fn memory_writeback_does_not_alter_frozen_trace_schema() {
        let wm = SimpleWorldModel::new(
            WorldSnapshot::new().with_variable("x", json!(1)),
            Some(WorldSnapshot::new().with_variable("x", json!(1))),
        );
        let mem = SimpleMemory::new();

        let collector = InMemoryTraceCollector::new();
        let _tick = AutonomousTick::new(
            2,
            TickTrigger::Manual,
            Box::new(ObserveWork),
            Box::new(SelectGoal),
            Box::new(FakePlanner),
            Box::new(FakeExecutor),
            Box::new(VerifyPass),
        )
        .with_world_model(Box::new(wm))
        .with_memory_store(Box::new(mem))
        .run(&collector);

        let t = traces(&collector);
        // Reconstructs into a single valid root — no schema breakage.
        let tree = reconstruct_trace_tree(&t);
        assert_eq!(tree.len(), 1);
        assert_eq!(tree[0].trace.operation, "resolve");

        // The writeback uses ONLY existing CognitiveTrace fields (operation, organ,
        // result) — no new fields were added to the frozen contract.
        let span = t
            .iter()
            .find(|x| x.operation == "writeback" && x.organ == "memory")
            .expect("memory.writeback emitted");
        assert_eq!(span.result, ResultType::Success);
    }

    // ── 7D: skill formation runs inside the bounded tick, never executes ──

    #[test]
    fn autonomous_tick_skill_formation_stops_and_is_bounded() {
        let wm = SimpleWorldModel::new(
            WorldSnapshot::new().with_variable("x", json!(1)),
            Some(WorldSnapshot::new().with_variable("x", json!(1))),
        );
        let mem = SimpleMemory::new();
        let skills = SimpleSkillStore::new();

        let collector = InMemoryTraceCollector::new();
        let _tick = AutonomousTick::new(
            1,
            TickTrigger::Manual,
            Box::new(ObserveWork),
            Box::new(SelectGoal),
            Box::new(FakePlanner),
            Box::new(FakeExecutor),
            Box::new(VerifyPass),
        )
        .with_world_model(Box::new(wm))
        .with_memory_store(Box::new(mem))
        .with_skill_store(Box::new(skills))
        .run(&collector);

        // One bounded episode, still stops — skill formation never spawns a tick.
        let t = traces(&collector);
        let ids: HashSet<TraceId> = t.iter().map(|x| x.trace_id).collect();
        assert_eq!(ids.len(), 1);

        // Exactly one skill.formation span (operation), alongside the rest.
        let skill_spans: Vec<&CognitiveTrace> = t
            .iter()
            .filter(|x| x.operation == "formation" && x.organ == "skill")
            .collect();
        assert_eq!(skill_spans.len(), 1);
        assert_eq!(skill_spans[0].result, ResultType::Success);

        // 8 spans: root+planner+agent+tool+verification+world_model.update+memory.writeback+skill.formation.
        assert_eq!(t.len(), 8);
    }

    // ── 7E seam 1: skill consultation is surfaced, not auto-executed ──────

    struct FakeProvider {
        skills: Vec<ApplicableSkill>,
    }
    impl SkillProvider for FakeProvider {
        fn applicable_skills(&self, _ctx: &SkillContext) -> Vec<ApplicableSkill> {
            self.skills.clone()
        }
    }

    #[test]
    fn skill_provider_is_consulted_and_surfaces_to_planner() {
        let provider = FakeProvider {
            skills: vec![ApplicableSkill {
                id: "skill-1".into(),
                trajectory: vec!["gather".into(), "synthesize".into()],
            }],
        };

        let collector = InMemoryTraceCollector::new();
        let _tick = AutonomousTick::new(
            1,
            TickTrigger::Scheduled,
            Box::new(ObserveWork),
            Box::new(SelectGoal),
            Box::new(FakePlanner),
            Box::new(FakeExecutor),
            Box::new(VerifyPass),
        )
        .with_skill_provider(Box::new(provider))
        .run(&collector);

        // One bounded episode, still stops — consultation never spawns a tick.
        let t = traces(&collector);
        let ids: HashSet<TraceId> = t.iter().map(|x| x.trace_id).collect();
        assert_eq!(ids.len(), 1);

        // Exactly one `skills.consult` operation is recorded (no new schema fields).
        let consult: Vec<&CognitiveTrace> = t
            .iter()
            .filter(|x| x.operation == "consult" && x.organ == "skills")
            .collect();
        assert_eq!(consult.len(), 1);
        assert_eq!(consult[0].result, ResultType::Success);

        // 6 spans: root+planner+consult+agent+tool+verification. The planner still
        // produced its own steps — the skill did NOT bypass planning.
        assert_eq!(t.len(), 6);
    }

    // ── 7E seam 2: deterministic scheduler drives many bounded ticks ───────

    #[test]
    fn scheduler_driver_runs_multiple_ticks_with_unique_trace_ids() {
        let clock = FakeClock::new();
        let mut p = SchedulerPolicy::allow_all();
        // Unlimited budget: this test exercises failure-isolation + unique trace_ids,
        // not the budget seam (covered by `scheduler::tests::budget_exhaustion_defers`).
        p.max_ticks_window = u64::MAX;
        p.window_ms = u64::MAX;
        let mut sched = Scheduler::new(p, Box::new(clock.clone()));

        let collector = InMemoryTraceCollector::new();
        let mut seen_ids: Vec<TraceId> = Vec::new();

        for i in 0..3 {
            let now = clock.now();
            match sched.decide(now) {
                SchedulerDecision::Allowed(trigger) => {
                    sched.mark_started(now);
                    // Second tick fails on purpose to exercise failure isolation.
                    let verifier: Box<dyn Verifier> = if i == 1 {
                        Box::new(VerifyFail)
                    } else {
                        Box::new(VerifyPass)
                    };
                    let tick = AutonomousTick::new(
                        i,
                        trigger,
                        Box::new(ObserveWork),
                        Box::new(SelectGoal),
                        Box::new(FakePlanner),
                        Box::new(FakeExecutor),
                        verifier,
                    )
                    .run(&collector);
                    seen_ids.push(tick.trace_id);
                    sched.mark_finished(tick.outcome, now);
                }
                other => panic!("expected Allowed, got {:?}", other),
            }
            clock.advance(1);
        }

        // Each scheduled tick got its own unique trace_id — episodes stay independent.
        assert_eq!(seen_ids.len(), 3);
        assert_ne!(seen_ids[0], seen_ids[1]);
        assert_ne!(seen_ids[1], seen_ids[2]);

        let t = traces(&collector);
        let ids: HashSet<TraceId> = t.iter().map(|x| x.trace_id).collect();
        assert_eq!(ids.len(), 3, "three independent episodes");

        // Scheduler survived the mid-run failure and kept allowing ticks.
        assert!(matches!(
            sched.decide(clock.now()),
            SchedulerDecision::Allowed(_)
        ));
    }
    // -- R2-1: plan-grounding revision + stale-plan admission -------------

    #[test]
    fn two_cycle_tick_regrounds_each_plan_against_current_revision() {
        let predicted = WorldSnapshot::new().with_variable("version", serde_json::json!(1));
        let observed = Some(WorldSnapshot::new().with_variable("version", serde_json::json!(1)));
        let shared = Arc::new(SimpleWorldModel::new(predicted, observed));

        let executor = Arc::new(CountingExecutor::default());

        let collector1 = InMemoryTraceCollector::new();
        let t1 = AutonomousTick::new(
            1,
            TickTrigger::Manual,
            Box::new(ObserveWork),
            Box::new(SelectGoal),
            Box::new(FakePlanner),
            Box::new(SharedCountingExecutor(Arc::clone(&executor))),
            Box::new(VerifyPass),
        )
        .with_world_model(Box::new(SharedModel(Arc::clone(&shared))))
        .run(&collector1);
        assert_eq!(t1.outcome, TickOutcome::Success);
        assert_eq!(executor.0.load(std::sync::atomic::Ordering::SeqCst), 1);
        // Tick 1 executed against revision 0 and its verified feedback advanced
        // the world model.
        assert_eq!(shared.current().revision, 1);
        assert!(
            !traces(&collector1)
                .iter()
                .any(|s| s.operation == "stale_plan_rejected"),
            "tick 1 must not be refused"
        );

        let collector2 = InMemoryTraceCollector::new();
        let t2 = AutonomousTick::new(
            2,
            TickTrigger::Manual,
            Box::new(ObserveWork),
            Box::new(SelectGoal),
            Box::new(FakePlanner),
            Box::new(SharedCountingExecutor(Arc::clone(&executor))),
            Box::new(VerifyPass),
        )
        .with_world_model(Box::new(SharedModel(Arc::clone(&shared))))
        .run(&collector2);
        assert_eq!(t2.outcome, TickOutcome::Success);
        assert_eq!(executor.0.load(std::sync::atomic::Ordering::SeqCst), 2);
        // Tick 2 grounded at revision 1 (not stale 0) and advanced it again.
        assert_eq!(shared.current().revision, 2);
        assert!(
            !traces(&collector2)
                .iter()
                .any(|s| s.operation == "stale_plan_rejected"),
            "tick 2 must be admitted because its grounding is current"
        );
    }

    #[test]
    fn stale_plan_is_rejected_without_executor_side_effects() {
        let executor = Arc::new(CountingExecutor::default());
        let counter = Arc::clone(&executor);
        let collector = InMemoryTraceCollector::new();
        let tick = AutonomousTick::new(
            9,
            TickTrigger::Manual,
            Box::new(ObserveWork),
            Box::new(SelectGoal),
            Box::new(FakePlanner),
            Box::new(SharedCountingExecutor(executor)),
            Box::new(VerifyPass),
        )
        .with_world_model(Box::new(AdvancingWorldModel(
            std::sync::atomic::AtomicUsize::new(0),
        )))
        .run(&collector);

        // Refused as failure BEFORE any execution side effect...
        assert_eq!(tick.outcome, TickOutcome::Failure);
        assert_eq!(
            counter.0.load(std::sync::atomic::Ordering::SeqCst),
            0,
            "a stale plan must never reach the executor"
        );

        let t = traces(&collector);
        assert!(
            t.iter()
                .any(|s| s.organ == "execution" && s.operation == "stale_plan_rejected"),
            "the refusal must carry an explicit stale_plan_rejected identity"
        );
        // ...and no verification or world update may be synthesized for work
        // that never ran.
        assert!(
            !t.iter().any(|s| s.operation == "verification_gate"),
            "no verification for a refused plan"
        );
        assert!(
            !t.iter().any(|s| s.organ == "world_model"),
            "no world-model update for a refused plan"
        );
    }
}
