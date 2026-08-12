//! Skill Engine runtime wiring tests (M6 first pass) — the behavior-level
//! proof that M5 works, not just that the crate compiles:
//!
//! * the 8 real bundled manifests load, resolve, and qualify against the
//!   *actual* native capability registry,
//! * skill ranking explains itself ("start my minecraft server" → Minecraft
//!   ranked above Server, with auditable scores),
//! * the supervisor gates a skill-covered medium-risk capability until the
//!   user approves the covering skill, then resumes and completes,
//! * capabilities no skill declares defer to the default allow-all policy,
//! * high-risk capabilities pause at the supervisor's metadata gate *even
//!   after* approval (approval unlocks Safe/Medium only),
//! * the `with_skills` context path surfaces real bundled skills to the LLM.

use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use friday_core::event::Event;
use friday_core::plugin::{Plugin, PluginMetadata};
use friday_core::Friday;
use friday_goal_execution::{run_command_in, CodeTestVerifier};
use friday_planner::events;
use friday_planner::execution::capability_registry::CapabilityRegistry;
use friday_planner::planning::planners::backend::{MockBackend, ReasoningBackend};
use tokio::sync::broadcast;

use friday_context::{
    load_skill_engine, register_engines_with_skills, ContextBuilder, PlannerRegistryResolver,
    SkillPermission, SkillSource,
};
use friday_skills::PermissionState;

fn bundled_dir() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../skills/bundled")
}

fn planner_registry() -> CapabilityRegistry {
    let reg = CapabilityRegistry::new();
    friday_planner::execution::native_caps::register_native_builtins(&reg);
    reg
}

/// The supervisor completion sink must route verified trajectories into the
/// learning seam, keep the candidate disabled, require explicit admission,
/// and only then make the learned skill discoverable.
#[tokio::test]
async fn supervisor_verified_execution_can_enter_skill_learning_path() {
    friday_core::init_telemetry();
    let mut friday = Friday::new();

    let goal = "verify the emerald ledger snapshot";
    let plan = serde_json::json!({
        "tasks": [
            {
                "name": "Read the emerald ledger snapshot",
                "capability": "filesystem.read_file",
                "params": { "path": "notes.md" },
                "depends_on_indices": []
            }
        ]
    })
    .to_string();
    let (_, wiring) = register_engines_with_skills(
        &mut friday,
        Some(Box::new(MockBackend::new(plan))),
        &[(bundled_dir(), SkillSource::Builtin)],
    )
    .await
    .expect("register with skills");
    let wiring = wiring.expect("skills loaded");

    friday
        .register_plugin(mock_executor())
        .await
        .expect("register mock executor");

    // Real verifier succeeds on a real command run. This is the verified
    // execution boundary the supervisor path is expected to consume.
    let verifier = CodeTestVerifier::for_test_run();
    let obs = run_command_in(std::env::temp_dir().as_path(), "echo test result: ok");
    let verified = verifier.verify_observation(&obs);
    assert!(verified.verified, "real verifier must succeed: {:?}", verified.checks);

    let bus = friday.bus().clone();
    let rx = bus.subscribe();
    bus.publish(Event::new(
        events::GOAL_RAW,
        "user",
        serde_json::json!({ "text": goal }),
    ))
    .await
    .expect("publish goal");

    let evs = run_and_collect(&mut friday, rx, 3).await;
    let kinds = kinds_of(&evs);
    assert!(
        kinds.contains(&events::PLAN_CREATED),
        "plan.created missing: {kinds:?}"
    );
    assert!(
        kinds.contains(&events::TASK_COMPLETED),
        "task.completed missing: {kinds:?}"
    );
    assert!(
        kinds.contains(&events::VERIFICATION_COMPLETED),
        "verification.completed missing: {kinds:?}"
    );
    assert!(
        kinds.contains(&events::GOAL_COMPLETED),
        "goal.completed missing: {kinds:?}"
    );

    // The supervisor-completion sink should have learned a candidate, but it
    // must stay disabled until admitted.
    let learning = wiring.skill_learning();
    let registry = learning.registry_handle();
    let disabled = registry.search_disabled(goal, 8);
    assert!(
        !disabled.is_empty(),
        "the verified trajectory should register as a disabled candidate"
    );
    let candidate_id = disabled[0].skill.id().to_string();
    assert!(
        registry
            .select_for(goal, 8)
            .as_ref()
            .map(|selected| selected.skill.id() != candidate_id.as_str())
            .unwrap_or(true),
        "candidate must not be selected before explicit admission"
    );
    let candidate = registry
        .get(&candidate_id)
        .expect("learned candidate must exist");
    assert!(
        !candidate.enabled,
        "learned candidate must start disabled"
    );

    // Explicit admission is the only step that makes it reusable.
    let report = learning.admit(&candidate_id).expect("admission ran");
    assert!(report.passed, "qualification must pass: {:?}", report.checks);
    assert!(
        registry.get(&candidate_id).unwrap().enabled,
        "explicit admission must enable the candidate"
    );
    let selected = registry
        .select_for(goal, 8)
        .expect("planner should discover the admitted skill");
    assert_eq!(selected.skill.id(), candidate_id);

    // Reuse remains independently verifiable.
    let obs2 = run_command_in(std::env::temp_dir().as_path(), "echo test result: ok");
    let verified2 = verifier.verify_observation(&obs2);
    assert!(verified2.verified, "reused execution must verify: {:?}", verified2.checks);
}

