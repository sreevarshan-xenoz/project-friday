//! R2-3 - the Director -> Workforce organism test.
//!
//! One scenario, existing FRIDAY seams only, fully hermetic (no LLM):
//!
//! ```text
//! Goal
//!  | DirectorActionExecutor -> AgentDirector            (delegation)
//!  | WorkforceDriver                                    (R2-3 enabler seam)
//!  | persistent Workforce board task + checkpoint       (durable identity)
//!  | worker executes the real docs skill                (execution)
//!  | hard crash in the F2 checkpoint window             (interruption)
//!  | Workforce::mount() reconciles the orphan           (recovery, b9acbd6)
//!  | replacement worker completes the recovered task    (completion)
//!  | unverified outcomes refused                        (verification honesty)
//!  | repeated prediction error -> ProductionLessonStore (Redb, durable)
//!  | evaluator admits skill into ProductionSkillStore   (learning/skill)
//!  | next tick: skills.consult surfaces it to planning  (future consultation)
//! ```
//!
//! Coupling contract (honest scope note): lesson production keys off the
//! bounded tick's prediction-vs-reality outcome by architecture (7B/7C); the
//! organism couples its delegation half and learning half through the same
//! goal string and the shared durable stores, not through a new pipeline.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use friday_actions::{AgentKernel, DocsAdapter, SkillAgentDriver};
use friday_agent_runtime::{
    AgentDirector, AgentDriver, AgentRole, JsonWorkforceStore, TaskStatus, Workforce,
    WorkforceDriver, WorkforceStore,
};
use friday_cognitive::{
    AutonomousTick as DriveTick, DecisionFunction, DecisionState, EngineId, GlobalWorkspace,
    JsonActionJournal, Priority, TickAction, Topic,
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
use friday_autonomy::tick::{ExecutionResult, NextAction, Observation, Plan, TickTrigger};
use friday_autonomy::world::WorldModel;
use friday_autonomy::world_production::ProductionWorldModelAdapter;
use friday_capabilities::CapabilityRegistry;

fn unique_dir(tag: &str) -> PathBuf {
    static N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = N.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    std::env::temp_dir().join(format!(
        "friday_organism_{tag}_{}_{}",
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

/// The real deterministic worker brain: skill engine + real kernel + DocsAdapter.
async fn docs_skill_driver(_docs_root: &Path) -> Arc<dyn AgentDriver> {
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
    assert!(report.ok(), "load failures: {:?}", report.failed);

    let kernel = Arc::new(AgentKernel::new());
    kernel.register_adapter(Box::new(DocsAdapter::new())).await;

    Arc::new(SkillAgentDriver::new(
        Arc::clone(&skills),
        Arc::new(SkillApprovals::new()),
        planner,
        Arc::clone(&kernel),
    ))
}
// -- Phase 1+2 fixtures: the goal entry and the host verifier ----------

/// Two trait seams over one shared store instance (test-local forwarding).
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
// The organism's Goal entry: one bounded decision from a sensor event,
/// grounding-stamped like IntentDecision does (the intent seam itself is
/// separately qualified; this keeps the organism deterministic).
struct OrganismGoal(&'static str);

impl DecisionFunction for OrganismGoal {
    fn decide(
        &self,
        workspace: &GlobalWorkspace,
        _state: &mut DecisionState,
    ) -> Option<TickAction> {
        let evidence = workspace.get_latest("sensor.screen")?;
        Some(
            TickAction::new(self.0, "organism goal", vec![evidence.id], Priority::High)
                .with_grounding_revision(0),
        )
    }
}

/// Host verification gate: the Director must have verified its children.
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

// -- Phase 3 fakes: the bounded learning tick (7B–7E recipe, golden 8B) -

struct Obs;
impl ObservationSource for Obs {
    fn observe(&self) -> Observation {
        Observation {
            summary: "inbox has actionable tasks".into(),
            // The world is showing the same signal the lesson/skill keyed on.
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

/// The docs goal's prediction diverges from reality on every tick (the causal
/// projection sets `docs_read`, live state stays unchanged) — the honest
/// repeated-error signal the lesson threshold needs.
fn learning_world() -> ProductionWorldModelAdapter {
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
    ProductionWorldModelAdapter::from_model(m)
}

fn docs_registry() -> Arc<CapabilityRegistry> {
    let reg = CapabilityRegistry::new();
    let cap = friday_capabilities::Capability::new("docs.tools", "Docs", "find the documentation")
        .with_actions(&["find the documentation"]);
    reg.register_provider("docs-svc", vec![cap]);
    Arc::new(reg)
}

// -- The organism -------------------------------------------------------

#[tokio::test]
async fn organism_r2_3_delegation_durability_recovery_learning_consult() {
    // -- Shared fixtures ------------------------------------------------
    let root = TempDir::new("root");
    let docs_root = TempDir::new("docs");
    std::fs::write(docs_root.path().join("README.md"), "# FRIDAY organism docs").unwrap();
    let driver = docs_skill_driver(docs_root.path()).await;
    let store: Arc<dyn WorkforceStore> =
        Arc::new(JsonWorkforceStore::new(root.path().join("store")));

    // -- Phase 1 — literal Director ? Workforce delegation ---------------
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
    let executor =
        DirectorActionExecutor::new(Arc::new(director)).with_cwd(docs_root.path().to_path_buf());

    let workspace = GlobalWorkspace::new();
    workspace.publish_from(
        Topic::Screen,
        EngineId::Perception,
        serde_json::json!({ "change": "content", "content_hash": "organism" }),
    );
    let model = Arc::new(Mutex::new(ProdWorldModel::new(WorldState::new())));
    let journal_dir = TempDir::new("journal");
    let journal_path = journal_dir.path().join("journal.json");
    let host = AutonomyHost::new(
        workspace,
        DriveTick::new(Arc::new(OrganismGoal("find the documentation"))),
        Arc::new(executor),
        Arc::new(DirectorVerified),
        Arc::new(JsonActionJournal::new(&journal_path)),
    )
    .with_world_model(Arc::clone(&model));

    // Goal ? decision ? Director ? board task ? worker ? verified result.
    let outcome = host.tick().await.expect("delegated tick must execute");
    assert!(outcome.verified, "the delegated child must pass the gate");

    // A. Literal delegation + B. durable identity: exactly one real board
    // task reached Done and its result names the durable identity.
    let tasks = workforce.tasks().await.unwrap();
    assert_eq!(tasks.len(), 1, "the delegation created one board task");
    assert_eq!(tasks[0].status, TaskStatus::Done);
    assert_eq!(tasks[0].goal, "find the documentation");
    let persisted = tasks[0].result.as_ref().expect("persisted result");
    assert!(
        persisted
            .steps
            .iter()
            .any(|s| s.description == "docs.search" && s.outcome == "ok"),
        "the board must carry the real execution evidence: {:?}",
        persisted.steps
    );

    // F-honesty: nothing fabricated. This outcome carries no world
    // observation payload (skill workers do not emit one), so the world
    // model must NOT advance — and a failed outcome never advances it.
    {
        let mut m = model.lock().unwrap();
        assert!(apply_verified_action_outcome(&mut m, &outcome).is_none());
        let mut failed = outcome.clone();
        failed.verified = false;
        assert!(apply_verified_action_outcome(&mut m, &failed).is_none());
        assert!(
            m.history().is_empty(),
            "no world transition may be synthesized without an explicit observation"
        );
    }

    // -- Phase 2 — interruption inside the checkpoint window + recovery --
    // The next delegation's durable state is written by its owning instance…
    let doomed = workforce
        .register(AgentRole::Researcher, "doomed delegate")
        .await
        .unwrap();
    let t2 = workforce
        .create_task("read a spec", "read a spec", AgentRole::Researcher)
        .await
        .unwrap();
    workforce.assign(&doomed.id, &t2.id).await.unwrap();
    // …exactly run()'s pre-dispatch checkpoint, stamped with generation G_A.
    {
        let mut w = store.load_worker(&doomed.id).await.unwrap().unwrap();
        w.status = friday_agent_runtime::WorkerStatus::Running;
        w.generation = "gen-A-dead".to_string();
        store.save_worker(&w).await.unwrap();
        let mut t = store.load_task(&t2.id).await.unwrap().unwrap();
        // the doomed delegation carried its dispatch params like any other:
        t.params.insert(
            "docs.search".to_string(),
            serde_json::json!({ "root": docs_root.path().to_string_lossy() }),
        );
        t.status = TaskStatus::InProgress;
        t.owner_generation = "gen-A-dead".to_string();
        t.run_deadline_ms = 4_000_000_000_000;
        store.save_task(&t).await.unwrap();
    }
    // ?? Instance A (and its generation) disappears with the task InProgress.
    drop(workforce);

    // C. Recovery: the successor mounts over the same persistent store and
    // reconciliation reopens the orphan before serving anything.
    let successor =
        Arc::new(Workforce::mount(store.clone(), driver.clone(), root.path().join("ws")).await);
    let recovered = successor
        .task(&t2.id)
        .await
        .unwrap()
        .expect("orphan survives");
    assert_eq!(
        recovered.status,
        TaskStatus::Open,
        "orphan reopened by mount()"
    );
    assert!(
        recovered
            .progress
            .iter()
            .any(|n| n.note.contains("orphaned task reopened")),
        "recovery audited in the existing note vocabulary"
    );

    // D. Completion: a replacement worker picks up and finishes the task.
    let replacement = successor
        .register(AgentRole::Researcher, "replacement delegate")
        .await
        .unwrap();
    let picked = successor
        .pick_up(&replacement.id)
        .await
        .unwrap()
        .expect("replacement takes the recovered task");
    assert_eq!(picked.id, t2.id);
    let outcome2 = successor.run(&replacement.id).await.unwrap();
    assert!(matches!(
        outcome2,
        friday_agent_runtime::TaskOutcome::Done { .. }
    ));

    // Delegation through the RECOVERED workforce still works end to end:
    // a second host tick delegates fresh durable work post-recovery.
    let workspace2 = GlobalWorkspace::new();
    workspace2.publish_from(
        Topic::Screen,
        EngineId::Perception,
        serde_json::json!({ "change": "content", "content_hash": "post-recovery" }),
    );
    let director2 = AgentDirector::new(Arc::new(
        WorkforceDriver::new(Arc::clone(&successor), AgentRole::Researcher).with_param(
            "docs.search",
            serde_json::json!({ "root": docs_root.path().to_string_lossy() }),
        ),
    ));
    let host2 = AutonomyHost::new(
        workspace2,
        DriveTick::new(Arc::new(OrganismGoal("search the docs"))),
        Arc::new(
            DirectorActionExecutor::new(Arc::new(director2))
                .with_cwd(docs_root.path().to_path_buf()),
        ),
        Arc::new(DirectorVerified),
        Arc::new(JsonActionJournal::new(journal_dir.path().join("j2.json"))),
    )
    .with_world_model(Arc::clone(&model));
    let outcome3 = host2
        .tick()
        .await
        .expect("post-recovery delegation executes");
    assert!(outcome3.verified);

    // Two verified delegations total — the honest verification evidence for
    // the skill candidate later in this scenario.

    // -- Phase 3 — learning: lesson ? skill ? future consultation --------
    let registry = docs_registry();
    let resolver = Arc::new(RegistryCapabilityResolver::new(registry));
    let redb_path = unique_dir("redb");
    let store_arc = Arc::new(RedbLessonStore::open(&redb_path, PruneConfig::default()).unwrap());
    let _lessons = ProductionLessonStore::new(
        store_arc.clone() as Arc<dyn LessonStore>,
        resolver,
        PruneConfig::default(),
    );
    let skills = SharedSkills(Arc::new(ProductionSkillStore::new()));

    let earned_key = LessonKey::new("docs.tools", ["docs_read"]);
    let prod_lessons = ProductionLessonStore::new(
        store_arc.clone() as Arc<dyn LessonStore>,
        Arc::new(RegistryCapabilityResolver::new(docs_registry())),
        PruneConfig::default(),
    );
    let learning_tick = AutonomousTick::new(
        1,
        TickTrigger::Manual,
        Box::new(Obs),
        Box::new(FixedGoal("find the documentation")),
        Box::new(CapturingPlanner::default()),
        Box::new(RecordingExecutor),
        Box::new(VerifyPass),
    )
    .with_world_model(Box::new(learning_world()) as Box<dyn WorldModel>)
    .with_memory_store(Box::new(prod_lessons));
    for i in 0..3 {
        let rec = learning_tick.run(&InMemoryTraceCollector::new());
        assert_eq!(rec.next_action, NextAction::Stop);
        let hits = store_arc
            .find_similar(&earned_key, MatchPolicy::Exact)
            .unwrap();
        if i < 2 {
            assert!(hits.is_empty(), "tick {i}: not yet corroborated");
        } else {
            assert_eq!(hits.len(), 1, "tick {i}: lesson admitted at threshold");
        }
    }

    // Skill admission through the REAL evaluator: the trajectory is the one
    // this organism actually exercised; the verification evidence is the two
    // gate-verified delegations from Phases 1–2.
    let cond = LessonCondition {
        action: "find the documentation".into(),
        signals: vec!["docs_read".into()],
    };
    let lesson = {
        let hits = store_arc
            .find_similar(&earned_key, MatchPolicy::Exact)
            .unwrap();
        assert_eq!(hits.len(), 1);
        friday_autonomy::memory::Lesson {
            condition: cond.clone(),
            claim: hits[0].claim.clone(),
            confidence: hits[0].confidence,
            support_count: hits[0].support_count,
            first_observed_ms: hits[0].first_observed_ms,
            last_observed_ms: hits[0].last_observed_ms,
        }
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
    let traj = evaluator
        .trajectory_for(&cond)
        .expect("known-good trajectory");
    let mut candidate = evaluator.form_candidate(&lesson, &cond, traj);
    candidate.verification_evidence = 2; // the two gate-verified delegations above
    match evaluator.evaluate(&candidate) {
        SkillAdmission::Promote(skill) => skills.admit(skill),
        other => panic!("the verified trajectory must be promotable: {other:?}"),
    }
    assert_eq!(
        skills.0.skill_count(),
        1,
        "compiled into the production registry"
    );

    // I. Future consultation: the NEXT tick consults the provider and the
    // planner is offered the admitted skill (skills.consult evidence).
    let offered = CapturingPlanner::default();
    let consult_tick = AutonomousTick::new(
        99,
        TickTrigger::Manual,
        Box::new(Obs),
        Box::new(FixedGoal("find the documentation")),
        Box::new(offered.clone()),
        Box::new(RecordingExecutor),
        Box::new(VerifyPass),
    )
    .with_world_model(Box::new(learning_world()) as Box<dyn WorldModel>)
    .with_memory_store(Box::new(
        friday_autonomy::memory_production::ProductionLessonStore::new(
            store_arc.clone() as Arc<dyn LessonStore>,
            Arc::new(RegistryCapabilityResolver::new(docs_registry())),
            PruneConfig::default(),
        ),
    ))
    .with_skill_store(Box::new(skills.clone()))
    .with_skill_provider(Box::new(skills.clone()));
    let collector = InMemoryTraceCollector::new();
    let rec = consult_tick.run(&collector);
    assert_eq!(rec.outcome, friday_autonomy::tick::TickOutcome::Success);
    assert!(
        collector
            .traces()
            .iter()
            .any(|t| t.organ == "skills" && t.operation == "consult"),
        "the consultation must leave its standard evidence span"
    );
    let offered_skills = offered.0.lock().unwrap().clone();
    assert!(
        offered_skills.iter().any(|s| !s.is_empty()),
        "the admitted skill must reach planning"
    );
}
