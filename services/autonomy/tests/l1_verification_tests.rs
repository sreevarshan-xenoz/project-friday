//! L1 — verification-evidence ledger qualification.
//!
//! The canonical learning identity is the PREDICTION-DERIVED condition
//! (`ExperienceExtractor::predicted_delta_condition`): goal + the world
//! model's own predicted delta against pre-action belief. Lesson support,
//! verification evidence, and candidate triggers share that exact
//! `LessonCondition`; consultation keeps its observation context (unchanged).
//!
//! Asymmetry preserved: prediction mismatches mint LESSONS; verified
//! successes mint EVIDENCE. Evidence never creates a lesson and never
//! banks against an absent claim.

use std::sync::Arc;

use friday_autonomy::memory::{Experience, ExperienceExtractor, LessonCondition, MemoryStore};
use friday_autonomy::memory_production::ProductionLessonStore;
use friday_autonomy::skill::{SkillAdmission, SkillEvaluator, SkillProvider, SkillStore};
use friday_autonomy::tick::ExecutionResult;
use friday_autonomy::world::PredictionOutcome;
use friday_autonomy::world::{WorldModel, WorldSnapshot};
use friday_autonomy::world_production::ProductionWorldModelAdapter;
use friday_core::trace::VerificationStatus;
use friday_lesson_store::{LessonStore, PruneConfig, RedbLessonStore, RegistryCapabilityResolver};

fn unique_dir(tag: &str) -> std::path::PathBuf {
    static N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = N.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    std::env::temp_dir().join(format!("friday_l1_{tag}_{}_{}", std::process::id(), n))
}

fn docs_registry() -> Arc<friday_capabilities::CapabilityRegistry> {
    let reg = friday_capabilities::CapabilityRegistry::new();
    let cap = friday_capabilities::Capability::new("docs.tools", "Docs", "find the documentation")
        .with_actions(&["find the documentation"]);
    reg.register_provider("docs-svc", vec![cap]);
    Arc::new(reg)
}

fn prod_store(path: &std::path::Path) -> ProductionLessonStore {
    ProductionLessonStore::new(
        Arc::new(RedbLessonStore::open(path, PruneConfig::default()).unwrap()),
        Arc::new(RegistryCapabilityResolver::new(docs_registry())),
        PruneConfig::default(),
    )
}

fn cond(signals: &[&str]) -> LessonCondition {
    LessonCondition {
        action: "find the documentation".into(),
        signals: signals.iter().map(|s| s.to_string()).collect(),
    }
}

/// Admits a corroborated lesson under `condition` through the real extractor
/// path (three mismatch experiences).
fn admit_lesson(store: &ProductionLessonStore, condition: &LessonCondition) {
    let ext = ExperienceExtractor::default();
    let mut predicted = WorldSnapshot::new();
    let actual = WorldSnapshot::new();
    for s in &condition.signals {
        // The projection claimed it; reality never showed it.
        predicted = predicted.with_variable(s.clone(), serde_json::json!(true));
    }
    let diff = predicted.diff(&actual);
    for i in 0..3u64 {
        let e = Experience {
            action: condition.action.clone(),
            predicted: predicted.clone(),
            actual: Some(actual.clone()),
            prediction_error: Some(diff.clone()),
            assessment: PredictionOutcome::Mismatch { diff: diff.clone() },
            observed_at_ms: 1_000 + i,
        };
        ext.process_with_condition(store, &e, condition);
    }
}

// ── 1. unverified execution -> evidence remains 0 ──────────────────────

#[test]
fn l1_unverified_execution_leaves_evidence_at_zero() {
    let dir = unique_dir("t1");
    let store = prod_store(&dir);
    let c = cond(&["docs_read"]);
    admit_lesson(&store, &c);

    assert_eq!(store.verification_count(&c), 0, "nothing verified yet");
    let lesson = store.find_similar_lesson(&c).unwrap();
    let ev = SkillEvaluator::default();
    let cand = ev.form_candidate(&lesson, &c, vec!["traj".into()]);
    match ev.evaluate(&cand) {
        SkillAdmission::Reject(reason) => {
            assert!(reason.contains("verified"), "{reason}");
        }
        other => panic!("unverified candidate must not promote: {other:?}"),
    }
}

// ── 2. one verified execution -> evidence = 1 ──────────────────────────

#[test]
fn l1_one_verified_execution_records_one_evidence() {
    let dir = unique_dir("t2");
    let store = prod_store(&dir);
    let c = cond(&["docs_read"]);
    admit_lesson(&store, &c);

    store.record_verification(&c);
    assert_eq!(store.verification_count(&c), 1);
    let lesson = store.find_similar_lesson(&c).unwrap();
    assert_eq!(lesson.verification_count, 1, "count rides the lesson");
}

// ── 3. N verified executions reach the promotion threshold ─────────────

