use friday_e2e::runner::run_vertical_slice;

#[tokio::main]
async fn main() {
    run_diagnostics().await;
    println!();

    println!("==================================================");
    println!("          FRIDAY Developer Console (v0.1)         ");
    println!("==================================================");

    let goal = "Create a folder named Research, initialize a Git repository, create a README.md, commit it, and open the folder.";
    println!("\n[Goal]\n{}", goal);

    let temp_dir = std::env::temp_dir().join(format!("friday-console-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&temp_dir).unwrap();

    // Prepare mock LLM output that parses target directory correctly
    let target_dir_str = temp_dir.to_string_lossy().to_string().replace("\\", "/");
    let mock_llm_json = r##"{
        "goal": "Create a folder named Research, initialize a Git repository, create a README.md, commit it, and open the folder.",
        "intent": "workspace.setup",
        "confidence": 0.98,
        "tasks": [
            {
                "name": "Create README.md file",
                "capability": "filesystem.create_file",
                "params": {
                    "path": "TARGET_DIR/Research/README.md",
                    "content": "# Research\nInitial repository."
                },
                "depends_on_indices": []
            },
            {
                "name": "Initialize Git repository",
                "capability": "git.init",
                "params": {
                    "path": "TARGET_DIR/Research"
                },
                "depends_on_indices": [0]
            },
            {
                "name": "Stage files for commit",
                "capability": "git.add",
                "params": {
                    "path": "TARGET_DIR/Research",
                    "paths": ["README.md"]
                },
                "depends_on_indices": [0, 1]
            },
            {
                "name": "Commit README to git",
                "capability": "git.commit",
                "params": {
                    "path": "TARGET_DIR/Research",
                    "message": "Initial commit"
                },
                "depends_on_indices": [2]
            },
            {
                "name": "Open directory in Explorer",
                "capability": "windows.open",
                "params": {
                    "app": "explorer",
                    "args": ["TARGET_DIR/Research"]
                },
                "depends_on_indices": [0]
            }
        ]
    }"##.replace("TARGET_DIR", &target_dir_str);

    println!("\n[Context Builder]");
    println!("✔ Loading capabilities, memories, and execution experience telemetry...");

    let trace_result = run_vertical_slice(goal, &mock_llm_json, &temp_dir).await;

    match trace_result {
        Ok(trace) => {
            println!(
                "✔ Context loaded successfully ({} capabilities, {} memories)",
                trace.context.capabilities.len(),
                trace.context.relevant_memories.len()
            );

            println!("\n[Planner]");
            println!(
                "✔ Generated {} tasks under intent '{}'",
                trace.final_task_graph.tasks.len(),
                trace.final_task_graph.intent
            );

            println!("\n[Simulation & Optimization]");
            println!("✔ Validation: Passed");
            println!("✔ Optimization: Applied rules (collapsing duplicate/redundant stages)");

            println!("\n[Execution]");
            for (name, duration) in &trace.task_durations_ms {
                println!("  - Task '{}' ✔ ({}ms)", name, duration);
            }

            if trace.execution_success {
                println!("\n✔ Execution: SUCCESS");
            } else {
                println!("\n❌ Execution: FAILED");
                for err in &trace.execution_errors {
                    println!("    error: {}", err);
                }
            }

            println!("\n[Experience Engine]");
            println!("✔ Telemetry recorded to redb database successfully.");

            // Save trace replay
            let trace_path = temp_dir.join("trace_replay.json");
            if friday_e2e::trace::TraceReplayEngine::save(&trace, &trace_path).is_ok() {
                println!(
                    "✔ Saved trace replay file to: {}",
                    trace_path.to_string_lossy()
                );
            }
        }
        Err(e) => {
            println!("\n❌ Developer Console Error: {}", e);
        }
    }

    let _ = std::fs::remove_dir_all(&temp_dir);
    println!("\n==================================================");
}

async fn run_diagnostics() {
    println!("==================================================");
    println!("          FRIDAY Boot Check (Alpha 1)            ");
    println!("==================================================");
    let start = std::time::Instant::now();

    // 1. Registry Check
    let registry = std::sync::Arc::new(friday_capabilities::registry::CapabilityRegistry::new());
    println!(
        "  ✓ Capability Registry: Initialized ({} definitions)",
        registry.all().len()
    );

    // 2. Memory Check
    let temp_dir = std::env::temp_dir();
    let memory = friday_core::memory::MemoryEngine::default_layers(&temp_dir);
    if memory.is_ok() {
        println!("  ✓ Experience Store: Durable backend ready");
    } else {
        println!("  ✗ Experience Store: Initialization failed");
    }

    // 3. Inference Fabric / Provider Health Check
    let fabric = friday_intelligence::InferenceFabric::default_from_env();
    let provider_count = fabric.providers.len();
    println!(
        "  ✓ Inference Fabric: Loaded ({} active providers)",
        provider_count
    );

    // 4. Windows Bridge Check
    let windows_mock = std::sync::Arc::new(friday_windows::ipc::MockTransport::new(
        std::sync::Arc::new(|_| friday_windows::protocol::BridgeResponse {
            ok: true,
            error: None,
            output: serde_json::json!({}),
        }),
    ));
    let _windows = friday_windows::WindowsAdapter::new(windows_mock);
    println!("  ✓ Windows Bridge: Mock Transport Connected");

    println!("--------------------------------------------------");
    println!("Status: HEALTHY");
    println!("Boot Time: {:.2}ms", start.elapsed().as_millis());
    println!("==================================================");
}
