//! Live test: the skill-wired runtime driven by a REAL local LLM.
//!
//! The llama.cpp server is expected at `http://127.0.0.1:8080` (the
//! `LLAMACPP_BASE_URL` default). Run with:
//!
//! ```text
//! LLAMACPP_ENABLED=true cargo test -p friday-e2e --test local_model_task -- --nocapture
//! ```
//!
//! The fabric's default routing mode is `Free`, which excludes local
//! providers, so this test pins `RoutingMode::Hybrid` to make the local model
//! a candidate. When the model is unreachable the test falls back to a
//! deterministic mock plan so the suite stays hermetic.
//!
//! This is the "prove M5 with a real brain" path: bundled skills load against
//! the live capability registry, the supervisor policy is skill-aware, the
//! local model plans the goal, and real Git/Filesystem adapters execute it.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use friday_agent_runtime::{AgentRuntime, AgentSpec, ModelCapability};
use friday_core::event::Event;
use friday_intelligence::{InferenceFabric, ModelClient, RoutingMode};
use friday_planner::events::GOAL_RAW;
use friday_planner::planning::planners::backend::{MockBackend, ReasoningBackend};

struct InferenceBackend<C: friday_intelligence::ModelClient> {
    client: Arc<C>,
}

impl<C: friday_intelligence::ModelClient> InferenceBackend<C> {
    fn new(client: Arc<C>) -> Self {
        Self { client }
    }
}

#[async_trait::async_trait]
impl<C: friday_intelligence::ModelClient + Send + Sync> ReasoningBackend for InferenceBackend<C> {
    async fn complete(&self, prompt: &str) -> Result<String, String> {
        self.client.complete(prompt).await
    }
}