#[test]
fn l1_verified_executions_reach_promotion_threshold_without_fabrication() {
    let dir = unique_dir("t3");
    let store = prod_store(&dir);
    let c = cond(&["docs_read"]);
    admit_lesson(&store, &c);

    let mut ev = SkillEvaluator::default();
    ev.known_trajectories
        .insert(c.clone(), vec!["docs.search".to_string()]);
    let traj = ev.trajectory_for(&c).unwrap();

    // One verification: below promote_verifications = 2 -> still rejected.
    store.record_verification(&c);
    let lesson = store.find_similar_lesson(&c).unwrap();
    let cand = ev.form_candidate(&lesson, &c, traj.clone());
    assert!(
        matches!(ev.evaluate(&cand), SkillAdmission::Reject(_)),
        "1 < 2 must still reject"
    );

    // Second verification: threshold reached -> promoted, honestly.
    store.record_verification(&c);
    let lesson = store.find_similar_lesson(&c).unwrap();
    assert_eq!(lesson.verification_count, 2);
    let cand = ev.form_candidate(&lesson, &c, traj);
    match ev.evaluate(&cand) {
        SkillAdmission::Promote(s) => {
            assert_eq!(s.verification_count, 2);
            assert_eq!(s.trigger, c);
        }
        other => panic!("threshold met but not promoted: {other:?}"),
    }
}

// ── 4. different condition, same action class -> no contamination ──────

#[test]
fn l1_same_action_class_different_conditions_stay_independent() {
    let dir = unique_dir("t4");
    let store = prod_store(&dir);
    let a = cond(&["signal_a"]);
    let b = cond(&["signal_b"]);
    admit_lesson(&store, &a);
    admit_lesson(&store, &b);

    store.record_verification(&a);
    store.record_verification(&a);

    assert_eq!(store.verification_count(&a), 2, "A earned its credit");
    assert_eq!(
        store.verification_count(&b),
        0,
        "B must not inherit A's evidence (same class, different condition)"
    );
}

// ── 5. restart mid-accumulation -> evidence survives ───────────────────

#[test]
fn l1_verification_evidence_survives_store_restart() {
    let dir = unique_dir("t5");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("lessons.redb");
    {
        let store = prod_store(&path);
        let c = cond(&["docs_read"]);
        admit_lesson(&store, &c);
        store.record_verification(&c);
        assert_eq!(store.verification_count(&c), 1);
    } // durable owner dropped

    let reopened = prod_store(&path);
    let c = cond(&["docs_read"]);
    assert_eq!(
        reopened.verification_count(&c),
        1,
        "durable evidence survives restart"
    );
    reopened.record_verification(&c);
    assert_eq!(reopened.verification_count(&c), 2, "accumulation continues");
}

// ── 6. prediction-derived condition identical across both uses ─────────

struct ArcModel(Arc<ProductionWorldModelAdapter>);

impl WorldModel for ArcModel {
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
    ) -> friday_autonomy::world::PredictionOutcome {
        self.0.update(predicted, actual)
    }
}

