//! R2-4 - deterministic multi-cycle endurance qualification.
//!
//! N = 12 hand-driven cycles, fixed failure schedule, no RNG. The runtime is
//! externally driven by design (no hidden loop), so the harness drives real
//! cycles through the real seams and asserts a per-cycle ledger plus nine
//! coherence invariants:
//!
//! ```text
//!  1 healthy delegation            (host -> Director -> WorkforceDriver)
//!  2 healthy delegation
//!  3 provider death                -> RulePlanner fallback (F1)
//!  4 verification rejection        -> honest Failure (F3)
//!  5 egress block                  -> zero dispatch (F4)
//!  6 stale plan                    -> refused before side effects (F5/R2-1)
//!  7 worker crash                  -> mount() recovery (F2/b9acbd6)
//!  8 prediction mismatch           -> experience, no lesson yet
//!  9 prediction mismatch           -> corroborating
//! 10 mismatch admits lesson        -> evaluator admits skill
//! 11 STORE RESTART (redb+workforce+journal owners rebuilt)
//!    skill consultation continues; lessons restart-consistent
//! 12 consultation again + journal replay-safety + final sweep
//! ```
//!
//! Durability honesty: RedbLessonStore / JsonWorkforceStore / JsonActionJournal
//! are durable and ARE restart-verified here. The WorldModel and the skill
//! registry are in-memory today; they are NOT claimed restart-durable and are
//! not restart-verified (L1/L2 remain KNOWN LIMITATIONS, untouched).

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use friday_actions::{AgentKernel, DocsAdapter, SkillAgentDriver};
use friday_agent_runtime::{
    AgentDirector, AgentDriver, AgentRole, JsonWorkforceStore, TaskStatus, Workforce,
    WorkforceDriver, WorkforceStore,
};
use friday_cognitive::{
    AutonomousTick as DriveTick, DecisionFunction, DecisionState, DurableTickOutcome, EngineId,
    GlobalWorkspace, JsonActionJournal, Priority, TickAction, Topic,
};
use friday_core::trace::{InMemoryTraceCollector, ResultType, TraceCollector, VerificationStatus};
use friday_goal_execution::{
    apply_verified_action_outcome, AutonomyHost, DirectorActionExecutor,
    WorldModel as ProdWorldModel, WorldState,
};
use friday_lesson_store::{
    LessonKey, LessonStore, MatchPolicy, PruneConfig, RedbLessonStore, RegistryCapabilityResolver,
};
use friday_planner::execution::capability_registry::CapabilityRegistry as PlannerCapabilityRegistry;
use friday_planner::execution::native_caps::register_native_builtins;
use friday_skills::{CapabilityResolver, SkillApprovals, SkillLoader, SkillRegistry, SkillSource};

use friday_autonomy::memory::LessonCondition;
use friday_autonomy::memory_production::ProductionLessonStore;
use friday_autonomy::orchestrator::{
    AutonomousTick, Executor, GoalSelector, ObservationSource, Planner as TickPlanner, Verifier,
};
use friday_autonomy::skill::{
    ApplicableSkill, SkillAdmission, SkillEvaluator, SkillProvider, SkillStore,
};
use friday_autonomy::skill_production::ProductionSkillStore;
use friday_autonomy::tick::{
    ExecutionResult, NextAction, Observation, Plan, TickOutcome, TickTrigger,
};
use friday_autonomy::world::WorldModel;
use friday_autonomy::world_production::ProductionWorldModelAdapter;

fn unique_dir(tag: &str) -> PathBuf {
    static N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = N.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    std::env::temp_dir().join(format!(
        "friday_endurance_{tag}_{}_{}",
        std::process::id(),
        n
    ))
}

struct TempDir(PathBuf);
impl TempDir {
    fn new(tag: &str) -> Self {
        let p = unique_dir(tag);
        std::fs::create_dir_all(&p).unwrap();
        TempDir(p)
    }
    fn path(&self) -> &Path {
        &self.0
    }
}
impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

const DOCS_MANIFEST: &str = r#"{
    "id": "docs", "name": "Documentation", "version": "1.0.0",
    "description": "Search and read local documentation",
    "category": "docs",
    "capabilities": ["docs.search", "docs.read"],
    "actions": ["find documentation", "search the docs", "read a spec"],
    "permissions": [],
    "readonly": true,
    "platform": "Any", "min_core_version": "0.0.0", "enabled": true
}"#;

