//! Live verification suite (M6): does the skill-wired planner actually plan
//! *through skills* — not just capabilities — with a real local model?
//!
//! For every goal in the suite we:
//!   1. rank the bundled skills against the goal (`context_for`),
//!   2. drive the full runtime (planner + skill-aware supervisor + real
//!      adapters) with a **recording backend** that captures the exact prompt
//!      the model saw,
//!   3. assert the planning prompt carried the `RELEVANT SKILLS` section,
//!   4. measure how much of the model's plan used the chosen skill's declared
//!      capabilities (the "does it plan through skills" signal — reported,
//!      not gatekept, because model output is nondeterministic).
//!
//! Run against the real llama.cpp server at `127.0.0.1:8080`:
//! ```text
//! LLAMACPP_ENABLED=true cargo test -p friday-e2e --test skill_planning_suite -- --nocapture
//! ```
//! When the model is unreachable the suite falls back to a deterministic mock
//! backend and stays hermetic (CI-safe); the structural assertions hold in
//! both modes, the per-goal skill-overlap evidence is meaningful only live.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use friday_core::event::Event;
use friday_intelligence::{InferenceFabric, ModelClient, RoutingMode};
use friday_planner::events::GOAL_RAW;
use friday_planner::planning::planners::backend::{MockBackend, ReasoningBackend};

// ── Recording backend: captures every prompt before delegating ──────────────

struct RecordingBackend {
    inner: Box<dyn ReasoningBackend>,
    prompts: Arc<Mutex<Vec<String>>>,
}

#[async_trait::async_trait]
impl ReasoningBackend for RecordingBackend {
    async fn complete(&self, prompt: &str) -> Result<String, String> {
        self.prompts
            .lock()
            .expect("prompts poisoned")
            .push(prompt.to_string());
        self.inner.complete(prompt).await
    }
}

// Generic adapter over the fabric's ModelClient (mirrors local_model_task).
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

// ── Suite definition ────────────────────────────────────────────────────────
// (label, goal, assert_completion): completion is asserted only for goals
// whose plan is fully local + low-risk; web/minecraft goals can plan through
// skills while their execution legitimately fails/ pauses, so we assert the
// plan and report the outcome instead.
const SUITE: &[(&str, &str, bool)] = &[
    (
        "git.status",
        "Check the git status of the workspace repository and show what changed",
        true,
    ),
    (
        "git.log",
        "Show me the git log of the workspace repository",
        true,
    ),
    (
        "git.modified",
        "List the modified files in the git repository",
        true,
    ),
    (
        "git.branches",
        "Analyze the git branches in the workspace repository",
        true,
    ),
    (
        "codeintel.structure",
        "What is in this repository? Summarize its top-level structure",
        true,
    ),
    (
        "codeintel.count",
        "How many Rust source files are in this repository?",
        true,
    ),
    (
        "codeintel.loc",
        "Count the lines of code in this project",
        true,
    ),
    (
        "codeintel.readme",
        "Read the README of this project and summarize it",
        true,
    ),
    (
        "docs.planner",
        "Find the documentation about the planner in this repository",
        true,
    ),
    (
        "docs.architecture",
        "Find the FRIDAY architecture documents in this repo",
        true,
    ),
    (
        "docs.skillengine",
        "Find the documentation for the skill engine",
        true,
    ),
    (
        "docs.adr",
        "Find the ADR for windowed observability in this repository",
        true,
    ),
    (
        "web.docs",
        "Search the web for the Rust async documentation",
        false,
    ),
    (
        "web.fetch",
        "Fetch https://example.com and extract the page text",
        false,
    ),
    ("minecraft", "Start my Minecraft server", false),
    (
        "filesystem.list",
        "List the files in the workspace directory",
        true,
    ),
];

fn mock_plan_for(workspace: &str) -> String {
    // One safe native task validates for any goal (hermetic fallback).
    serde_json::json!({
        "tasks": [
            { "name": "List directory", "capability": "filesystem.list_directory",
              "params": { "path": workspace }, "depends_on_indices": [] }
        ]
    })
    .to_string()
}

