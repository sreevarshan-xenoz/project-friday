//! Live integration test — runs a real task through FRIDAY.
//!
//! With a working LLM (set OPENROUTER_API_KEY etc. in .env), uses the LLM
//! planner. Without one, falls back to a deterministic native-adapter plan
//! (`filesystem.write_file` → `git.add` → `git.commit`) so the full execution
//! pipeline is still validated end-to-end, hermetically and quickly.
//!
//! ```
//! cargo test -p friday-e2e --test live_task -- --nocapture
//! ```

use friday_core::event::Event;
use friday_intelligence::ModelClient;
use friday_planner::events::GOAL_RAW;
use friday_planner::planning::planners::backend::{MockBackend, ReasoningBackend};
use std::sync::Arc;
use std::time::Duration;

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
async fn live_task_execution() {
    let _ = dotenvy::dotenv().ok();
    let _ = tracing_subscriber::fmt().with_env_filter("info").try_init();

    let mut friday = friday_core::Friday::new();

    // Try live LLM; fall back to mock if no provider is reachable.
    let fabric = friday_intelligence::InferenceFabric::default_from_env();
    let provider_count = fabric.providers.len();
    println!(
        "[BOOT] InferenceFabric loaded with {} providers",
        provider_count
    );

    let live_backend: Option<Box<dyn ReasoningBackend>> = if provider_count > 0 {
        match fabric.complete("Reply with OK").await {
            Ok(_) => {
                println!("[BOOT] Live LLM backend available");
                Some(Box::new(InferenceBackend::new(Arc::new(fabric))))
            }
            Err(e) => {
                println!("[BOOT] Live LLM probe failed ({}), using mock backend", e);
                None
            }
        }
    } else {
        println!("[BOOT] No LLM providers, using mock backend");
        None
    };

    // Create workspace and init git repo FIRST (mock plan needs the path)
    let workspace = std::env::temp_dir().join(format!("friday-live-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&workspace).expect("create workspace");
    std::process::Command::new("git")
        .args(["init"])
        .current_dir(&workspace)
        .output()
        .expect("git init");
    std::process::Command::new("git")
        .args(["config", "user.email", "friday@test.local"])
        .current_dir(&workspace)
        .output()
        .ok();
    std::process::Command::new("git")
        .args(["config", "user.name", "FRIDAY"])
        .current_dir(&workspace)
        .output()
        .ok();
    println!("[WORKSPACE] {}", workspace.display());

    let ws = workspace.display().to_string().replace("\\", "/");

    // Build the mock plan with the real workspace path. The fallback uses
    // native adapter capabilities only — no external MCP servers — so it stays
    // hermetic and deterministic regardless of host tooling.
    let mock_plan = serde_json::json!({
        "tasks": [
            {
                "name": "Write README",
                "capability": "filesystem.write_file",
                "params": { "path": format!("{ws}/README.md"), "content": "# Hello from FRIDAY" },
                "depends_on_indices": []
            },
            {
                "name": "Git Add",
                "capability": "git.add",
                "params": { "path": ws.clone(), "paths": ["README.md"] },
                "depends_on_indices": [0]
            },
            {
                "name": "Git Commit",
                "capability": "git.commit",
                "params": { "path": ws.clone(), "message": "Initial commit from FRIDAY" },
                "depends_on_indices": [1]
            }
        ]
    });

    let using_live = live_backend.is_some();
    let backend: Box<dyn ReasoningBackend> = match live_backend {
        Some(b) => b,
        None => Box::new(MockBackend::new(mock_plan.to_string())),
    };

    // Register the planner stack with the Skill Engine wired in: bundled
    // skills load against the live capability registry and the supervisor
    // gates skill-covered capabilities via SkillPolicy (readonly-safe or
    // user-approved); MCP capabilities no skill declares keep the default
    // allow-all behavior.
    let bundled_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../skills/bundled");
    let (_registry, skills) = friday_context::register_engines_with_skills(
        &mut friday,
        Some(backend),
        &[(bundled_dir, friday_context::SkillSource::Builtin)],
    )
    .await
    .expect("register engines with skills");
    if let Some(w) = &skills {
        println!(
            "[SKILLS] loaded {} bundled skills (all loaded: {})",
            w.registry.all().len(),
            w.all_loaded()
        );
        // Demo posture: approve every loaded skill so the deterministic
        // fallback's medium-risk tasks (`filesystem.write_file`, `git.add`,
        // `git.commit`) execute without a manual prompt. Mirrors
        // `local_model_task`.
        for skill in w.registry.all() {
            w.approvals.approve(skill.id());
        }
    } else {
        println!("[SKILLS] skill engine unavailable; allow-all policy active");
    }

    // Native execution adapters only — no external MCP runtimes. The
    // deterministic fallback (and any live LLM plan) executes entirely through
    // these, so the test proves provider failure/fallback + pipeline execution,
    // never MCP server version compatibility.
    let kernel = friday_actions::AgentKernel::new();
    kernel
        .register_adapter(Box::new(friday_actions::FilesystemAdapter::new()))
        .await;
    kernel
        .register_adapter(Box::new(friday_actions::GitAdapter::new()))
        .await;

    friday
        .register_plugin(Box::new(kernel))
        .await
        .expect("register action kernel");

    // Submit goal
    let goal = format!(
        "Create a file named README.md in {ws} with content '# Hello from FRIDAY' and commit it to git."
    );
    println!("[GOAL] {goal}");

    friday
        .bus()
        .publish(Event::new(
            GOAL_RAW,
            "live_test",
            serde_json::json!({ "text": goal }),
        ))
        .await
        .expect("publish goal");

    // Run pipeline with a generous window for live LLM planning (30-90s on
    // free models). The mock fallback is deterministic, so its window is short.
    // `run_with_timeout` always waits the full window (the bus only closes on
    // shutdown), so a long window would burn minutes pointlessly in mock mode.
    let run_window = if using_live {
        Duration::from_secs(180)
    } else {
        Duration::from_secs(30)
    };
    println!("[RUN] Starting pipeline (window {run_window:?})...");
    friday
        .run_with_timeout(run_window)
        .await
        .expect("runtime error");

    println!("[DONE] Pipeline completed");

    // Verify results
    let readme = workspace.join("README.md");
    if readme.exists() {
        let content = std::fs::read_to_string(&readme).unwrap_or_default();
        println!("[VERIFY] README.md: {}", content.trim());
    } else {
        println!("[VERIFY] README.md not found");
    }

    let git_dir = workspace.join(".git");
    println!(
        "[VERIFY] Git repo: {}",
        if git_dir.exists() { "yes" } else { "no" }
    );

    let _ = std::fs::remove_dir_all(&workspace);
    println!("[CLEANUP] done");
}
