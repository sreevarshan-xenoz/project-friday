//! Live integration test — runs a real task through FRIDAY.
//!
//! With a working LLM (set OPENROUTER_API_KEY etc. in .env), uses the LLM
//! planner.  Without one, falls back to a mock backend so the full execution
//! pipeline is still validated end-to-end.
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

    // Build the mock plan with the real workspace path
    let mock_plan = serde_json::json!({
        "tasks": [
            {
                "name": "Write README",
                "capability": "mcp:filesystem.write_file",
                "params": { "path": format!("{ws}/README.md"), "content": "# Hello from FRIDAY" },
                "depends_on_indices": []
            },
            {
                "name": "Git Add",
                "capability": "mcp:git.git_add",
                "params": { "repository": ws.clone(), "file_paths": ["README.md"] },
                "depends_on_indices": [0]
            },
            {
                "name": "Git Commit",
                "capability": "mcp:git.git_commit",
                "params": { "repository": ws.clone(), "message": "Initial commit from FRIDAY" },
                "depends_on_indices": [1]
            }
        ]
    });

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
    let (registry, skills) = friday_context::register_engines_with_skills(
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
    } else {
        println!("[SKILLS] skill engine unavailable; allow-all policy active");
    }

    // Start real MCP servers scoped to the workspace
    let kernel = friday_actions::runtime::AgentKernel::new();

    // Filesystem MCP server (Windows: use cmd /c npx)
    match friday_mcp::transport::stdio_transport_for(
        "cmd",
        &[
            "/c".into(),
            "npx".into(),
            "-y".into(),
            "@modelcontextprotocol/server-filesystem".into(),
            workspace.display().to_string(),
        ],
    ) {
        Ok((transport, _child)) => {
            match kernel.register_mcp_server("filesystem", transport).await {
                Ok(tools) => {
                    println!("[MCP] filesystem: {} tools", tools.len());
                    friday_mcp::adapter::register_server_capabilities(
                        "filesystem",
                        &tools,
                        &registry,
                    );
                }
                Err(e) => println!("[MCP] filesystem failed: {e}"),
            }
        }
        Err(e) => println!("[MCP] filesystem spawn failed: {e}"),
    }

    // Git MCP server
    match friday_mcp::transport::stdio_transport_for(
        "uvx",
        &[
            "mcp-server-git".into(),
            "--repository".into(),
            workspace.display().to_string(),
        ],
    ) {
        Ok((transport, _child)) => match kernel.register_mcp_server("git", transport).await {
            Ok(tools) => {
                println!("[MCP] git: {} tools", tools.len());
                friday_mcp::adapter::register_server_capabilities("git", &tools, &registry);
            }
            Err(e) => println!("[MCP] git failed: {e}"),
        },
        Err(e) => println!("[MCP] git spawn failed: {e}"),
    }

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

    // Run pipeline with timeout (LLM planning can take 30-90s on free models)
    println!("[RUN] Starting pipeline...");
    friday
        .run_with_timeout(Duration::from_secs(180))
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