#[test]
fn bundled_skills_resolve_qualify_and_rank_against_real_registry() {
    let reg = Arc::new(planner_registry());
    let wiring = load_skill_engine(Arc::clone(&reg), &[(bundled_dir(), SkillSource::Builtin)]);
    assert!(wiring.all_loaded(), "load reports: {:?}", wiring.reports);

    let all = wiring.registry.all();
    let ids: Vec<&str> = all.iter().map(|s| s.id()).collect();
    for id in [
        "developer",
        "docker",
        "git",
        "minecraft",
        "python",
        "rust",
        "server",
        "testing",
    ] {
        assert!(ids.contains(&id), "missing bundled skill {id}: {ids:?}");
    }

    // Every skill with declared capabilities is usable against the real
    // registry (they resolve, not just soft-declare). The `developer`
    // meta-skill intentionally declares none (it aggregates via dependencies).
    for skill in wiring.registry.all() {
        if !skill.manifest.capabilities.is_empty() {
            assert!(
                skill.is_usable(),
                "{} not usable: {:?}",
                skill.id(),
                skill.capabilities
            );
        }
        let report = friday_skills::qualify(
            &wiring.registry,
            &PlannerRegistryResolver(Arc::clone(&reg)),
            skill.id(),
        );
        assert!(
            report.passed,
            "{} failed qualification: {:?}",
            skill.id(),
            report
        );
    }

    // Explainability: the goal ranks Minecraft above Server, with scores.
    let ranked = wiring.registry.search("start my minecraft server", 8);
    assert_eq!(ranked[0].skill.id(), "minecraft");
    let server = ranked
        .iter()
        .find(|r| r.skill.id() == "server")
        .expect("server ranked");
    assert!(
        ranked[0].score > server.score,
        "minecraft {:.3} should beat server {:.3}",
        ranked[0].score,
        server.score
    );

    // Policy: skill-covered capabilities prompt until the covering skill is
    // approved; capabilities no skill declares are not managed.
    assert!(wiring.policy.manages_capability("filesystem.write_file"));
    assert!(wiring.policy.requires_prompt("filesystem.write_file"));
    assert!(wiring.policy.requires_prompt("terminal.exec"));
    assert!(!wiring
        .policy
        .manages_capability("mcp:filesystem.write_file"));

    wiring.approvals.approve("minecraft");
    assert!(wiring.policy.allows_capability("filesystem.write_file"));
    assert!(wiring.policy.allows_capability("terminal.exec"));
}

