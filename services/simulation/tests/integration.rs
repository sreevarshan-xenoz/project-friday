use std::sync::Arc;

use friday_capabilities::registry::CapabilityRegistry;
use friday_capabilities::sim::WorldState;
use friday_planner::TaskGraph;
use friday_simulation::{render_report, SimulationEngine};

#[test]
fn test_simulation_full_flow() {
    // 1. Build capability registry
    let reg = Arc::new(CapabilityRegistry::new());
    
    // Register filesystem capabilities
    for c in friday_actions::adapters::all_capabilities() {
        reg.register("friday-actions", c);
    }
    // Register windows capabilities
    for c in friday_windows::capabilities() {
        reg.register("windows-host", c);
    }

    // Combine sim registries
    let mut sims = friday_actions::sim_registry();
    for (k, v) in friday_windows::sim_registry() {
        sims.insert(k, v);
    }

    // 2. Build immutable WorldState
    let mut world = WorldState::from_registry(&reg);
    world.filesystem.path_counts.insert("Downloads".to_string(), 143);
    world.filesystem.writable = true;

    // 3. Build TaskGraph
    let mut graph = TaskGraph::new("Test Full Flow Intent");

    // Task 1: Create a file
    let t1 = graph.add_task(
        "Create readme",
        "filesystem.create_file",
        serde_json::json!({
            "path": "README.md",
            "content": "Hello world"
        }),
        &[],
    );

    // Task 2: Git Init
    let t2 = graph.add_task(
        "Git Init",
        "git.init",
        serde_json::Value::Null,
        &[],
    );

    // Task 3: Git Commit (depends on t1 and t2)
    let _t3 = graph.add_task(
        "Git Commit",
        "git.commit",
        serde_json::json!({
            "message": "Initial commit"
        }),
        &[t1, t2],
    );

    // Task 4: Attempt a browser.open action.
    // (This should fallback to windows.open because browser is not in the registry)
    let _t4 = graph.add_task(
        "Open browser search",
        "browser.open",
        serde_json::json!({
            "app": "chrome"
        }),
        &[],
    );

    // 4. Run Simulation
    let engine = SimulationEngine::new(reg.clone(), sims, world);
    let report = engine.simulate_plan(&graph).expect("simulation should succeed");

    // 5. Verify results
    assert_eq!(report.intent, "Test Full Flow Intent");
    assert_eq!(report.summary.actions, 4);

    // Fallbacks check
    assert_eq!(report.fallbacks.len(), 1);
    assert_eq!(report.fallbacks[0].from, "browser.open");
    assert_eq!(report.fallbacks[0].to, "windows.open");
    assert!(report.fallbacks[0].available);

    // Confidence check
    assert!(report.confidence.confidence > 0.0);
    assert!(report.confidence.confidence <= 1.0);
    assert!(report.confidence.assumptions.iter().any(|a| a.contains("disk writable: true")));

    // Preconditions check
    assert_eq!(report.issues.len(), 1, "Expected exactly 1 issue but got: {:?}", report.issues);
    assert_eq!(report.issues[0].kind, "impossible_action");

    // Side effects & Resource forecast check
    assert!(report.resource_forecast.cpu_pct > 0.0);
    assert!(report.resource_forecast.memory_mb > 0);
    assert!(report.resource_forecast.disk_bytes > 0);

    // Check Plan Diff (WorldState progression)
    // The "before" Downloads count was 143.
    assert_eq!(*report.world_before.filesystem.path_counts.get("Downloads").unwrap(), 143);
    // After applying the filesystem.create_file (if it target is README.md) or window launch side-effects:
    // Let's verify that a window was launched in the after state
    assert!(report.world_after.windows.iter().any(|w| w.app == "chrome"));

    // Render report (human-readable checks)
    let rendered = render_report(&report);
    assert!(rendered.contains("Simulation Report"));
    assert!(rendered.contains("Plan Summary"));
    assert!(rendered.contains("Resource Forecast"));
    assert!(rendered.contains("Predicted Timeline"));
    assert!(rendered.contains("Confidence Score"));
}

#[test]
fn test_optimizer_prune_duplicates() {
    let reg = Arc::new(CapabilityRegistry::new());
    for c in friday_actions::adapters::all_capabilities() {
        reg.register("friday-actions", c);
    }
    let sims = friday_actions::sim_registry();
    let world = WorldState::from_registry(&reg);

    let mut graph = TaskGraph::new("Prune Duplicates Intent");

    // Add two identical consecutive file creation tasks
    let t1 = graph.add_task(
        "Create file 1",
        "filesystem.create_file",
        serde_json::json!({
            "path": "test.txt",
            "content": "hello"
        }),
        &[],
    );
    let _t2 = graph.add_task(
        "Create file 2",
        "filesystem.create_file",
        serde_json::json!({
            "path": "test.txt",
            "content": "hello"
        }),
        &[t1],
    );

    let engine = SimulationEngine::new(reg, sims, world);
    let report = engine.simulate_plan(&graph).expect("simulation should succeed");

    // The optimized plan should prune the second redundant task
    let opt_plan = report.optimized_plan.expect("must have optimized plan");
    assert_eq!(opt_plan.tasks.len(), 1);
    assert_eq!(opt_plan.tasks[0].id, t1);

    let diff = report.optimizer_diff.expect("must have optimizer diff");
    assert!(diff.removed.iter().any(|r| r.contains("pruned duplicate filesystem.create_file")));
}

#[test]
fn test_optimizer_collapse_open_close() {
    let reg = Arc::new(CapabilityRegistry::new());
    for c in friday_windows::capabilities() {
        reg.register("windows-host", c);
    }
    let mut sims = friday_actions::sim_registry();
    for (k, v) in friday_windows::sim_registry() {
        sims.insert(k, v);
    }
    let world = WorldState::from_registry(&reg);

    let mut graph = TaskGraph::new("Collapse Open Close Intent");

    // We add a dummy task first, then Close, then Open.
    // Kahn's topo sort will visit the tasks in order of definition when traversing successors.
    // So by defining Close before Open, and having both depend on t_dummy,
    // Kahn's algorithm will push Close, then Open onto the queue,
    // which pops Open, then Close, resulting in a deterministic [t_dummy, Open, Close] order.
    let t_dummy = graph.add_task(
        "List windows",
        "windows.list",
        serde_json::Value::Null,
        &[],
    );
    let _t2 = graph.add_task(
        "Close window",
        "windows.close",
        serde_json::json!({
            "app": "notepad"
        }),
        &[t_dummy],
    );
    let _t1 = graph.add_task(
        "Open window",
        "windows.open",
        serde_json::json!({
            "app": "notepad"
        }),
        &[t_dummy],
    );

    let engine = SimulationEngine::new(reg, sims, world);
    let report = engine.simulate_plan(&graph).expect("simulation should succeed");

    // The optimized plan should collapse both the open and close, leaving only the dummy task
    let opt_plan = report.optimized_plan.expect("must have optimized plan");
    assert_eq!(opt_plan.tasks.len(), 1);
    assert_eq!(opt_plan.tasks[0].id, t_dummy);

    let diff = report.optimizer_diff.expect("must have optimizer diff");
    assert!(diff.collapsed.iter().any(|c| c.contains("collapsed windows.open + windows.close")));
}
