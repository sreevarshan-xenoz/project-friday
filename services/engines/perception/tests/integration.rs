use friday_cognitive::handler::FnHandler;
use friday_cognitive::phase::CognitivePhase;
use friday_cognitive::registry::PhaseRegistry;
use friday_cognitive::runtime::{CognitiveRuntime, RuntimeConfig};
use friday_cognitive::workspace::GlobalWorkspace;
use friday_perception::PerceptionEngine;
use std::sync::Arc;

/// Full integration test: Perception engine registered into cognitive runtime.
/// Proves: engine → PhaseRegistry → CognitiveRuntime → GlobalWorkspace pipeline works.
#[tokio::test]
async fn perception_engine_feeds_workspace_through_runtime() {
    let workspace = GlobalWorkspace::new();
    let mut registry = PhaseRegistry::new();

    // Register the real Perception engine.
    registry.register(Arc::new(PerceptionEngine::with_seed(42)));

    // Also register a tiny handler that reads what Perception published.
    let workspace_clone = workspace.clone();
    registry.register(Arc::new(FnHandler::new(
        "observe_reader",
        vec![CognitivePhase::Observe],
        move |_phase, _ws, _tick| {
            let ws = workspace_clone.clone();
            Box::pin(async move {
                // After Perception runs, there should be sensor events.
                let mouse = ws.get("sensor.mouse");
                let keyboard = ws.get("sensor.keyboard");
                let screen = ws.get("sensor.screen");
                let timer = ws.get("sensor.timer");

                let has_any =
                    mouse.is_some() || keyboard.is_some() || screen.is_some() || timer.is_some();
                assert!(
                    has_any,
                    "Expected at least one sensor event in workspace after Perception"
                );

                friday_cognitive::handler::PhaseResult::Ok
            })
        },
    )));

    let config = RuntimeConfig {
        verbose: false,
        ..Default::default()
    };
    let mut runtime = CognitiveRuntime::new(config, registry, workspace);

    // Run 5 ticks.
    for i in 1..=5 {
        let (tick_id, _duration) = runtime.tick(i).await;
        assert!(!tick_id.0.is_nil());
    }

    // Verify the workspace accumulated events across ticks.
    let history = runtime.workspace().history(None);
    // 5 ticks × 1 sensor event from Perception + 5 cognitive.tick.completed = 10 events minimum.
    assert!(
        history.len() >= 10,
        "Expected at least 10 workspace events (5 sensor + 5 tick.completed), got {}",
        history.len()
    );

    // Verify sensor events are present.
    let sensor_events: Vec<_> = history
        .iter()
        .filter(|e| e.topic.starts_with("sensor."))
        .collect();
    assert_eq!(
        sensor_events.len(),
        5,
        "Expected 5 sensor events (one per tick)"
    );

    // Verify tick completion events are present.
    let tick_events: Vec<_> = history
        .iter()
        .filter(|e| e.topic == friday_cognitive::Topic::TickCompleted)
        .collect();
    assert_eq!(tick_events.len(), 5, "Expected 5 tick completion events");
}

/// Verify metrics track phase execution correctly with a real engine.
#[tokio::test]
async fn runtime_metrics_reflect_perception_execution() {
    let workspace = GlobalWorkspace::new();
    let mut registry = PhaseRegistry::new();
    registry.register(Arc::new(PerceptionEngine::new()));

    let config = RuntimeConfig::default();
    let mut runtime = CognitiveRuntime::new(config, registry, workspace);

    for i in 1..=10 {
        runtime.tick(i).await;
    }

    let metrics = runtime.metrics();
    // 10 ticks × 1 handler (Perception on Observe only) = 10 total handler executions.
    assert_eq!(metrics.total_phases_executed, 10);
    // Observe phase should have 10 executions (one per tick, from Perception).
    assert_eq!(
        metrics.phase_counts.get(&CognitivePhase::Observe),
        Some(&10)
    );
    // No errors.
    assert_eq!(metrics.errors, 0);
}