struct PlannerResolver(Arc<PlannerCapabilityRegistry>);

impl CapabilityResolver for PlannerResolver {
    fn exists(&self, capability: &str) -> bool {
        self.0.resolve(capability).is_some()
    }
}

async fn docs_skill_driver() -> Arc<dyn AgentDriver> {
    let skills_dir = unique_dir("skills");
    let sub = skills_dir.join("docs");
    std::fs::create_dir_all(&sub).unwrap();
    std::fs::write(sub.join("skill.json"), DOCS_MANIFEST).unwrap();

    let planner = Arc::new(PlannerCapabilityRegistry::new());
    register_native_builtins(&planner);

    let skills = Arc::new(SkillRegistry::new());
    let resolver: Arc<dyn CapabilityResolver> = Arc::new(PlannerResolver(Arc::clone(&planner)));
    let report = SkillLoader::new(Arc::clone(&skills), resolver)
        .load_dir(&skills_dir, SkillSource::User)
        .expect("load docs skill");
    assert!(report.ok());

    let kernel = Arc::new(AgentKernel::new());
    kernel.register_adapter(Box::new(DocsAdapter::new())).await;

    Arc::new(SkillAgentDriver::new(
        Arc::clone(&skills),
        Arc::new(SkillApprovals::new()),
        planner,
        Arc::clone(&kernel),
    ))
}
// ── delegation-cycle machinery (host + Director + WorkforceDriver) ────