#[tokio::test]
async fn local_model_drives_skill_wired_runtime() {
    let _ = dotenvy::dotenv().ok();
    let _ = tracing_subscriber::fmt().with_env_filter("info").try_init();

    // The local llama.cpp server is already running — register the provider
    // and route to it (Hybrid mode includes Local-capable providers; Free
    // mode excludes them).
    std::env::set_var("LLAMACPP_ENABLED", "true");

    // A git repo so the real GitAdapter has something to inspect.
    let workspace = std::env::temp_dir().join(format!("friday-local-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&workspace).expect("create workspace");
    std::process::Command::new("git")
        .args(["init"])
        .current_dir(&workspace)
        .output()
        .expect("git init");
    println!("[WORKSPACE] {}", workspace.display());
    let ws = workspace.display().to_string().replace("\\", "/");

    // Deterministic fallback plan (used only when the local model is down).
    let mock_plan = serde_json::json!({
        "tasks": [
            { "name": "Check git status", "capability": "git.status",
              "params": { "path": ws }, "depends_on_indices": [] },
            { "name": "List directory", "capability": "filesystem.list_directory",
              "params": { "path": ws }, "depends_on_indices": [0] }
        ]
    });

    let mut fabric = InferenceFabric::default_from_env();
    fabric.default_mode = RoutingMode::Hybrid;
    // Only the local llama.cpp server may serve this test. With provider keys
    // in `.env`, a cloud provider would otherwise answer the probe below and
    // the test would silently become a live, non-deterministic cloud run.
    fabric.providers.retain(|name, _| name == "llamacpp");
    fabric.models.retain(|m| m.provider == "llamacpp");
    println!(
        "[BOOT] InferenceFabric loaded {} provider(s), mode Hybrid",
        fabric.providers.len()
    );

    let fabric_arc = Arc::new(fabric);
    let probe = fabric_arc.complete("Reply with exactly: OK").await;
    let backend: Box<dyn ReasoningBackend> = match probe {
        Ok(_) => {
            println!("[BOOT] local llama.cpp model reachable → live backend");
            Box::new(InferenceBackend::new(Arc::clone(&fabric_arc)))
        }
        Err(e) => {
            println!("[BOOT] local model unreachable ({e}); using mock backend");
            Box::new(MockBackend::new(mock_plan.to_string()))
        }
    };

    // Boot the planner stack with the Skill Engine wired in.
    let mut friday = friday_core::Friday::new();
    let bundled = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../skills/bundled");
    let (_registry, wiring) = friday_context::register_engines_with_skills(
        &mut friday,
        Some(backend),
        &[(bundled, friday_context::SkillSource::Builtin)],
    )
    .await
    .expect("register engines with skills");
    let wiring = wiring.expect("skills loaded");
    println!(
        "[SKILLS] {} bundled skills loaded (all_loaded: {})",
        wiring.registry.all().len(),
        wiring.all_loaded()
    );

    // M8: attach the real MemoryEngine so skills get a gated memory handle.
    let mem_engine = Arc::new(
        friday_core::memory::MemoryEngine::default_layers(&workspace.join(".memory"))
            .expect("memory layers"),
    );
    let wiring = wiring.with_memory(Arc::new(friday_context::CoreMemorySource(Arc::clone(
        &mem_engine,
    ))));

    // Real execution adapters (native, no MCP servers required). Shared as an
    // `Arc` so the same kernel serves both the event bus and the agent driver.
    let kernel = Arc::new(friday_actions::AgentKernel::new());
    kernel
        .register_adapter(Box::new(friday_actions::FilesystemAdapter::new()))
        .await;
    kernel
        .register_adapter(Box::new(friday_actions::GitAdapter::new()))
        .await;
    kernel
        .register_adapter(Box::new(friday_actions::DocsAdapter::new()))
        .await;
    kernel
        .register_adapter(Box::new(friday_actions::CodeIntelAdapter::new()))
        .await;
    kernel
        .register_adapter(Box::new(friday_actions::WebAdapter::new()))
        .await;
    // M7: the browser runtime arrives — its capabilities flip the web skill's
    // soft-declared browser verbs (open/click/type/scroll/extract/screenshot)
    // to resolved, and both adapters share the `web` tool (action-routed).
    friday_actions::register_browser_capabilities(&wiring.planner_registry);
    kernel
        .register_adapter(Box::new(friday_actions::BrowserAdapter::new()))
        .await;
    let changed = wiring.reconcile();
    println!(
        "[BROWSER] browser verbs registered; skill capabilities reconciled (changed: {changed})"
    );
    friday
        .register_plugin(Box::new(Arc::clone(&kernel)))
        .await
        .expect("register action kernel");

    // Explainability: rank the bundled skills against the goal (this also
    // emits the auditable `skill ranking` trace).
    let goal = format!(
        "Check the git status of {} and list the files in that directory",
        workspace.display()
    );
    println!("[GOAL] {goal}");
    let ctx = wiring.context_source().context_for(&goal, 8);
    println!(
        "[RANKING] {}",
        ctx.relevant_skills
            .iter()
            .map(|s| s.id.as_str())
            .collect::<Vec<_>>()
            .join(" > ")
    );

    // The auditable decision chain (Relevant → Chosen → Capabilities →
    // Permission) and the live skill-health dashboard.
    let decision = wiring.decision_for(&goal);
    println!(
        "[DECISION] chosen={:?} candidates={:?} skipped_disabled={:?}",
        decision.chosen, decision.candidates, decision.skipped
    );
    for cap in &decision.capabilities {
        println!("[DECISION]   {} → {:?}", cap.capability, cap.permission);
    }
    let health = wiring.health();
    println!(
        "[HEALTH] {}",
        health
            .iter()
            .map(|(id, h)| format!("{id}={h:?}"))
            .collect::<Vec<_>>()
            .join(" ")
    );

    // M8 demo: the git skill's gated memory handle stores + reads back its
    // declared key (git.last_workspace) through the real MemoryEngine.
    let git_memory = wiring.memory_for("git").expect("git memory handle");
    let ws_path = workspace.display().to_string();
    git_memory
        .store("git.last_workspace", &ws_path)
        .await
        .expect("store through git skill memory gate");
    let recalled = git_memory.read("git.last_workspace").await.unwrap();
    println!(
        "[MEMORY] git skill: store+read git.last_workspace = {:?}",
        recalled.map(|h| h.value)
    );

    // Demo posture: approve every loaded skill so skill-covered medium-risk
    // capabilities execute without a manual prompt (the permission gate itself
    // is proven by the deterministic tests in `friday-context`).
    for skill in wiring.registry.all() {
        wiring.approvals.approve(skill.id());
    }

    // ── Agent Runtime demo: the planner orchestrates scoped workers ──────
    // The SkillAgentDriver plans each goal through the skill engine *scoped to
    // the agent's allowed skills*, then executes through the same kernel. The
    // local model is NOT the orchestrator — the planner is; the model only
    // powers the main goal below.
    let agent_runtime = AgentRuntime::new().with_max_concurrency(2);
    let agent_driver = Arc::new(friday_actions::SkillAgentDriver::new(
        Arc::clone(&wiring.registry),
        Arc::clone(&wiring.approvals),
        Arc::clone(&wiring.planner_registry),
        Arc::clone(&kernel),
    ));

    // Worker A: read-only research (docs + codeintel skills). The readonly
    // skills auto-approve their safe capabilities, so no session approval is
    // needed. Preferred models are *constraints*, never provider names.
    let spec_research = AgentSpec::new("research", "understand the workspace repository")
        .with_allowed_skills(&["docs", "codeintel"])
        .with_preferred_models(&[ModelCapability::Local, ModelCapability::Reasoning])
        .with_memory_scope(&["private.research"])
        .with_param("codeintel.understand", serde_json::json!({ "root": ws }));

    // Worker B: approved git worker — its scope is [git] only; docs/web/etc.
    // are physically unreachable even though the goal is about a repo.
    let spec_inspect = AgentSpec::new("inspect", "check the git status of the workspace")
        .with_allowed_skills(&["git"])
        .with_param("git.status", serde_json::json!({ "path": ws }));

    let handles = agent_runtime.spawn_many(vec![spec_research, spec_inspect], agent_driver);
    let agent_results = agent_runtime.await_all(handles).await;
    let merged = agent_runtime.merge(agent_results);
    for line in merged.summary_lines() {
        println!("[AGENT] {line}");
    }
    for result in &merged.agents {
        println!(
            "[AGENT] {}: {} — steps: {}",
            result.agent_id,
            result.state,
            result
                .steps
                .iter()
                .map(|s| format!("{}: {}", s.description, s.outcome))
                .collect::<Vec<_>>()
                .join(", ")
        );
    }
    println!(
        "[MERGE] succeeded={} failed={} total_steps={}",
        merged.succeeded, merged.failed, merged.total_steps
    );
    assert!(
        merged.all_succeeded(),
        "agent demo must succeed: {:?}",
        merged.agents
    );

    let bus = friday.bus().clone();
    let mut rx = bus.subscribe();
    bus.publish(Event::new(
        GOAL_RAW,
        "local_model_test",
        serde_json::json!({ "text": goal }),
    ))
    .await
    .expect("publish goal");

    let collector = tokio::spawn(async move {
        let mut out = Vec::new();
        // ~90 s covers the probe + local-model planning (~20 s observed on
        // Gemma 4 E4B) + execution with a wide margin; the goal completes long
        // before this and `run_with_timeout` idles until it fires.
        let deadline = tokio::time::Instant::now() + Duration::from_secs(90);
        while tokio::time::Instant::now() < deadline {
            if let Ok(Ok(ev)) = tokio::time::timeout(Duration::from_millis(100), rx.recv()).await {
                out.push(ev);
            }
        }
        out
    });
    friday
        .run_with_timeout(Duration::from_secs(90))
        .await
        .expect("runtime error");
    let evs = collector.await.expect("collector panicked");

    let kinds: Vec<&str> = evs.iter().map(|e| e.kind.as_str()).collect();
    println!("[EVENTS] {}", kinds.join(" → "));

    // What the model actually planned (or the fallback plan).
    if let Some(plan) = evs
        .iter()
        .find(|e| e.kind == friday_planner::events::PLAN_CREATED)
    {
        let tasks = plan
            .payload
            .get("tasks")
            .and_then(|t| t.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|t| {
                        t.get("capability")
                            .and_then(|c| c.as_str())
                            .map(|c| c.to_string())
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        println!("[PLAN] {}", tasks.join(", "));
    }

    // The routing decision proving which provider/model served the planning
    // calls (auditable via `RoutingDecision`).
    if let Some(decision) = fabric_arc.last_decision() {
        println!(
            "[ROUTING] mode={} chosen={:?} served={:?} failures={}",
            decision.routing_mode,
            decision.chosen,
            decision.served,
            decision.failures.len()
        );
    }

    assert!(
        kinds.contains(&friday_planner::events::GOAL_COMPLETED),
        "goal did not complete; events: {kinds:?}"
    );
    assert!(
        kinds.contains(&friday_planner::events::TASK_COMPLETED),
        "no task completed; events: {kinds:?}"
    );

    let _ = std::fs::remove_dir_all(&workspace);
    println!("[CLEANUP] done");
}