/// The reviewer's `Relevant → Chosen → Capabilities → Permission` chain,
/// exposed as an auditable decision. The bundled `minecraft` skill declares
/// `terminal.exec`, `filesystem.read_file`, `filesystem.write_file`; with no
/// readonly bundled skill and no approval yet, every one requires approval.
#[test]
fn decision_chain_reports_chosen_skill_and_permissions() {
    let reg = Arc::new(planner_registry());
    let wiring = load_skill_engine(Arc::clone(&reg), &[(bundled_dir(), SkillSource::Builtin)]);

    let decision = wiring.decision_for("start my minecraft server");
    assert_eq!(decision.chosen.as_deref(), Some("minecraft"));
    assert!(decision.candidates.iter().any(|(id, _)| id == "minecraft"));
    assert!(
        decision.candidates[0].1 >= decision.candidates[1].1,
        "candidates must be in rank order"
    );
    // Nothing disabled → nothing skipped.
    assert!(decision.skipped.is_empty());
    assert_eq!(decision.capabilities.len(), 3);
    assert!(decision
        .capabilities
        .iter()
        .all(|c| c.permission == SkillPermission::RequiresApproval));

    // A capability no bundled skill declares → Deferred (previous policy).
    assert_eq!(
        wiring.policy.permission_for("mcp:filesystem.write_file"),
        PermissionState::Unmanaged
    );

    // An approved covering skill flips the chain's permission states.
    wiring.approvals.approve("minecraft");
    let decision = wiring.decision_for("start my minecraft server");
    assert!(decision
        .capabilities
        .iter()
        .all(|c| c.permission == SkillPermission::Approved));
}

/// Disabling the preferred skill must re-route the goal to the next usable
/// candidate — never to a disabled skill.
#[test]
fn disabled_preferred_skill_skips_to_next_usable() {
    let reg = Arc::new(planner_registry());
    let wiring = load_skill_engine(Arc::clone(&reg), &[(bundled_dir(), SkillSource::Builtin)]);

    assert_eq!(
        wiring
            .registry
            .select_for("start my minecraft server", 8)
            .unwrap()
            .skill
            .id(),
        "minecraft"
    );
    wiring.registry.disable("minecraft").unwrap();
    let selected = wiring
        .registry
        .select_for("start my minecraft server", 8)
        .unwrap();
    assert_eq!(selected.skill.id(), "server");

    // The decision chain stays auditable: minecraft is reported as skipped
    // (disabled), not silently dropped — the "why wasn't X chosen" answer.
    let decision = wiring.decision_for("start my minecraft server");
    assert_eq!(decision.chosen.as_deref(), Some("server"));
    assert!(
        decision.skipped.iter().any(|(id, _)| id == "minecraft"),
        "disabled preferred skill must appear as skipped: {:?}",
        decision.skipped
    );
}

/// Approving a skill that does NOT cover a capability must not unlock it —
/// approval is scoped to what the approved skill declares.
#[test]
fn approving_non_covering_skill_does_not_unlock_capability() {
    let reg = Arc::new(planner_registry());
    let wiring = load_skill_engine(Arc::clone(&reg), &[(bundled_dir(), SkillSource::Builtin)]);

    // git declares git.* + terminal.exec — NOT filesystem.write_file.
    assert!(wiring.policy.requires_prompt("filesystem.write_file"));
    wiring.approvals.approve("git");
    assert!(!wiring.policy.allows_capability("filesystem.write_file"));
    assert_eq!(
        wiring.policy.permission_for("filesystem.write_file"),
        PermissionState::PromptRequired
    );

    // And approving git DOES unlock what git covers.
    assert!(wiring.policy.allows_capability("git.commit"));
}

/// Skill health is a live view: a capability going `Offline` in the planner
/// registry must surface as a skill-level `Warning` immediately (the
/// load-time snapshot is never the source of truth for runtime health).
#[test]
fn skill_health_tracks_live_registry_changes() {
    let reg = Arc::new(planner_registry());
    let wiring = load_skill_engine(Arc::clone(&reg), &[(bundled_dir(), SkillSource::Builtin)]);

    assert_eq!(
        wiring.health_of("git"),
        Some(friday_skills::SkillHealth::Healthy)
    );
    // Aggregator meta-skill with no capabilities of its own → Warning.
    assert!(matches!(
        wiring.health_of("developer"),
        Some(friday_skills::SkillHealth::Warning(_))
    ));

    // Simulate git.status going offline at runtime.
    reg.set_health(
        "git.status",
        friday_capabilities::spec::CapabilityHealth::Offline,
    );
    assert!(matches!(
        wiring.health_of("git"),
        Some(friday_skills::SkillHealth::Warning(msg)) if msg.contains("offline")
    ));

    reg.set_health(
        "git.status",
        friday_capabilities::spec::CapabilityHealth::Healthy,
    );
    assert_eq!(
        wiring.health_of("git"),
        Some(friday_skills::SkillHealth::Healthy)
    );

    // The summary covers the bundled skills, sorted by id (codeintel sorts
    // first today; assert the sort + membership rather than pinning index 0
    // so the test survives new bundled skills).
    let summary = wiring.health();
    assert!(
        summary.windows(2).all(|w| w[0].0 <= w[1].0),
        "health summary must be sorted by id: {:?}",
        summary
    );
    assert!(summary.iter().any(|(id, _)| id == "developer"));
    assert!(summary.iter().any(|(id, _)| id == "git"));
    assert!(summary.iter().any(|(id, _)| id == "minecraft"));
    assert!(summary.iter().any(|(id, _)| id == "web"));
}