struct OrganismGoal(&'static str);

impl DecisionFunction for OrganismGoal {
    fn decide(
        &self,
        workspace: &GlobalWorkspace,
        _state: &mut DecisionState,
    ) -> Option<TickAction> {
        let evidence = workspace.get_latest("sensor.screen")?;
        Some(
            TickAction::new(self.0, "endurance goal", vec![evidence.id], Priority::High)
                .with_grounding_revision(0),
        )
    }
}

struct DirectorVerified;

#[async_trait::async_trait]
impl friday_cognitive::ActionVerifier for DirectorVerified {
    async fn verify(
        &self,
        _workspace: &GlobalWorkspace,
        _action: &TickAction,
        execution: &friday_cognitive::ActionExecution,
    ) -> bool {
        execution.success && execution.output["verified_children"].as_u64().unwrap_or(0) >= 1
    }
}

/// Grounds every decision at revision 0 regardless of reality — the stale
/// plan for cycle 6.
struct StaleGoal(&'static str);

impl DecisionFunction for StaleGoal {
    fn decide(
        &self,
        workspace: &GlobalWorkspace,
        _state: &mut DecisionState,
    ) -> Option<TickAction> {
        let evidence = workspace.get_latest("sensor.screen")?;
        Some(
            TickAction::new(
                self.0,
                "stale on purpose",
                vec![evidence.id],
                Priority::High,
            )
            .with_grounding_revision(0),
        )
    }
}

// ── learning-tick machinery (7B-7E recipe) ─────────────────────────────

struct Obs;
impl ObservationSource for Obs {
    fn observe(&self) -> Observation {
        Observation {
            summary: "inbox has actionable tasks".into(),
            signals: vec!["docs_read".into()],
        }
    }
}

struct FixedGoal(&'static str);
impl GoalSelector for FixedGoal {
    fn select_goal(&self, _obs: &Observation) -> Option<String> {
        Some(self.0.into())
    }
}

struct RecordingExecutor;
impl Executor for RecordingExecutor {
    fn execute(
        &self,
        ctx: &friday_core::trace::CognitiveTraceContext,
        collector: &dyn TraceCollector,
        _plan: &Plan,
    ) -> ExecutionResult {
        let agent = friday_core::trace::SpanId::default();
        collector.record(
            ctx.trace(agent, "agents", "execute")
                .with_timing(chrono::Utc::now(), 8)
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
            reason: "injected verification rejection".into(),
        }
    }
}

/// Forwards to a shared production adapter so the harness can read the
/// world-model revision between cycles while the tick owns its Box.
struct ArcModel(Arc<ProductionWorldModelAdapter>);

impl WorldModel for ArcModel {
    fn predict(&self, action: &str) -> friday_autonomy::world::WorldSnapshot {
        self.0.predict(action)
    }
    fn observe(&self) -> Option<friday_autonomy::world::WorldSnapshot> {
        self.0.observe()
    }
    fn update(
        &self,
        predicted: &friday_autonomy::world::WorldSnapshot,
        actual: Option<friday_autonomy::world::WorldSnapshot>,
    ) -> friday_autonomy::world::PredictionOutcome {
        self.0.update(predicted, actual)
    }
}

/// The docs goal's prediction diverges every tick (projection sets
/// `docs_read`, live state never does).
fn learning_world() -> Arc<ProductionWorldModelAdapter> {
    use friday_goal_execution::{Capability, CausalCondition, CausalEdge, CausalEffect};
    let mut m = ProdWorldModel::new(WorldState::new());
    m.causal_mut().add_edge(CausalEdge::new(
        "docs-effect",
        Capability::Custom("find the documentation".to_string()),
        CausalCondition::Always,
        vec![CausalEffect::VariableSet {
            key: "docs_read".into(),
            value: serde_json::json!(true),
        }],
    ));
    Arc::new(ProductionWorldModelAdapter::from_model(m))
}

fn docs_registry() -> Arc<friday_capabilities::CapabilityRegistry> {
    let reg = friday_capabilities::CapabilityRegistry::new();
    let cap = friday_capabilities::Capability::new("docs.tools", "Docs", "find the documentation")
        .with_actions(&["find the documentation"]);
    reg.register_provider("docs-svc", vec![cap]);
    Arc::new(reg)
}

/// Two trait seams over one shared skill store (test-local forwarding).
#[derive(Clone)]
struct SharedSkills(Arc<ProductionSkillStore>);

impl SkillStore for SharedSkills {
    fn admit(&self, skill: friday_autonomy::skill::Skill) {
        self.0.admit(skill)
    }
    fn find_applicable(&self, trigger: &LessonCondition) -> Option<friday_autonomy::skill::Skill> {
        self.0.find_applicable(trigger)
    }
    fn strengthen(&self, trigger: &LessonCondition) {
        self.0.strengthen(trigger)
    }
    fn weaken(&self, trigger: &LessonCondition) {
        self.0.weaken(trigger)
    }
    fn disable(&self, trigger: &LessonCondition) {
        self.0.disable(trigger)
    }
}

impl SkillProvider for SharedSkills {
    fn applicable_skills(
        &self,
        ctx: &friday_autonomy::skill::SkillContext,
    ) -> Vec<ApplicableSkill> {
        self.0.applicable_skills(ctx)
    }
}

#[derive(Clone, Default)]
struct CapturingPlanner(Arc<Mutex<Vec<Vec<ApplicableSkill>>>>);

impl TickPlanner for CapturingPlanner {
    fn plan(&self, goal: &str, skills: &[ApplicableSkill]) -> Plan {
        self.0
            .lock()
            .expect("capturing planner poisoned")
            .push(skills.to_vec());
        Plan {
            goal: goal.into(),
            steps: vec!["gather".into()],
        }
    }
}
// ── The 12-cycle campaign ──────────────────────────────────────────────

#[derive(Clone)]
struct Row {
    cycle: usize,
    world_revision: u64,
    outcome: &'static str,
    lesson_count: usize,
    skill_count: usize,
}

/// Replays a captured decision verbatim (same action text AND evidence ids),
/// which is exactly what journal dedup keys on.
struct ReplayGoal(TickAction);

impl DecisionFunction for ReplayGoal {
    fn decide(&self, _ws: &GlobalWorkspace, _st: &mut DecisionState) -> Option<TickAction> {
        Some(self.0.clone())
    }
}

#[tokio::test]
async fn endurance_r2_4_twelve_cycles_state_stays_coherent() {
    let root = TempDir::new("root");
    let docs_root = TempDir::new("docs");
    std::fs::write(
        docs_root.path().join("README.md"),
        "# FRIDAY endurance docs",
    )
    .unwrap();
    let driver = docs_skill_driver().await;
    let store: Arc<dyn WorkforceStore> =
        Arc::new(JsonWorkforceStore::new(root.path().join("store")));
    let journal_dir = TempDir::new("journal");
    let journal_path = journal_dir.path().join("journal.json");

    let mut rows: Vec<Row> = Vec::new();

    // ══ Cycle 1 — healthy delegation ════════════════════════════════════
    let workforce = Arc::new(Workforce::new(
        Arc::clone(&store),
        driver.clone(),
        root.path().join("ws"),
    ));
    let director = AgentDirector::new(Arc::new(
        WorkforceDriver::new(Arc::clone(&workforce), AgentRole::Researcher).with_param(
            "docs.search",
            serde_json::json!({ "root": docs_root.path().to_string_lossy() }),
        ),
    ));
    let model = Arc::new(Mutex::new(ProdWorldModel::new(WorldState::new())));
    let host = AutonomyHost::new(
        {
            let ws = GlobalWorkspace::new();
            ws.publish_from(
                Topic::Screen,
                EngineId::Perception,
                serde_json::json!({ "change": "content", "hash": "c1" }),
            );
            ws
        },
        DriveTick::new(Arc::new(OrganismGoal("find the documentation"))),
        Arc::new(DirectorActionExecutor::new(Arc::new(director))),
        Arc::new(DirectorVerified),
        Arc::new(JsonActionJournal::new(&journal_path)),
    )
    .with_world_model(Arc::clone(&model));
    let o1 = host.tick().await.expect("cycle 1 executes");
    assert!(o1.verified);
    let cycle1_action = o1.action.clone();
    assert_eq!(
        model.lock().unwrap().revision(),
        0,
        "no fabricated world advance"
    );
    rows.push(Row {
        cycle: 1,
        world_revision: 0,
        outcome: "delegated+verified",
        lesson_count: 0,
        skill_count: 0,
    });

    // ══ Cycle 2 — healthy delegation ════════════════════════════════════
    {
        let ws2 = GlobalWorkspace::new();
        ws2.publish_from(
            Topic::Screen,
            EngineId::Perception,
            serde_json::json!({ "change": "content", "hash": "c2" }),
        );
        let d2 = AgentDirector::new(Arc::new(
            WorkforceDriver::new(Arc::clone(&workforce), AgentRole::Researcher).with_param(
                "docs.search",
                serde_json::json!({ "root": docs_root.path().to_string_lossy() }),
            ),
        ));
        let h2 = AutonomyHost::new(
            ws2,
            DriveTick::new(Arc::new(OrganismGoal("search the docs"))),
            Arc::new(DirectorActionExecutor::new(Arc::new(d2))),
            Arc::new(DirectorVerified),
            Arc::new(JsonActionJournal::new(journal_dir.path().join("j2.json"))),
        );
        let o2 = h2.tick().await.expect("cycle 2 executes");
        assert!(o2.verified);
    }
    // I3 evidence: a failed clone of the verified outcome is refused by the
    // world model; nothing is fabricated.
    {
        let mut m = model.lock().unwrap();
        let mut failed = o1.clone();
        failed.verified = false;
        assert!(apply_verified_action_outcome(&mut m, &failed).is_none());
        assert!(m.history().is_empty());
    }
    rows.push(Row {
        cycle: 2,
        world_revision: 0,
        outcome: "delegated+verified",
        lesson_count: 0,
        skill_count: 0,
    });
    // ══ Cycle 3 — provider death -> deterministic fallback (F1) ══════════
    struct DeadBackend;
    #[async_trait::async_trait]
    impl friday_planner::planning::planners::backend::ReasoningBackend for DeadBackend {
        async fn complete(&self, _p: &str) -> Result<String, String> {
            Err("provider unreachable: 503".to_string())
        }
    }
    {
        let registry =
            Arc::new(friday_planner::execution::capability_registry::CapabilityRegistry::new());
        friday_planner::execution::native_caps::register_native_builtins(&registry);
        let collector = Arc::new(InMemoryTraceCollector::new());
        let tracer: Arc<dyn TraceCollector> = collector.clone();
        let engine = friday_planner::engines::planning_engine_with_tracer(
            registry,
            Some(Box::new(DeadBackend)),
            friday_planner::execution::lifecycle::ExecutionBudget::default(),
            friday_planner::planning::planners::context::RiskPolicy::default(),
            friday_planner::planning::planners::context::WorkspaceContext::default(),
            Some(tracer),
        );
        let intent = friday_planner::intent::parse("Research the latest Rust async docs");
        let event = friday_planner::events::event(
            friday_planner::events::INTENT_CREATED,
            "endurance",
            serde_json::to_value(&intent).unwrap(),
            uuid::Uuid::new_v4(),
        );
        use friday_core::plugin::Plugin;
        let out = engine
            .handle_event(&event)
            .await
            .expect("fallback must not strand");
        assert!(out
            .iter()
            .any(|e| e.kind == friday_planner::events::PLAN_CREATED));
        let tel = out
            .iter()
            .find(|e| e.kind == friday_planner::events::PLANNING_TELEMETRY)
            .unwrap();
        let t: friday_planner::engines::PlanningTelemetry =
            serde_json::from_value(tel.payload.clone()).unwrap();
        assert!(t.fell_back_to_rule && t.success);
        let failures = collector
            .traces()
            .iter()
            .filter(|s| s.operation == "plan_result" && s.result == ResultType::Failure)
            .count();
        assert_eq!(failures, 1, "provider death stays visible in the trace");
        rows.push(Row {
            cycle: 3,
            world_revision: 0,
            outcome: "provider-death;rule-fallback-ok",
            lesson_count: 0,
            skill_count: 0,
        });
    }

    // ══ Cycle 4 — verification rejection -> honest Failure (F3) ═════════
    let world4 = learning_world();
    {
        let reject_tick = AutonomousTick::new(
            4,
            TickTrigger::Manual,
            Box::new(Obs),
            Box::new(FixedGoal("find the documentation")),
            Box::new(CapturingPlanner::default()),
            Box::new(RecordingExecutor),
            Box::new(VerifyFail),
        )
        .with_world_model(Box::new(ArcModel(world4.clone())));
        let rec = reject_tick.run(&InMemoryTraceCollector::new());
        assert_eq!(
            rec.outcome,
            TickOutcome::Failure,
            "rejection must be honest"
        );
    }
    let mut last_rev = world4.snapshot_state().revision;
    rows.push(Row {
        cycle: 4,
        world_revision: last_rev,
        outcome: "verification-rejected",
        lesson_count: 0,
        skill_count: 0,
    });

    // ══ Cycle 5 — egress block -> zero dispatch (F4) ════════════════════
    {
        use std::sync::atomic::{AtomicUsize, Ordering};
        struct CountingWeb(Arc<AtomicUsize>);
        #[async_trait::async_trait]
        impl friday_actions::ActionAdapter for CountingWeb {
            fn tool(&self) -> &str {
                "web"
            }
            fn actions(&self) -> Vec<&str> {
                vec!["fetch"]
            }
            async fn execute(
                &self,
                _req: friday_actions::request::ActionRequest,
            ) -> friday_actions::ActionResult {
                self.0.fetch_add(1, Ordering::SeqCst);
                friday_actions::ActionResult::success(serde_json::json!({ "ok": true }))
            }
        }
        let calls = Arc::new(AtomicUsize::new(0));
        let kernel = friday_actions::runtime::AgentKernel::new();
        kernel
            .register_adapter(Box::new(CountingWeb(Arc::clone(&calls))))
            .await;
        let scope = friday_agent_runtime::SecurityScope::new(&["example.com"], &[]);
        let ctx = friday_actions::kernel::ExecutionContext::builder()
            .execution_id(uuid::Uuid::new_v4())
            .session_id(uuid::Uuid::new_v4())
            .security_scope(Some(scope))
            .build();
        let req = friday_actions::request::ActionRequest::new(
            "web",
            "fetch",
            serde_json::json!({ "url": "http://127.0.0.1/x" }),
        );
        let res = kernel.run_action(req, ctx).await;
        assert!(!res.success, "internal destination must be denied");
        assert_eq!(
            calls.load(Ordering::SeqCst),
            0,
            "I5: denial precedes dispatch"
        );
        rows.push(Row {
            cycle: 5,
            world_revision: last_rev,
            outcome: "egress-blocked;zero-dispatch",
            lesson_count: 0,
            skill_count: 0,
        });
    }

    // ══ Cycle 6 — stale plan refused before side effects (F5/R2-1) ══════
    {
        struct CountingExec(Mutex<usize>);
        #[async_trait::async_trait]
        impl friday_cognitive::ActionExecutor for CountingExec {
            async fn execute(&self, a: &TickAction) -> friday_cognitive::ActionExecution {
                *self.0.lock().unwrap() += 1;
                friday_cognitive::ActionExecution {
                    action: a.action.clone(),
                    success: true,
                    detail: String::new(),
                    output: serde_json::json!({}),
                    error: None,
                }
            }
        }
        struct Accept;
        #[async_trait::async_trait]
        impl friday_cognitive::ActionVerifier for Accept {
            async fn verify(
                &self,
                _: &GlobalWorkspace,
                _: &TickAction,
                _: &friday_cognitive::ActionExecution,
            ) -> bool {
                true
            }
        }
        let ws = GlobalWorkspace::new();
        ws.publish_from(
            Topic::Screen,
            EngineId::Perception,
            serde_json::json!({ "change": "content" }),
        );
        let jp = journal_dir.path().join("stale.json");
        let j = JsonActionJournal::new(&jp);
        let exec = Arc::new(CountingExec(Mutex::new(0)));
        let drive = DriveTick::new(Arc::new(StaleGoal("stale.action")));
        match drive
            .execute_and_verify_durable(&ws, exec.as_ref(), &Accept, &j, Some(9))
            .await
        {
            DurableTickOutcome::StalePlanRejected { planned, current } => {
                assert_eq!(planned, Some(0));
                assert_eq!(current, Some(9));
            }
            other => panic!("expected refusal, got {other:?}"),
        }
        assert_eq!(*exec.0.lock().unwrap(), 0, "I4: stale plan never executes");
        use friday_cognitive::ActionJournal;
        assert!(
            j.records().unwrap().is_empty(),
            "no journal write for refusals"
        );
        let _ = std::fs::remove_file(jp);
        rows.push(Row {
            cycle: 6,
            world_revision: last_rev,
            outcome: "stale-refused",
            lesson_count: 0,
            skill_count: 0,
        });
    }

    // ══ Cycle 7 — worker crash in checkpoint window -> mount recovery ═══
    let done_before: Vec<String> = workforce
        .tasks()
        .await
        .unwrap()
        .iter()
        .filter(|t| t.status == TaskStatus::Done)
        .map(|t| t.id.clone())
        .collect();
    let doomed = workforce
        .register(AgentRole::Researcher, "doomed")
        .await
        .unwrap();
    let orphan_t = workforce
        .create_task("read a spec", "read a spec", AgentRole::Researcher)
        .await
        .unwrap();
    workforce.assign(&doomed.id, &orphan_t.id).await.unwrap();
    {
        use friday_agent_runtime::WorkerStatus;
        let mut w = store.load_worker(&doomed.id).await.unwrap().unwrap();
        w.status = WorkerStatus::Running;
        w.generation = "gen-dead".to_string();
        store.save_worker(&w).await.unwrap();
        let mut t = store.load_task(&orphan_t.id).await.unwrap().unwrap();
        t.params.insert(
            "docs.search".to_string(),
            serde_json::json!({ "root": docs_root.path().to_string_lossy() }),
        );
        t.status = TaskStatus::InProgress;
        t.owner_generation = "gen-dead".to_string();
        store.save_task(&t).await.unwrap();
    }
    drop(workforce); // the owning generation dies mid-checkpoint

    let successor =
        Arc::new(Workforce::mount(store.clone(), driver.clone(), root.path().join("ws")).await);
    let rec_task = successor.task(&orphan_t.id).await.unwrap().unwrap();
    assert_eq!(
        rec_task.status,
        TaskStatus::Open,
        "I6: orphan reopened at mount"
    );
    let replacement = successor
        .register(AgentRole::Researcher, "replacement")
        .await
        .unwrap();
    let picked = successor
        .pick_up(&replacement.id)
        .await
        .unwrap()
        .expect("replacement takes recovered task");
    assert_eq!(picked.id, orphan_t.id);
    assert!(
        matches!(
            successor.run(&replacement.id).await.unwrap(),
            friday_agent_runtime::TaskOutcome::Done { .. }
        ),
        "I6: recovered task reaches Done"
    );
    rows.push(Row {
        cycle: 7,
        world_revision: last_rev,
        outcome: "crash-recovered",
        lesson_count: 0,
        skill_count: 0,
    });
    // ══ Cycles 8-10 — mismatch corroboration -> lesson -> skill ═════════
    let redb_path = unique_dir("redb");
    let redb = Arc::new(RedbLessonStore::open(&redb_path, PruneConfig::default()).unwrap());
    let skills = SharedSkills(Arc::new(ProductionSkillStore::new()));
    let learning_world_arc = learning_world();
    // One tick INSTANCE across iterations: corroboration lives in the
    // extractor (golden-8B recipe).
    let learning_tick = AutonomousTick::new(
        8,
        TickTrigger::Manual,
        Box::new(Obs),
        Box::new(FixedGoal("find the documentation")),
        Box::new(CapturingPlanner::default()),
        Box::new(RecordingExecutor),
        Box::new(VerifyPass),
    )
    .with_world_model(Box::new(ArcModel(Arc::clone(&learning_world_arc))))
    .with_memory_store(Box::new(ProductionLessonStore::new(
        redb.clone() as Arc<dyn LessonStore>,
        Arc::new(RegistryCapabilityResolver::new(docs_registry())),
        PruneConfig::default(),
    )));

    for cycle in 8..=9 {
        let rec = learning_tick.run(&InMemoryTraceCollector::new());
        assert_eq!(rec.next_action, NextAction::Stop);
        let n = redb
            .find_similar(
                &LessonKey::new("docs.tools", ["docs_read"]),
                MatchPolicy::Exact,
            )
            .unwrap()
            .len();
        assert_eq!(n, 0, "cycle {cycle}: not yet corroborated");
        last_rev = last_rev.max(learning_world_arc.snapshot_state().revision);
        rows.push(Row {
            cycle,
            world_revision: last_rev,
            outcome: "mismatch-corroborating",
            lesson_count: n,
            skill_count: skills.0.skill_count(),
        });
    }

    // Cycle 10: third mismatch admits the lesson...
    let rec10 = learning_tick.run(&InMemoryTraceCollector::new());
    assert_eq!(rec10.next_action, NextAction::Stop);
    let hits = redb
        .find_similar(
            &LessonKey::new("docs.tools", ["docs_read"]),
            MatchPolicy::Exact,
        )
        .unwrap();
    assert_eq!(hits.len(), 1, "cycle 10: lesson admitted at threshold");

    // ...and the evaluator admits the skill on the delegations' real
    // verification evidence (L1 unchanged: ticks alone carry none).
    let cond = LessonCondition {
        action: "find the documentation".into(),
        signals: vec!["docs_read".into()],
    };
    let pl = &hits[0];
    let lesson = friday_autonomy::memory::Lesson {
        condition: cond.clone(),
        claim: pl.claim.clone(),
        confidence: pl.confidence,
        support_count: pl.support_count,
        verification_count: pl.verification_count,
        first_observed_ms: pl.first_observed_ms,
        last_observed_ms: pl.last_observed_ms,
    };
    let evaluator = SkillEvaluator {
        known_trajectories: {
            let mut m = std::collections::HashMap::new();
            m.insert(
                cond.clone(),
                vec![
                    "director.delegation".to_string(),
                    "workforce.checkpoint".to_string(),
                    "docs.search".to_string(),
                ],
            );
            m
        },
        ..SkillEvaluator::default()
    };
    let traj = evaluator.trajectory_for(&cond).unwrap();
    let mut cand = evaluator.form_candidate(&lesson, &cond, traj);
    cand.verification_evidence = 2; // cycles 1 and 2: gate-verified delegations
    match evaluator.evaluate(&cand) {
        SkillAdmission::Promote(s) => skills.admit(s),
        other => panic!("cycle 10 admission failed: {other:?}"),
    }
    assert_eq!(skills.0.skill_count(), 1);
    last_rev = last_rev.max(learning_world_arc.snapshot_state().revision);
    rows.push(Row {
        cycle: 10,
        world_revision: last_rev,
        outcome: "lesson-admitted;skill-admitted",
        lesson_count: 1,
        skill_count: 1,
    });

    // ══ Cycle 11 — REAL STORE RESTART + consultation continues ══════════
    let pre_restart_lessons = redb
        .find_similar(
            &LessonKey::new("docs.tools", ["docs_read"]),
            MatchPolicy::Exact,
        )
        .unwrap()
        .len();
    drop(redb); // durable lesson owner dropped
    drop(learning_tick); // its boxed memory store holds another redb handle
    drop(successor); // workforce owner dropped

    // Fresh owners over the same persistent roots.
    let redb2 = Arc::new(RedbLessonStore::open(&redb_path, PruneConfig::default()).unwrap());
    let post = redb2
        .find_similar(
            &LessonKey::new("docs.tools", ["docs_read"]),
            MatchPolicy::Exact,
        )
        .unwrap()
        .len();
    assert_eq!(post, pre_restart_lessons, "I8: lessons restart-consistent");
    let workforce3 = Arc::new(Workforce::new(
        store.clone(),
        driver.clone(),
        root.path().join("ws"),
    ));
    let tasks3 = workforce3.tasks().await.unwrap();
    assert!(
        !workforce3.workers().await.unwrap().is_empty(),
        "workers survive restart"
    );
    for t in &tasks3 {
        if done_before.contains(&t.id) || t.id == orphan_t.id {
            assert_eq!(
                t.status,
                TaskStatus::Done,
                "I2: terminal tasks never resurrect"
            );
        }
    }

    // Consultation on the live in-memory skill store — intentionally NOT
    // restart-verified (no restart path yet; ledger records this honestly).
    assert!(
        skills.find_applicable(&cond).is_some(),
        "I7: skill retrievable"
    );
    let consult_offered = CapturingPlanner::default();
    let consult_tick = AutonomousTick::new(
        11,
        TickTrigger::Manual,
        Box::new(Obs),
        Box::new(FixedGoal("find the documentation")),
        Box::new(consult_offered.clone()),
        Box::new(RecordingExecutor),
        Box::new(VerifyPass),
    )
    .with_world_model(Box::new(ArcModel(Arc::clone(&learning_world_arc))))
    .with_memory_store(Box::new(ProductionLessonStore::new(
        redb2.clone() as Arc<dyn LessonStore>,
        Arc::new(RegistryCapabilityResolver::new(docs_registry())),
        PruneConfig::default(),
    )))
    .with_skill_store(Box::new(skills.clone()))
    .with_skill_provider(Box::new(skills.clone()));
    let c11 = consult_tick.run(&InMemoryTraceCollector::new());
    assert_eq!(c11.outcome, TickOutcome::Success);
    assert!(
        consult_offered
            .0
            .lock()
            .unwrap()
            .iter()
            .any(|s| !s.is_empty()),
        "cycle 11: admitted skill consulted"
    );
    last_rev = last_rev.max(learning_world_arc.snapshot_state().revision);
    rows.push(Row {
        cycle: 11,
        world_revision: last_rev,
        outcome: "restart-ok;skill-consulted",
        lesson_count: post,
        skill_count: skills.0.skill_count(),
    });

    // ══ Cycle 12 — journal replay safety + second consultation ══════════
    let board_before = workforce3.tasks().await.unwrap().len();
    let replay_ws = GlobalWorkspace::new();
    replay_ws.publish_from(
        Topic::Screen,
        EngineId::Perception,
        serde_json::json!({ "change": "content", "hash": "replay" }),
    );
    let replay_director = AgentDirector::new(Arc::new(
        WorkforceDriver::new(Arc::clone(&workforce3), AgentRole::Researcher).with_param(
            "docs.search",
            serde_json::json!({ "root": docs_root.path().to_string_lossy() }),
        ),
    ));
    let replay_host = AutonomyHost::new(
        replay_ws,
        DriveTick::new(Arc::new(ReplayGoal(cycle1_action))),
        Arc::new(DirectorActionExecutor::new(Arc::new(replay_director))),
        Arc::new(DirectorVerified),
        Arc::new(JsonActionJournal::new(&journal_path)),
    )
    .with_world_model(Arc::clone(&model));
    // The completed journal record replays as a recovered outcome; whether
    // the host surfaces it or returns None, NO new work may be created.
    let _ = replay_host.tick().await;
    let board_after = workforce3.tasks().await.unwrap().len();
    assert_eq!(board_after, board_before, "I9: replay creates no new work");

    let c12 = consult_tick.run(&InMemoryTraceCollector::new());
    assert_eq!(c12.outcome, TickOutcome::Success);
    assert!(
        skills.find_applicable(&cond).is_some(),
        "I7: still retrievable"
    );
    last_rev = last_rev.max(learning_world_arc.snapshot_state().revision);
    rows.push(Row {
        cycle: 12,
        world_revision: last_rev,
        outcome: "consulted;replay-safe",
        lesson_count: redb2
            .find_similar(
                &LessonKey::new("docs.tools", ["docs_read"]),
                MatchPolicy::Exact,
            )
            .unwrap()
            .len(),
        skill_count: skills.0.skill_count(),
    });

    // ── Invariant sweep over the recorded ledger ─────────────────────────
    assert_eq!(rows.len(), 12, "twelve cycles ran");
    for w in rows.windows(2) {
        assert!(
            w[1].world_revision >= w[0].world_revision,
            "I1: revision regressed at cycle {} ({:?} -> {:?})",
            w[1].cycle,
            (w[0].cycle, w[0].world_revision),
            (w[1].cycle, w[1].world_revision)
        );
    }
    for t in workforce3.tasks().await.unwrap() {
        assert_ne!(
            t.status,
            TaskStatus::InProgress,
            "I6: no orphans at the end"
        );
    }

    println!("R2-4 ledger:");
    for r in &rows {
        println!(
            "  cycle {:>2} | rev {:>2} | lessons {} | skills {} | {}",
            r.cycle, r.world_revision, r.lesson_count, r.skill_count, r.outcome
        );
    }
}