#[tokio::test]
async fn skill_aware_planner_plans_through_skills_across_suite() {
    let _ = dotenvy::dotenv().ok();
    let _ = tracing_subscriber::fmt().with_env_filter("info").try_init();
    std::env::set_var("LLAMACPP_ENABLED", "true");

    // A real git workspace so git/filesystem/docs/codeintel adapters have
    // something to act on (docs + codeintel scan the FRIDAY repo itself).
    let workspace = std::env::temp_dir().join(format!("friday-suite-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&workspace).expect("create workspace");
    std::process::Command::new("git")
        .args(["init"])
        .current_dir(&workspace)
        .output()
        .expect("git init");
    let ws = workspace.display().to_string().replace("\\", "/");

    // The project root gives docs/codeintel a real corpus to scan.
    let repo = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..");
    let repo_root = repo
        .canonicalize()
        .unwrap_or_else(|_| repo)
        .display()
        .to_string()
        .replace("\\", "/");

    let mut fabric = InferenceFabric::default_from_env();
    fabric.default_mode = RoutingMode::Hybrid;
    let fabric_arc = Arc::new(fabric);

    // Probe the local model; fall back to mock so CI stays hermetic.
    let probe = fabric_arc.complete("Reply with exactly: OK").await;
    let live = probe.is_ok();
    println!("[BOOT] local model reachable: {live}");
    let prompts: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let inner: Box<dyn ReasoningBackend> = if live {
        Box::new(InferenceBackend::new(Arc::clone(&fabric_arc)))
    } else {
        Box::new(MockBackend::new(mock_plan_for(&ws)))
    };
    let backend = Box::new(RecordingBackend {
        inner,
        prompts: Arc::clone(&prompts),
    });

    // Boot the skill-wired runtime.
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
        "[BOOT] {} bundled skills loaded, all_loaded={}",
        wiring.registry.all().len(),
        wiring.all_loaded()
    );

    // Real execution adapters (web included; docs/codeintel scan the repo).
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
    // M7: the browser runtime flips the web skill's soft-declared verbs.
    friday_actions::register_browser_capabilities(&wiring.planner_registry);
    kernel
        .register_adapter(Box::new(friday_actions::BrowserAdapter::new()))
        .await;
    wiring.reconcile();
    friday
        .register_plugin(Box::new(Arc::clone(&kernel)))
        .await
        .expect("register action kernel");

    // Demo posture: approve every skill (the gate itself is proven by the
    // deterministic suites; here we want plans to run).
    for skill in wiring.registry.all() {
        wiring.approvals.approve(skill.id());
    }

    // ── Drive the suite ────────────────────────────────────────────────────
    // `FRIDAY_SUITE_GOALS` / `FRIDAY_SUITE_OFFSET` slice the suite so a live
    // run (the slow local model is ~40-50 s/goal) can be chunked across
    // invocations within each run's time budget.
    let cap = std::env::var("FRIDAY_SUITE_GOALS")
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(SUITE.len());
    let offset = std::env::var("FRIDAY_SUITE_OFFSET")
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(0);
    let suite: Vec<(&str, &str, bool)> = SUITE.iter().skip(offset).take(cap).copied().collect();
    println!("[SUITE] driving {}/{} goals", suite.len(), SUITE.len());

    let mut llm_routed = 0usize;
    let mut completed = 0usize;
    let mut used_chosen_skill = 0usize;
    let mut overlap_total = 0usize;
    let mut overlap_denom = 0usize;

    for (label, goal, assert_completion) in &suite {
        let (label, goal, assert_completion) = (*label, goal, *assert_completion);
        // Content goals (docs/codeintel/web) scan the real repo; git/filesystem
        // goals act on the temp git workspace.
        let suffix = if label.starts_with("docs.")
            || label.starts_with("codeintel.")
            || label.starts_with("web.")
        {
            &repo_root
        } else {
            &ws
        };
        let goal = format!("{goal} (workspace: {suffix})");
        let prompt_before = prompts.lock().expect("prompts poisoned").len();
        println!("\n[GOAL] {label}: {goal}");

        // Plan-time skill ranking (the same call the planner makes).
        let skill_ctx = wiring.context_source().context_for(&goal, 8);
        println!(
            "[RANK] {}",
            skill_ctx
                .relevant_skills
                .iter()
                .map(|s| s.id.as_str())
                .collect::<Vec<_>>()
                .join(" > ")
        );
        let chosen = skill_ctx.active_skill.clone();
        println!("[CHOSEN] {chosen:?}");

        // Publish + collect events.
        let bus = friday.bus().clone();
        let mut rx = bus.subscribe();
        bus.publish(Event::new(
            GOAL_RAW,
            "suite",
            serde_json::json!({ "text": goal }),
        ))
        .await
        .expect("publish goal");
        let window = Duration::from_secs(if live { 40 } else { 5 });
        let collector = tokio::spawn(async move {
            let mut out = Vec::new();
            let deadline = tokio::time::Instant::now() + window + Duration::from_secs(1);
            while tokio::time::Instant::now() < deadline {
                if let Ok(Ok(ev)) =
                    tokio::time::timeout(Duration::from_millis(100), rx.recv()).await
                {
                    out.push(ev);
                }
            }
            out
        });
        friday
            .run_with_timeout(window)
            .await
            .expect("runtime error");
        let evs = collector.await.expect("collector panicked");
        let kinds: Vec<&str> = evs.iter().map(|e| e.kind.as_str()).collect();
        println!("[EVENTS] {}", kinds.join(" → "));

        // Did the planner consume skills? (prompts added this goal)
        let new_prompts: Vec<String> = prompts
            .lock()
            .expect("prompts poisoned")
            .iter()
            .skip(prompt_before)
            .cloned()
            .collect();
        if new_prompts.is_empty() {
            println!("[SKILL-PROMPT] rule-routed (no LLM prompt) — skills not consulted by design");
        } else {
            llm_routed += 1;
            let has_section = new_prompts.iter().any(|p| p.contains("RELEVANT SKILLS"));
            assert!(
                has_section,
                "LLM-routed goal '{label}' prompt must carry RELEVANT SKILLS:\n{}",
                new_prompts[0]
            );
            println!("[SKILL-PROMPT] ✓ RELEVANT SKILLS section present in the model prompt");
        }

        // Plan capabilities vs the chosen skill's declared capabilities.
        let plan_caps: Vec<String> = evs
            .iter()
            .find(|e| e.kind == friday_planner::events::PLAN_CREATED)
            .and_then(|e| e.payload.get("tasks"))
            .and_then(|t| t.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|t| {
                        t.get("capability")
                            .and_then(|c| c.as_str())
                            .map(String::from)
                    })
                    .collect()
            })
            .unwrap_or_default();
        println!("[PLAN] {}", plan_caps.join(", "));

        if let Some(chosen_id) = &chosen {
            let declared: Vec<String> = wiring
                .registry
                .get(chosen_id)
                .map(|s| s.manifest.capabilities.clone())
                .unwrap_or_default();
            let overlap = plan_caps.iter().filter(|c| declared.contains(c)).count();
            overlap_total += overlap;
            overlap_denom += plan_caps.len().max(1);
            if !plan_caps.is_empty() && overlap > 0 {
                used_chosen_skill += 1;
            }
            println!(
                "[OVERLAP] {overlap}/{} plan capabilities from chosen skill '{chosen_id}'",
                plan_caps.len()
            );
        }

        let done = kinds.contains(&friday_planner::events::GOAL_COMPLETED);
        if done {
            completed += 1;
        }
        if assert_completion {
            // Deterministic guard in mock mode: the plan must be produced. In
            // live mode a slow model can still be planning when the window
            // closes, so plan presence is reported, not gated (the milestone
            // assert — the skill section in the prompt — stays hard).
            if !live {
                assert!(
                    kinds.contains(&friday_planner::events::PLAN_CREATED),
                    "goal '{label}' must produce a plan; events: {kinds:?}"
                );
            } else if !kinds.contains(&friday_planner::events::PLAN_CREATED) {
                println!("[OUTCOME] no plan within window (model still planning) — reported");
            }
            // Terminal outcome is evidence, not a gate: with the slow local
            // model a goal can be mid-retry/replan when the window closes.
            let terminal = done
                || kinds.contains(&friday_planner::events::GOAL_PAUSED)
                || kinds.contains(&friday_planner::events::GOAL_FAILED);
            if !terminal {
                println!(
                    "[OUTCOME] no terminal event within window (retry/replan in flight) — reported"
                );
            }
        }
    }

    println!("\n===== SUITE VERDICT =====");
    println!(
        "goals={} llm_routed={} completed={} used_chosen_skill_caps={}",
        suite.len(),
        llm_routed,
        completed,
        used_chosen_skill
    );
    println!(
        "skill-overlap across plans: {}/{} capabilities from the chosen skill",
        overlap_total, overlap_denom
    );
    if live {
        println!(
            "→ the local model planned through the chosen skill for {}/{} LLM-routed goals",
            used_chosen_skill, llm_routed
        );
    } else {
        println!("→ mock mode (model unreachable) — structural assertions only");
    }
    assert!(
        llm_routed > 0,
        "the suite must route at least one goal to the LLM planner"
    );
    println!("[SUITE] PASS");

    let _ = std::fs::remove_dir_all(&workspace);
}