#[test]
fn l1_prediction_derived_condition_is_the_single_learning_identity() {
    use friday_autonomy::orchestrator::{
        AutonomousTick, Executor, GoalSelector, ObservationSource, Planner as TickPlanner,
    };
    use friday_autonomy::tick::{ExecutionResult, Observation, Plan, TickTrigger};
    use friday_core::trace::{
        CognitiveTraceContext, InMemoryTraceCollector, ResultType, SpanId, TraceCollector,
    };

    struct Obs;
    impl ObservationSource for Obs {
        fn observe(&self) -> Observation {
            Observation {
                summary: "inbox".into(),
                signals: vec![],
            }
        }
    }
    struct FixedGoal;
    impl GoalSelector for FixedGoal {
        fn select_goal(&self, _o: &Observation) -> Option<String> {
            Some("find the documentation".into())
        }
    }
    struct FakePlanner;
    impl TickPlanner for FakePlanner {
        fn plan(&self, goal: &str, _s: &[friday_autonomy::skill::ApplicableSkill]) -> Plan {
            Plan {
                goal: goal.into(),
                steps: vec!["step".into()],
            }
        }
    }
    struct Exec;
    impl Executor for Exec {
        fn execute(
            &self,
            ctx: &CognitiveTraceContext,
            col: &dyn TraceCollector,
            _p: &Plan,
        ) -> ExecutionResult {
            col.record(
                ctx.trace(SpanId::default(), "agents", "execute")
                    .with_timing(chrono::Utc::now(), 1)
                    .with_result(ResultType::Success),
            );
            ExecutionResult {
                success: true,
                detail: "ok".into(),
            }
        }
    }

    // Mismatch world: the causal projection claims docs_read=true while live
    // state never shows it — every tick is a mismatch, verifier passes.
    let mut m = friday_goal_execution::WorldModel::new(friday_goal_execution::WorldState::new());
    m.causal_mut()
        .add_edge(friday_goal_execution::CausalEdge::new(
            "docs-effect",
            friday_goal_execution::Capability::Custom("find the documentation".to_string()),
            friday_goal_execution::CausalCondition::Always,
            vec![friday_goal_execution::CausalEffect::VariableSet {
                key: "docs_read".into(),
                value: serde_json::json!(true),
            }],
        ));
    let world = Arc::new(ProductionWorldModelAdapter::from_model(m));

    let redb_path = unique_dir("t6");
    let redb = Arc::new(RedbLessonStore::open(&redb_path, PruneConfig::default()).unwrap());
    let memory = ProductionLessonStore::new(
        redb.clone() as Arc<dyn LessonStore>,
        Arc::new(RegistryCapabilityResolver::new(docs_registry())),
        PruneConfig::default(),
    );
    let skills =
        std::sync::Arc::new(friday_autonomy::skill_production::ProductionSkillStore::new());

    let tick = AutonomousTick::new(
        6,
        TickTrigger::Manual,
        Box::new(Obs),
        Box::new(FixedGoal),
        Box::new(FakePlanner),
        Box::new(Exec),
        Box::new(VerifyPassLike),
    )
    .with_world_model(Box::new(ArcModel(Arc::clone(&world))))
    .with_memory_store(Box::new(ProductionLessonStore::new(
        redb.clone() as Arc<dyn LessonStore>,
        Arc::new(RegistryCapabilityResolver::new(docs_registry())),
        PruneConfig::default(),
    )))
    .with_skill_store(Box::new(SkillProviderBox(skills.clone())));

    // Three ticks: corroboration reaches 3 AND three passed verifications
    // accumulate against the SAME canonical condition. No manual evidence.
    for _ in 0..3 {
        let rec = tick.run(&InMemoryTraceCollector::new());
        assert_eq!(rec.outcome, friday_autonomy::tick::TickOutcome::Success);
    }

    // The canonical condition (prediction delta of this goal).
    let pre = world.observe().unwrap_or_default();
    let pred = world.predict("find the documentation");
    let canonical =
        ExperienceExtractor::predicted_delta_condition("find the documentation", &pred, &pre);

    // I-identity: verification evidence landed on exactly that condition...
    assert_eq!(
        memory.verification_count(&canonical),
        3,
        "every passed verification credited the canonical condition"
    );
    // ...and the candidate formed from the SAME condition carries it.
    let lesson = memory
        .find_similar_lesson(&canonical)
        .expect("lesson exists");
    assert_eq!(lesson.support_count, 3);
    assert_eq!(lesson.verification_count, 3);
    let mut ev = SkillEvaluator::default();
    ev.known_trajectories
        .insert(canonical.clone(), vec!["director.delegation".to_string()]);
    let traj = ev.trajectory_for(&canonical).unwrap();
    let cand = ev.form_candidate(&lesson, &canonical, traj);
    match ev.evaluate(&cand) {
        SkillAdmission::Promote(s) => {
            assert_eq!(s.verification_count, 3);
            // The tick itself could not admit (its default evaluator knows
            // no trajectory); admission goes through the same store the
            // tick consults — the closed learning loop.
            use friday_autonomy::skill::SkillStore as _;
            skills.admit(s);
            assert_eq!(skills.skill_count(), 1, "admitted into production");
        }
        other => panic!("real evidence must satisfy admission: {other:?}"),
    }

    // Subset regression: diverging keys ⊆ predicted-delta keys.
    let diff = pre.diff(&pred);
    for k in &diff.added {
        assert!(canonical.signals.contains(k));
    }
}

// thin forwarders so the tick can own boxed traits over shared stores
struct VerifyPassLike;
#[async_trait::async_trait]
impl friday_autonomy::orchestrator::Verifier for VerifyPassLike {
    fn verify(&self, _g: &str, _r: &ExecutionResult) -> VerificationStatus {
        VerificationStatus::Passed
    }
}

#[derive(Clone)]
struct SkillProviderBox(std::sync::Arc<friday_autonomy::skill_production::ProductionSkillStore>);

impl SkillStore for SkillProviderBox {
    fn admit(&self, s: friday_autonomy::skill::Skill) {
        self.0.admit(s)
    }
    fn find_applicable(&self, t: &LessonCondition) -> Option<friday_autonomy::skill::Skill> {
        self.0.find_applicable(t)
    }
    fn strengthen(&self, t: &LessonCondition) {
        self.0.strengthen(t)
    }
    fn weaken(&self, t: &LessonCondition) {
        self.0.weaken(t)
    }
    fn disable(&self, t: &LessonCondition) {
        self.0.disable(t)
    }
}

impl SkillProvider for SkillProviderBox {
    fn applicable_skills(
        &self,
        ctx: &friday_autonomy::skill::SkillContext,
    ) -> Vec<friday_autonomy::skill::ApplicableSkill> {
        self.0.applicable_skills(ctx)
    }
}
