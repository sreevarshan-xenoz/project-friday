//! 8B final gate — golden integration test.
//!
//! Proves the full production stack works end-to-end through a real bounded
//! autonomous tick (no `RedbLessonStore::admit` is called directly here — the tick
//! itself must reach the durable store):
//!
//! ```text
//! AutonomousTick
//!    ├── real ProductionWorldModelAdapter
//!    ├── ExperienceExtractor
//!    ├── ProductionLessonStore
//!    │       └── RedbLessonStore
//!    └── CognitiveTrace
//! ```
//!
//! Two cases (so the test cannot accidentally prove only the happy path):
//!   * Case A — prediction correct  ⇒ Experience produced, NO lesson admitted.
//!   * Case B — same prediction error repeated enough ⇒ lesson admitted,
//!     persisted, dropped/reopened, retrieved — with `action_class` sourced from
//!     the Capability Registry, never fabricated.

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use serde_json::json;

use friday_capabilities::CapabilityRegistry;
use friday_core::trace::{
    reconstruct_trace_tree, CognitiveTraceContext, InMemoryTraceCollector, ResultType, SpanId,
    TraceCollector,
};
use friday_goal_execution::{
    Capability, CausalCondition, CausalEdge, CausalEffect, WorldModel as ProdWorldModel, WorldState,
};
use friday_lesson_store::{
    LessonKey, LessonStore, MatchPolicy, PruneConfig, RedbLessonStore, RegistryCapabilityResolver,
};

use crate::memory::MemoryStore;
use crate::memory_production::ProductionLessonStore;
use crate::orchestrator::{
    AutonomousTick, Executor, GoalSelector, ObservationSource, Planner, Verifier,
};
use crate::skill::ApplicableSkill;
use crate::tick::{ExecutionResult, NextAction, Observation, Plan, TickTrigger};
use crate::world::WorldModel;
use crate::world_production::ProductionWorldModelAdapter;

static COUNTER: AtomicU64 = AtomicU64::new(0);

fn unique_temp() -> PathBuf {
    let n = COUNTER.fetch_add(1, Ordering::SeqCst);
    std::env::temp_dir().join(format!("friday_golden_8b_{}_{}", std::process::id(), n))
}

/// Register `deploy` → capability id `deploy.tools` so the resolver maps the raw
/// goal string to a canonical registry id (the proof for assertion 9).
fn make_registry() -> CapabilityRegistry {
    let reg = CapabilityRegistry::new();
    let cap = friday_capabilities::Capability::new("deploy.tools", "Deploy Tools", "deploy")
        .with_actions(&["deploy"]);
    reg.register_provider("deploy-svc", vec![cap]);
    reg
}

/// Production world model wired with a causal edge so that simulating
/// `Custom("deploy")` projects a change (variable `deployed` set), while the live
/// `state()` stays unchanged → a predicted/observed Mismatch.
fn deploy_world() -> ProductionWorldModelAdapter {
    let mut m = ProdWorldModel::new(WorldState::new());
    m.causal_mut().add_edge(CausalEdge::new(
        "deploy-effect",
        Capability::Custom("deploy".to_string()),
        CausalCondition::Always,
        vec![CausalEffect::VariableSet {
            key: "deployed".into(),
            value: json!(true),
        }],
    ));
    ProductionWorldModelAdapter::from_model(m)
}

/// Plain world model: simulating any unknown capability is inapplicable, so
/// predict == observe ⇒ Correct. Used for Case A.
fn plain_world() -> ProductionWorldModelAdapter {
    ProductionWorldModelAdapter::from_model(ProdWorldModel::new(WorldState::new()))
}

// ── Fakes ──────────────────────────────────────────────────────────────

struct Obs;
impl ObservationSource for Obs {
    fn observe(&self) -> Observation {
        Observation {
            summary: "inbox has actionable tasks".into(),
            signals: vec![],
        }
    }
}

