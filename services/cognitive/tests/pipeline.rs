use async_trait::async_trait;
use friday_cognitive::handler::PhaseHandler;
use friday_cognitive::phase::CognitivePhase;
use friday_cognitive::recorder::EventRecorder;
use friday_cognitive::registry::PhaseRegistry;
use friday_cognitive::runtime::{CognitiveRuntime, RuntimeConfig};
use friday_cognitive::workspace::{EngineId, GlobalWorkspace, Topic};
use friday_cognitive::WorkspaceEvent;
use std::sync::Arc;

struct TestHandler;

#[async_trait]
impl PhaseHandler for TestHandler {
    fn name(&self) -> &str {
        "test_handler"
    }
    fn phases(&self) -> Vec<CognitivePhase> {
        vec![CognitivePhase::Observe]
    }
    async fn execute(
        &self,
        _phase: CognitivePhase,
        workspace: &GlobalWorkspace,
        _tick_number: u64,
    ) -> friday_cognitive::handler::PhaseResult {
        workspace.publish_from(
            Topic::WindowFocusChanged,
            EngineId::Perception,
            serde_json::json!({
                "title": "test.rs — Visual Studio Code",
                "process_name": "Code.exe",
            }),
        );
        friday_cognitive::handler::PhaseResult::Published {
            topics: vec!["sensor.window.focus_changed".into()],
        }
    }
}

/// Full pipeline: Runtime → Workspace → Recorder → JSONL → Replay.
#[tokio::test]
async fn runtime_events_recorded_to_jsonl() {
    let dir = std::env::temp_dir().join(format!("friday_pipeline_test_{}", uuid::Uuid::new_v4()));
    let workspace = GlobalWorkspace::with_recorder(&dir).unwrap();
    let mut registry = PhaseRegistry::new();
    registry.register(Arc::new(TestHandler));

    let config = RuntimeConfig::default();
    let mut runtime = CognitiveRuntime::new(config, registry, workspace);

    // Run 5 ticks.
    for i in 1..=5 {
        runtime.tick(i).await;
    }

    // Read back from JSONL.
    let recorder = EventRecorder::new(&dir).unwrap();
    let events = recorder.read_recent(100).unwrap();

    // Should have: 5 sensor.window.focus_changed + 5 cognitive.tick.completed = 10.
    assert_eq!(events.len(), 10, "Expected 10 events in JSONL");

    let focus_events: Vec<_> = events
        .iter()
        .filter(|e| e.topic == Topic::WindowFocusChanged)
        .collect();
    assert_eq!(focus_events.len(), 5, "Expected 5 focus_changed events");

    let tick_events: Vec<_> = events
        .iter()
        .filter(|e| e.topic == Topic::TickCompleted)
        .collect();
    assert_eq!(tick_events.len(), 5, "Expected 5 tick.completed events");

    // Every event should have a valid timestamp and non-nil ID.
    for event in &events {
        assert!(event.timestamp_ms > 0, "Event should have a timestamp");
        assert!(!event.id.is_nil(), "Event should have a non-nil ID");
    }

    // Clean up.
    std::fs::remove_dir_all(&dir).ok();
}

/// JSONL file format: one JSON object per line, parseable.
#[tokio::test]
async fn jsonl_format_is_valid() {
    let dir = std::env::temp_dir().join(format!("friday_jsonl_test_{}", uuid::Uuid::new_v4()));
    let workspace = GlobalWorkspace::with_recorder(&dir).unwrap();

    workspace.publish_from(
        Topic::Mouse,
        EngineId::Perception,
        serde_json::json!({"x": 1}),
    );
    workspace.publish_from(
        Topic::TickCompleted,
        EngineId::Runtime,
        serde_json::json!({"tick": 1}),
    );

    // Read the raw JSONL file.
    let today = chrono::Utc::now().date_naive();
    let file_path = dir.join(format!("{}.jsonl", today));
    let content = std::fs::read_to_string(&file_path).unwrap();

    let lines: Vec<&str> = content.lines().collect();
    assert_eq!(lines.len(), 2, "Expected 2 lines in JSONL");

    // Each line should be valid JSON.
    for line in &lines {
        let parsed: WorkspaceEvent = serde_json::from_str(line).unwrap();
        assert!(parsed.timestamp_ms > 0);
    }

    std::fs::remove_dir_all(&dir).ok();
}