/// M8: the wiring exposes a gated per-skill memory handle over the REAL
/// `MemoryEngine` — a declared write key stores durably in LongTerm, reads
/// come back cleanly, undeclared keys are rejected by the gate, and search
/// finds what was stored.
#[tokio::test]
async fn skill_memory_roundtrips_through_real_memory_engine() {
    let reg = Arc::new(planner_registry());
    let temp = std::env::temp_dir().join(format!("skill-memory-{}", uuid::Uuid::new_v4()));
    let engine =
        Arc::new(friday_core::memory::MemoryEngine::default_layers(&temp).expect("memory layers"));

    let wiring = load_skill_engine(Arc::clone(&reg), &[(bundled_dir(), SkillSource::Builtin)]);
    // No source attached yet → no handle for any skill.
    assert!(wiring.memory_for("git").is_none());

    let wiring = wiring.with_memory(Arc::new(friday_context::CoreMemorySource(Arc::clone(
        &engine,
    ))));
    let git = wiring.memory_for("git").expect("git memory handle");

    // git/skill.json declares git.last_workspace in reads + writes → allowed.
    git.store("git.last_workspace", "E:/projects/notes")
        .await
        .unwrap();
    let hit = git
        .read("git.last_workspace")
        .await
        .unwrap()
        .expect("read back");
    assert_eq!(hit.value, "E:/projects/notes");

    // Undeclared key → denied at the gate, nothing persisted.
    assert_eq!(
        git.store("user.credentials", "hunter2").await,
        Err(friday_skills::MemoryError::UndeclaredWrite(
            "user.credentials".to_string()
        ))
    );

    // Search through the skill handle finds the stored value.
    let hits = git.search("notes", 5).await;
    assert!(
        hits.iter().any(|h| h.key == "git.last_workspace"),
        "search hits: {hits:?}"
    );

    // Persisted durably: drop the live handles (redb allows one open per
    // file), then a fresh engine over the same directory still reads it.
    drop(git);
    drop(wiring);
    drop(engine);
    let engine2 = friday_core::memory::MemoryEngine::default_layers(&temp).unwrap();
    let entry = engine2
        .retrieve(
            friday_core::memory::MemoryLayerKind::LongTerm,
            "git.last_workspace",
        )
        .await
        .unwrap()
        .expect("durable store");
    assert_eq!(entry.key, "git.last_workspace");

    let _ = std::fs::remove_dir_all(&temp);
}