struct FixedGoal(&'static str);
impl GoalSelector for FixedGoal {
    fn select_goal(&self, _obs: &Observation) -> Option<String> {
        Some(self.0.into())
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

struct FakeExecutor;
impl Executor for FakeExecutor {
    fn execute(
        &self,
        ctx: &CognitiveTraceContext,
        collector: &dyn TraceCollector,
        _plan: &Plan,
    ) -> ExecutionResult {
        let agent = SpanId::default();
        collector.record(
            ctx.trace(agent, "agents", "execute")
                .with_timing(chrono::Utc::now(), 8)
                .with_result(ResultType::Success),
        );
        let tool_ctx = ctx.child(agent);
        collector.record(
            tool_ctx
                .trace(SpanId::default(), "web", "tool_call")
                .with_timing(chrono::Utc::now(), 4)
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
    fn verify(
        &self,
        _goal: &str,
        _result: &ExecutionResult,
    ) -> friday_core::trace::VerificationStatus {
        friday_core::trace::VerificationStatus::Passed
    }
}

fn build_tick(
    goal: &'static str,
    world: ProductionWorldModelAdapter,
    memory: Box<dyn MemoryStore>,
) -> AutonomousTick {
    AutonomousTick::new(
        1,
        TickTrigger::Manual,
        Box::new(Obs),
        Box::new(FixedGoal(goal)),
        Box::new(FakePlanner),
        Box::new(FakeExecutor),
        Box::new(VerifyPass),
    )
    .with_world_model(Box::new(world) as Box<dyn WorldModel>)
    .with_memory_store(memory)
}

// ── Case A: correct prediction earns no lesson ─────────────────────────

#[test]
fn golden_8b_case_a_correct_prediction_admits_no_lesson() {
    let tmp = unique_temp();
    let registry = Arc::new(make_registry());
    let resolver = Arc::new(RegistryCapabilityResolver::new(registry));

    let store_arc =
        Arc::new(RedbLessonStore::open(&tmp, PruneConfig::default()).expect("open redb"));
    let prod = ProductionLessonStore::new(
        store_arc.clone() as Arc<dyn LessonStore>,
        resolver,
        PruneConfig::default(),
    );

    // Goal that simulates inapplicable ⇒ Correct prediction (no mismatch).
    let tick = build_tick("noop", plain_world(), Box::new(prod));

    let mut trace_ids = HashSet::new();
    for i in 0..3 {
        let collector = InMemoryTraceCollector::new();
        let rec = tick.run(&collector);
        assert_eq!(rec.next_action, NextAction::Stop, "tick {} must stop", i);
        let trs = collector.traces();
        let ids: HashSet<_> = trs.iter().map(|t| t.trace_id).collect();
        assert_eq!(ids.len(), 1, "tick {}: exactly one trace_id", i);
        // An Experience was produced (writeback span present)…
        assert!(
            trs.iter()
                .any(|t| t.organ == "memory" && t.operation == "writeback"),
            "tick {}: memory.writeback recorded (Experience produced)",
            i
        );
        trace_ids.insert(rec.trace_id);
    }
    assert_eq!(trace_ids.len(), 3, "three bounded episodes");

    // …yet NO lesson is admitted. Memory is earned through evidence, not produced
    // by a correct execution.
    let none = LessonKey::new("noop", Vec::<String>::new());
    let hits = store_arc
        .find_similar(&none, MatchPolicy::Exact)
        .expect("query");
    assert!(
        hits.is_empty(),
        "correct prediction must NOT admit a lesson"
    );
}

// ── Case B: repeated mismatch earns a lesson that survives a restart ───

#[test]
fn golden_8b_case_b_mismatch_earns_lesson_and_persists_across_restart() {
    let tmp = unique_temp();
    let registry = Arc::new(make_registry());
    let resolver = Arc::new(RegistryCapabilityResolver::new(registry.clone()));

    let store_arc =
        Arc::new(RedbLessonStore::open(&tmp, PruneConfig::default()).expect("open redb"));
    let prod = ProductionLessonStore::new(
        store_arc.clone() as Arc<dyn LessonStore>,
        resolver.clone(),
        PruneConfig::default(),
    );

    let tick = build_tick("deploy", deploy_world(), Box::new(prod));

    // The canonical key the lesson must be stored under: action_class comes from
    // the registry id "deploy.tools", and the diverging signal is "deployed".
    let earned_key = LessonKey::new("deploy.tools", ["deployed"]);
    let fabricated_key = LessonKey::new("deploy", ["deployed"]);

    for i in 0..3 {
        let collector = InMemoryTraceCollector::new();
        let rec = tick.run(&collector);
        assert_eq!(rec.next_action, NextAction::Stop, "tick {} must stop", i);
        let trs = collector.traces();
        let ids: HashSet<_> = trs.iter().map(|t| t.trace_id).collect();
        assert_eq!(ids.len(), 1, "tick {}: exactly one trace_id", i);
        assert_eq!(ids.iter().next().copied(), Some(rec.trace_id));

        // Trace reconstructs with no orphan spans (frozen schema intact).
        let tree = reconstruct_trace_tree(&trs);
        assert_eq!(tree.len(), 1, "tick {}: single rooted tree", i);

        // Prediction error ⇒ world_model.update span exists; Experience ⇒ writeback.
        assert!(
            trs.iter()
                .any(|t| t.organ == "world_model" && t.operation == "update"),
            "tick {}: world_model.update recorded (prediction error)",
            i
        );
        assert!(
            trs.iter()
                .any(|t| t.organ == "memory" && t.operation == "writeback"),
            "tick {}: memory.writeback recorded (Experience produced)",
            i
        );

        // Corroboration must accumulate; the lesson only exists after the threshold.
        let present = || {
            !store_arc
                .find_similar(&earned_key, MatchPolicy::Exact)
                .expect("query")
                .is_empty()
        };
        if i < 2 {
            assert!(
                !present(),
                "tick {}: not yet admitted (evidence not enough)",
                i
            );
        } else {
            assert!(present(), "tick {}: admitted once corroborated", i);
        }
    }

    // (5/6) The production store contains the earned lesson.
    let live = store_arc
        .find_similar(&earned_key, MatchPolicy::Exact)
        .expect("query");
    assert_eq!(live.len(), 1, "lesson present in production store");
    assert_eq!(live[0].support_count, 3, "corroborated to threshold count");

    // (9) action_class is the REGISTRY id, not the fabricated raw text.
    let fabricated = store_arc
        .find_similar(&fabricated_key, MatchPolicy::Exact)
        .expect("query");
    assert!(
        fabricated.is_empty(),
        "lesson must NOT be stored under the raw action text"
    );

    // (7/8) Drop the live store and REOPEN the same file; the lesson survives.
    drop(tick);
    drop(store_arc);

    let reopened = RedbLessonStore::open(&tmp, PruneConfig::default()).expect("reopen redb");
    let recovered = reopened
        .find_similar(&earned_key, MatchPolicy::Exact)
        .expect("reopen query");
    assert_eq!(recovered.len(), 1, "lesson survives process restart");
    assert_eq!(recovered[0].key.action_class, "deploy.tools");
    assert!(recovered[0].key.signals.contains("deployed"));

    // (12) The frozen CognitiveTrace schema is unchanged: the recovered proof is a
    // production lesson, not trace data — the trace contract was never touched.
    assert_eq!(recovered[0].provenance.action_raw, "deploy");
}
