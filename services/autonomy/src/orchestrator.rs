//! 7A orchestrator — drives one bounded cognitive tick.
//!
//! `AutonomousTick::run` performs exactly one observation → (goal) → plan →
//! execute → verify → record → STOP cycle and returns a `CognitiveTick`. It does
//! not loop, does not schedule, and does not spawn further ticks. The caller may
//! invoke `run` again later (a future scheduler, 7E), but each call creates its
//! own `trace_id` — there is no hidden recursion.
//!
//! Stage behaviors are injected as traits so the real planner/director/verifier
//! (7B+) plug in without changing the bounded control flow proven here.

use chrono::Utc;
use friday_core::trace::{
    CognitiveTraceContext, GoalId, ResultType, SessionId, SpanId, TraceCollector, TraceId,
    VerificationStatus,
};

use crate::tick::{
    CognitiveTick, ExecutionResult, NextAction, Observation, Plan, TickOutcome, TickTrigger,
};

/// What the tick observes about the world before deciding.
pub trait ObservationSource: Send + Sync {
    fn observe(&self) -> Observation;
}

/// Turns an observation into at most one bounded goal. `None` ⇒ no-op tick.
pub trait GoalSelector: Send + Sync {
    fn select_goal(&self, obs: &Observation) -> Option<String>;
}

/// Produces a plan for a chosen goal.
pub trait Planner: Send + Sync {
    fn plan(&self, goal: &str) -> Plan;
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
        }
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
            Some(goal) => self.run_episode(collector, session, goal_id, trace_id, observation, goal),
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

        // Planner span (child of the goal root).
        collector.record(
            planner_ctx
                .trace(planner_span, "planner", "create_plan")
                .with_timing(Utc::now(), 10)
                .with_result(ResultType::Success),
        );

        // Execute — the executor records its own agent/tool spans under the planner.
        let plan = self.planner.plan(&goal);
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
    use friday_core::trace::{
        CognitiveTrace, InMemoryTraceCollector, TraceCollector, VerificationStatus,
        reconstruct_trace_tree,
    };
    use friday_observability::metrics::CognitiveMetrics;
    use friday_observability::viewer::ViewerModel;
    use std::collections::{HashMap, HashSet};

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
        fn plan(&self, goal: &str) -> Plan {
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
        assert_eq!(t.len(), 5, "one episode: root+planner+agent+tool+verification");

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
}
