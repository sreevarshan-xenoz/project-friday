use chrono::Utc;
use std::io::Write;
use std::path::Path;
use std::sync::Arc;
use std::time::Instant;
use uuid::Uuid;

use friday_actions::adapters::{FilesystemAdapter, GitAdapter};
use friday_actions::request::ActionRequest;
use friday_actions::runtime::AgentKernel;
use friday_capabilities::registry::CapabilityRegistry;
use friday_context::ContextBuilder;
use friday_core::memory::{ExecutionExperience, ExperienceTask, MemoryEngine};
use friday_intelligence::{
    InferenceFabric, IntelligenceRouter, MockModelClient, ModelClient, ModelProvider,
    PromptCompiler, ResponseInterpreter,
};
use friday_simulation::simulator::SimulationEngine;
use friday_windows::ipc::MockTransport;
use friday_windows::protocol::{BridgeFrame, BridgeResponse};
use friday_windows::WindowsAdapter;

use crate::trace::ExecutionTrace;

/// Run a vertical slice execution end-to-end, recording telemetry into an ExecutionTrace.
pub async fn run_vertical_slice(
    goal: &str,
    mock_llm_json: &str,
    temp_dir: &Path,
) -> Result<ExecutionTrace, String> {
    // 1. Initialize Registry
    let registry = Arc::new(CapabilityRegistry::new());

    // Register real capability definitions
    for cap in friday_actions::adapters::filesystem::capabilities() {
        registry.register("filesystem", cap);
    }
    for cap in friday_actions::adapters::git::capabilities() {
        registry.register("git", cap);
    }
    for cap in friday_windows::capabilities() {
        registry.register("windows", cap);
    }

    // 2. Initialize Memory
    let memory = Arc::new(MemoryEngine::default_layers(temp_dir).map_err(|e| e.to_string())?);

    // 3. Initialize Context Builder
    let exp_db_path = temp_dir.join("experience.redb");
    let context_builder =
        ContextBuilder::new(registry.clone(), None, memory.clone(), exp_db_path.clone());

    // 4. Initialize Kernel Runtime & Adapters
    let kernel = Arc::new(AgentKernel::new());
    kernel
        .register_adapter(Box::new(FilesystemAdapter::new()))
        .await;
    kernel.register_adapter(Box::new(GitAdapter::new())).await;

    // Windows adapter mock named-pipe transport
    let windows_mock = Arc::new(MockTransport::new(Arc::new(|_frame: BridgeFrame| {
        BridgeResponse {
            ok: true,
            error: None,
            output: serde_json::json!({ "opened": true }),
            meta: None,
        }
    })));
    kernel
        .register_adapter(Box::new(WindowsAdapter::new(windows_mock)))
        .await;

    // 5. Cognitive pipeline execution
    let _router_mode = IntelligenceRouter::route(goal);

    // Build context
    let context = context_builder.build(goal).await?;

    // Prompt compilation
    let ast = PromptCompiler::compile_ast(&context);
    let provider = ModelProvider::Gemini;
    let compiled_prompt = PromptCompiler::compile_to_string(&ast, provider);

    // LLM Completion
    let raw_llm_response = if mock_llm_json.is_empty() {
        let fabric = InferenceFabric::default_from_env();
        fabric.complete(&compiled_prompt).await?
    } else {
        let model = MockModelClient {
            response: mock_llm_json.to_string(),
        };
        model.complete(&compiled_prompt).await?
    };

    // Response Interpretation & Ontology Verification
    let mut known_caps = Vec::new();
    for (_, cap) in registry.all() {
        known_caps.push(cap.id.clone());
        for action in &cap.actions {
            known_caps.push(format!("{}.{}", cap.id, action));
        }
    }
    let parsed_llm_response = ResponseInterpreter::parse_response(&raw_llm_response, &known_caps)?;

    // Planner Graph generation
    let task_graph = ResponseInterpreter::compile_to_task_graph(&parsed_llm_response);

    // 6. Simulation & Optimization
    let providers = std::collections::HashMap::new();
    let world_state = friday_capabilities::sim::WorldState::from_registry(&registry);
    let simulator = SimulationEngine::new(registry.clone(), providers, world_state);

    let sim_report = simulator
        .simulate_plan(&task_graph)
        .map_err(|e| format!("Simulation failed: {}", e))?;

    let final_task_graph = if sim_report.issues.is_empty() {
        let (opt_graph, _diff) = friday_simulation::optimize(&task_graph, &registry);
        opt_graph
    } else {
        task_graph.clone()
    };

    // 6.5 Interactive Approval (Human Collaboration)
    if mock_llm_json.is_empty()
        && (sim_report.risk == friday_simulation::RiskLevel::High
            || sim_report.confidence.confidence < 0.8)
    {
        println!("\n[Warning] Plan execution requires human approval.");
        println!(
            "  - Reason: Risk level is {:?}, Confidence is {:.1}%",
            sim_report.risk,
            sim_report.confidence.confidence * 100.0
        );
        println!("  - Proposed Tasks:");
        for task in &final_task_graph.tasks {
            println!("    * {} ({})", task.name, task.capability);
        }
        print!("Proceed with execution? (y/N): ");
        let _ = std::io::stdout().flush();
        let mut input = String::new();
        let _ = std::io::stdin().read_line(&mut input);
        if !input.trim().eq_ignore_ascii_case("y") {
            return Err("Execution rejected by user".to_string());
        }
    }

    // 7. Execution Loop (topological walk)
    let levels = friday_planner::levels(&final_task_graph)?;
    let mut task_durations_ms = Vec::new();
    let mut execution_errors = Vec::new();
    let mut execution_success = true;

    let start_time = Instant::now();

    for level in levels {
        for task_id in level {
            if let Some(task) = final_task_graph.task(task_id) {
                // Parse namespace out of capability
                let parts: Vec<&str> = task.capability.split('.').collect();
                let tool = parts[0];
                let action = parts.get(1).copied().unwrap_or("");

                // Prepare ActionRequest
                let req = ActionRequest::new(tool, action, task.params.clone());
                let ctx = friday_actions::kernel::ExecutionContext::builder().build();

                let task_start = Instant::now();
                let result = kernel.run_action(req, ctx).await;
                let task_elapsed = task_start.elapsed().as_millis() as u64;

                task_durations_ms.push((task.name.clone(), task_elapsed));

                if !result.success {
                    execution_success = false;
                    if let Some(err) = result.error {
                        execution_errors.push(format!("Task '{}' failed: {}", task.name, err));
                    } else {
                        execution_errors.push(format!("Task '{}' failed", task.name));
                    }
                }
            }
        }
    }

    let actual_duration_ms = start_time.elapsed().as_millis() as u64;

    // 8. Record procedural memory in ExperienceEngine
    let exp_id = Uuid::new_v4();
    let mut experience_tasks = Vec::new();
    for (name, duration) in &task_durations_ms {
        experience_tasks.push(ExperienceTask {
            id: Uuid::new_v4(),
            name: name.clone(),
            capability: name.clone(), // stub
            success: true,
            duration_ms: *duration,
            retry_count: 0,
            error: None,
        });
    }

    let experience = ExecutionExperience {
        experience_id: exp_id,
        intent: final_task_graph.intent.clone(),
        plan_id: final_task_graph.plan_id,
        success: execution_success,
        actual_duration_ms,
        predicted_duration_ms: sim_report.timeline_ms,
        timing_drift: if sim_report.timeline_ms > 0 {
            (actual_duration_ms as f64 - sim_report.timeline_ms as f64)
                / (sim_report.timeline_ms as f64)
        } else {
            0.0
        },
        tasks: experience_tasks,
        resource_forecast: serde_json::json!(sim_report.resource_forecast),
        resource_actual: serde_json::json!({}),
        timestamp: Utc::now(),
    };

    if let Ok(engine) = friday_core::memory::experience::ExperienceEngine::open(&exp_db_path) {
        let _ = engine.record(&experience);
    }

    Ok(ExecutionTrace {
        goal: goal.to_string(),
        context,
        compiled_prompt,
        raw_llm_response,
        parsed_llm_response,
        final_task_graph,
        execution_success,
        task_durations_ms,
        execution_errors,
    })
}