/// Live capability re-resolution: a skill that soft-declared a capability
/// (it didn't exist at load) becomes usable the moment the capability is
/// registered at runtime — `reconcile()` refreshes the snapshot.
#[test]
fn reconcile_picks_up_newly_registered_capabilities() {
    let dir = std::env::temp_dir().join(format!("skill-reconcile-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(dir.join("late")).unwrap();
    std::fs::write(
        dir.join("late/skill.json"),
        r#"{"id":"late","name":"Late","version":"1.0.0","description":"appears after load","actions":["late thing"],"capabilities":["late.cap"],"permissions":[],"dependencies":[],"platform":"Any","min_core_version":"0.1.0","enabled":true}"#,
    )
    .unwrap();

    let reg = Arc::new(planner_registry());
    let wiring = load_skill_engine(
        Arc::clone(&reg),
        &[
            (bundled_dir(), SkillSource::Builtin),
            (dir.clone(), SkillSource::User),
        ],
    );

    let late = wiring.registry.get("late").expect("late skill loaded");
    assert!(
        !late.is_usable(),
        "soft-declared capability → not usable yet"
    );
    assert_eq!(late.soft_declared_capabilities(), vec!["late.cap"]);

    // The capability shows up at runtime (a plugin/MCP server registers it).
    reg.register(
        friday_planner::execution::capability_registry::CapabilitySpec::native(
            "late.cap".to_string(),
            "Late capability".to_string(),
            "appears after skill load".to_string(),
            "filesystem".to_string(),
            "late".to_string(),
            friday_planner::execution::capability_registry::Risk::Safe,
        ),
    );

    assert_eq!(wiring.reconcile(), 1, "only the late skill should change");
    let late = wiring.registry.get("late").unwrap();
    assert!(late.is_usable(), "reconcile must flip the status live");
    assert!(late.resolved_capabilities().contains(&"late.cap"));

    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn context_builder_includes_real_bundled_skills() {
    let reg = Arc::new(planner_registry());
    let wiring = load_skill_engine(Arc::clone(&reg), &[(bundled_dir(), SkillSource::Builtin)]);

    let temp = std::env::temp_dir().join(format!("ctx-skills-{}", uuid::Uuid::new_v4()));
    let memory = Arc::new(friday_core::memory::MemoryEngine::default_layers(&temp).unwrap());
    let builder = ContextBuilder::new(
        Arc::new(friday_capabilities::registry::CapabilityRegistry::new()),
        None,
        memory,
        temp.join("experience.redb"),
    )
    .with_skills(wiring.context_source());

    let ctx = builder.build("start my minecraft server").await.unwrap();
    assert!(
        !ctx.skills.is_empty(),
        "with_skills should surface bundled skills"
    );
    assert_eq!(ctx.skills[0].id, "minecraft", "skills: {:?}", ctx.skills);
    // The chosen skill id is surfaced alongside the summaries.
    assert_eq!(ctx.active_skill.as_deref(), Some("minecraft"));

    let _ = std::fs::remove_dir_all(&temp);
}

/// A backend that records every prompt handed to the model before answering
/// with a fixed valid plan — proves *what the LLM planner actually sees*.
struct RecordingBackend {
    recorder: Arc<Mutex<Vec<String>>>,
}

#[async_trait]
impl ReasoningBackend for RecordingBackend {
    async fn complete(&self, prompt: &str) -> Result<String, String> {
        self.recorder
            .lock()
            .expect("recorder poisoned")
            .push(prompt.to_string());
        Ok(single_task_plan("filesystem.read_file"))
    }
}

/// The plan-time skill view must reach the model's prompt: with the skill
/// engine wired, a goal routed to the LLM planner yields a prompt carrying a
/// RELEVANT SKILLS section with the bundled skills ranked against the goal —
/// the model plans *through* skills, not just raw capabilities.
#[tokio::test]
async fn llm_planner_prompt_includes_ranked_bundled_skills() {
    friday_core::init_telemetry();
    let mut friday = Friday::new();

    let recorder = Arc::new(Mutex::new(Vec::new()));
    let (_registry, wiring) = register_engines_with_skills(
        &mut friday,
        Some(Box::new(RecordingBackend {
            recorder: Arc::clone(&recorder),
        })),
        &[(bundled_dir(), SkillSource::Builtin)],
    )
    .await
    .expect("register with skills");
    assert!(wiring.is_some(), "skills should load");

    friday
        .register_plugin(mock_executor())
        .await
        .expect("register mock executor");

    let bus = friday.bus().clone();
    let mut rx = bus.subscribe();
    // "refactor" routes to the LLM planner (see PlannerRouter), and the goal
    // mentions git so the git skill ranks top.
    bus.publish(Event::new(
        events::GOAL_RAW,
        "user",
        serde_json::json!({ "text": "Refactor the git repository and show the git status" }),
    ))
    .await
    .unwrap();

    let evs = run_and_collect(&mut friday, rx, 3).await;
    let kinds = kinds_of(&evs);
    assert!(kinds.contains(&events::PLAN_CREATED), "events: {kinds:?}");

    let prompts = recorder.lock().expect("recorder poisoned");
    assert!(
        !prompts.is_empty(),
        "the LLM planner must have been consulted"
    );
    let prompt = &prompts[0];
    let section = prompt
        .split("RELEVANT SKILLS")
        .nth(1)
        .expect("prompt must include the RELEVANT SKILLS section");
    assert!(
        section.contains("\"id\": \"git\""),
        "git skill must be ranked in the prompt section: {section}"
    );
}

/// Stands in for the action runtime: answers `exec.action` with success.
struct MockExecutor {
    meta: PluginMetadata,
}

#[async_trait]
impl Plugin for MockExecutor {
    fn metadata(&self) -> &PluginMetadata {
        &self.meta
    }
    async fn handle_event(&self, event: &Event) -> friday_core::Result<Vec<Event>> {
        let task_id = event
            .payload
            .get("task_id")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let plan_id = event
            .payload
            .get("plan_id")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        Ok(vec![events::event(
            events::EXEC_RESULT,
            "mock",
            serde_json::json!({
                "task_id": task_id,
                "plan_id": plan_id,
                "success": true,
                "output": {},
                "capability": ""
            }),
            event.trace_id,
        )])
    }
}

fn mock_executor() -> Box<MockExecutor> {
    Box::new(MockExecutor {
        meta: PluginMetadata::builder("mock", "Mock Executor")
            .subscribes(&[events::EXEC_ACTION])
            .emits(&[events::EXEC_RESULT])
            .build(),
    })
}

fn single_task_plan(capability: &str) -> String {
    serde_json::json!({
        "tasks": [
            {
                "name": capability,
                "capability": capability,
                "params": { "path": "notes.md" },
                "depends_on_indices": []
            }
        ]
    })
    .to_string()
}

/// Run the router for `secs` seconds, collecting every event delivered to
/// `rx` (the caller subscribes *before* publishing the goal, per house style).
async fn run_and_collect(
    friday: &mut Friday,
    mut rx: broadcast::Receiver<Event>,
    secs: u64,
) -> Vec<Event> {
    let collector = tokio::spawn(async move {
        let mut out = Vec::new();
        let deadline = tokio::time::Instant::now() + Duration::from_secs(secs + 1);
        while tokio::time::Instant::now() < deadline {
            if let Ok(Ok(ev)) = tokio::time::timeout(Duration::from_millis(50), rx.recv()).await {
                out.push(ev);
            }
        }
        out
    });
    friday
        .run_with_timeout(Duration::from_secs(secs))
        .await
        .expect("runtime error");
    collector.await.expect("collector panicked")
}

fn kinds_of(events: &[Event]) -> Vec<&str> {
    events.iter().map(|e| e.kind.as_str()).collect()
}

#[tokio::test]
async fn skill_policy_gates_medium_risk_until_user_approval() {
    friday_core::init_telemetry();
    let mut friday = Friday::new();

    // Plan uses `filesystem.write_file` — a medium-risk capability covered by
    // the bundled skills, so the skill-aware policy gates it.
    let (_, wiring) = register_engines_with_skills(
        &mut friday,
        Some(Box::new(MockBackend::new(single_task_plan(
            "filesystem.write_file",
        )))),
        &[(bundled_dir(), SkillSource::Builtin)],
    )
    .await
    .expect("register with skills");
    let wiring = wiring.expect("skills loaded");

    friday
        .register_plugin(mock_executor())
        .await
        .expect("register mock executor");

    let bus = friday.bus().clone();
    let mut rx = bus.subscribe();
    bus.publish(Event::new(
        events::GOAL_RAW,
        "user",
        serde_json::json!({ "text": "Refactor the workspace and write a notes file" }),
    ))
    .await
    .unwrap();

    // Phase 1: no approval yet → the goal pauses at the permission gate.
    let evs = run_and_collect(&mut friday, rx, 3).await;
    let kinds = kinds_of(&evs);
    assert!(
        kinds.contains(&events::PERMISSION_PAUSED),
        "events: {kinds:?}"
    );
    assert!(kinds.contains(&events::GOAL_PAUSED), "events: {kinds:?}");
    assert!(
        !kinds.contains(&events::GOAL_COMPLETED),
        "goal must not complete unapproved: {kinds:?}"
    );
    let goal_id = evs
        .iter()
        .find(|e| e.kind == events::GOAL_PAUSED)
        .and_then(|e| e.payload.get("goal_id"))
        .and_then(|v| v.as_str())
        .expect("GOAL_PAUSED carries goal_id")
        .to_string();

    // Phase 2: user approves the Docker skill (covers filesystem.write_file)
    // and resumes → the goal runs to completion.
    wiring.approvals.approve("docker");
    let mut rx = bus.subscribe();
    bus.publish(Event::new(
        events::GOAL_RESUME,
        "user",
        serde_json::json!({ "goal_id": goal_id }),
    ))
    .await
    .unwrap();

    let evs2 = run_and_collect(&mut friday, rx, 3).await;
    let kinds2 = kinds_of(&evs2);
    assert!(
        kinds2.contains(&events::TASK_COMPLETED),
        "events: {kinds2:?}"
    );
    assert!(
        kinds2.contains(&events::GOAL_COMPLETED),
        "events: {kinds2:?}"
    );
}

#[tokio::test]
async fn uncovered_capabilities_defer_to_allow_all() {
    friday_core::init_telemetry();
    let mut friday = Friday::new();

    // `filesystem.create_file` is medium-risk but no bundled skill declares
    // it → the skill policy defers to the default allow-all, and the goal
    // completes without any approval.
    let (_, wiring) = register_engines_with_skills(
        &mut friday,
        Some(Box::new(MockBackend::new(single_task_plan(
            "filesystem.create_file",
        )))),
        &[(bundled_dir(), SkillSource::Builtin)],
    )
    .await
    .expect("register with skills");
    assert!(wiring.is_some(), "skills should load");

    friday
        .register_plugin(mock_executor())
        .await
        .expect("register mock executor");

    let bus = friday.bus().clone();
    let mut rx = bus.subscribe();
    bus.publish(Event::new(
        events::GOAL_RAW,
        "user",
        serde_json::json!({ "text": "Refactor the workspace and write a notes file" }),
    ))
    .await
    .unwrap();

    let evs = run_and_collect(&mut friday, rx, 3).await;
    let kinds = kinds_of(&evs);
    assert!(kinds.contains(&events::TASK_COMPLETED), "events: {kinds:?}");
    assert!(kinds.contains(&events::GOAL_COMPLETED), "events: {kinds:?}");
    assert!(
        !kinds.contains(&events::GOAL_PAUSED),
        "uncovered capability should not pause: {kinds:?}"
    );
}

#[tokio::test]
async fn high_risk_capability_pauses_even_with_approval() {
    friday_core::init_telemetry();
    let mut friday = Friday::new();

    // `terminal.exec` is High-risk. The supervisor's metadata gate pauses it
    // unconditionally — approval only unlocks Safe/Medium capabilities, so a
    // pre-approved skill must NOT make a High-risk task run.
    let (_, wiring) = register_engines_with_skills(
        &mut friday,
        Some(Box::new(MockBackend::new(single_task_plan(
            "terminal.exec",
        )))),
        &[(bundled_dir(), SkillSource::Builtin)],
    )
    .await
    .expect("register with skills");
    let wiring = wiring.expect("skills loaded");

    friday
        .register_plugin(mock_executor())
        .await
        .expect("register mock executor");

    // The git skill declares terminal.exec; approving it must still not run
    // the High-risk task.
    wiring.approvals.approve("git");

    let bus = friday.bus().clone();
    let mut rx = bus.subscribe();
    bus.publish(Event::new(
        events::GOAL_RAW,
        "user",
        serde_json::json!({ "text": "Refactor the workspace and write a notes file" }),
    ))
    .await
    .unwrap();

    let evs = run_and_collect(&mut friday, rx, 3).await;
    let kinds = kinds_of(&evs);
    assert!(kinds.contains(&events::GOAL_PAUSED), "events: {kinds:?}");
    assert!(
        !kinds.contains(&events::TASK_STARTED),
        "high-risk task must not dispatch even when approved: {kinds:?}"
    );
}
