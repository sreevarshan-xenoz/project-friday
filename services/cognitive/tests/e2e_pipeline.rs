use friday_cognitive::recorder::EventRecorder;
use friday_cognitive::workspace::{EngineId, GlobalWorkspace, Topic};
use friday_cognitive::WorkspaceEvent;
use friday_memory::{spawn_watcher, MemoryWatcher};
use friday_sensors::{MockSensor, SensorLoop};

/// End-to-end proof: Window → Sensor → Workspace → Memory → Recorder → Replay.
#[tokio::test]
async fn e2e_window_change_to_memory_episode() {
    let dir = std::env::temp_dir().join(format!("friday_e2e_{}", uuid::Uuid::new_v4()));
    let workspace = GlobalWorkspace::with_recorder(&dir).unwrap();
    let watcher = spawn_watcher(workspace.clone());

    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    let mut sensor_loop = SensorLoop::new();
    sensor_loop.add(Box::new(MockSensor::new(
        "mock_window",
        vec![
            WorkspaceEvent::new(
                Topic::WindowFocusChanged,
                serde_json::json!({"title": "main.rs — VS Code", "pid": 1234}),
            ),
            WorkspaceEvent::new(
                Topic::WindowFocusChanged,
                serde_json::json!({"title": "Budget.xlsx — Excel", "pid": 5678}),
            ),
            WorkspaceEvent::new(
                Topic::WindowFocusChanged,
                serde_json::json!({"title": "FRIDAY — Terminal", "pid": 9012}),
            ),
        ],
    )));

    for _ in 0..3 {
        sensor_loop.poll_all(&workspace).await;
    }

    // Verify workspace state.
    let history = workspace.history(None);
    assert_eq!(history.len(), 3);
    for event in &history {
        assert!(event.timestamp_ms > 0);
        assert!(!event.id.is_nil());
        assert_eq!(event.schema_version, 1);
    }

    // Verify memory episodes.
    for _ in 0..20 {
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        if watcher.lock().unwrap().episode_count() >= 3 {
            break;
        }
    }
    {
        let w = watcher.lock().unwrap();
        assert_eq!(w.episode_count(), 3);

        let eps = w.episodes();
        assert_eq!(eps[0].event.payload["title"], "main.rs — VS Code");
        assert_eq!(eps[1].event.payload["title"], "Budget.xlsx — Excel");
        assert_eq!(eps[2].event.payload["title"], "FRIDAY — Terminal");

        // Episodes preserve original event IDs.
        assert_eq!(eps[0].event.id, history[0].id);
        assert_eq!(eps[1].event.id, history[1].id);
        assert_eq!(eps[2].event.id, history[2].id);
    }

    // Verify recorder persisted to JSONL.
    let recorder = EventRecorder::new(&dir).unwrap();
    let recorded = recorder.read_recent(100).unwrap();
    assert_eq!(recorded.len(), 3);
    assert_eq!(recorded[0].id, history[0].id);

    std::fs::remove_dir_all(&dir).ok();
}

/// THE REPRODUCIBILITY PROOF.
///
/// Live run → JSONL → Replay → Memory → identical episodes.
/// Run twice: both produce the exact same result.
#[tokio::test]
async fn e2e_replay_produces_identical_episodes() {
    // ── Phase 1: Live run ──────────────────────────────────────────
    let dir = std::env::temp_dir().join(format!("friday_repro_{}", uuid::Uuid::new_v4()));
    let workspace = GlobalWorkspace::with_recorder(&dir).unwrap();

    // Publish a deterministic sequence of events.
    let titles = vec!["Alpha", "Bravo", "Charlie"];
    for title in &titles {
        workspace.publish_from(
            Topic::WindowFocusChanged,
            EngineId::Perception,
            serde_json::json!({"title": title}),
        );
    }

    // Observe live episodes.
    let mut watcher_live = MemoryWatcher::new();
    for event in workspace.history(None) {
        watcher_live.observe_event(event.clone());
    }
    let live_eps = watcher_live.episodes().to_vec();

    // ── Phase 2: Replay from JSONL ─────────────────────────────────
    let recorder = EventRecorder::new(&dir).unwrap();
    let replay_eps = MemoryWatcher::replay_from_recorder(&recorder).unwrap();

    // ── Phase 3: Verify identical ──────────────────────────────────
    assert_eq!(
        live_eps.len(),
        replay_eps.len(),
        "Live and replay should produce same number of episodes"
    );

    for (i, (live, replay)) in live_eps.iter().zip(replay_eps.iter()).enumerate() {
        // Same event content.
        assert_eq!(
            live.event.id, replay.event.id,
            "Episode {}: event ID mismatch",
            i
        );
        assert_eq!(
            live.event.topic, replay.event.topic,
            "Episode {}: topic mismatch",
            i
        );
        assert_eq!(
            live.event.payload, replay.event.payload,
            "Episode {}: payload mismatch",
            i
        );
        assert_eq!(
            live.event.timestamp_ms, replay.event.timestamp_ms,
            "Episode {}: timestamp mismatch",
            i
        );

        // Same memory metadata.
        assert_eq!(
            live.meta.id, replay.meta.id,
            "Episode {}: meta ID mismatch",
            i
        );
        assert_eq!(
            live.meta.importance, replay.meta.importance,
            "Episode {}: importance mismatch",
            i
        );
        assert_eq!(
            live.meta.consolidated, replay.meta.consolidated,
            "Episode {}: consolidated mismatch",
            i
        );
    }

    // ── Phase 4: Run again — still identical ───────────────────────
    let replay_eps_2 = MemoryWatcher::replay_from_recorder(&recorder).unwrap();
    assert_eq!(replay_eps.len(), replay_eps_2.len());
    for (i, (a, b)) in replay_eps.iter().zip(replay_eps_2.iter()).enumerate() {
        assert_eq!(
            a.event.id, b.event.id,
            "Second replay: event ID mismatch at {}",
            i
        );
        assert_eq!(
            a.meta.id, b.meta.id,
            "Second replay: meta ID mismatch at {}",
            i
        );
    }

    std::fs::remove_dir_all(&dir).ok();
}

/// Prove that the subscription mechanism delivers events in order.
#[tokio::test]
async fn e2e_subscription_ordering() {
    let workspace = GlobalWorkspace::new();
    let watcher = spawn_watcher(workspace.clone());

    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    let titles = vec!["Alpha", "Bravo", "Charlie", "Delta", "Echo"];
    for title in &titles {
        workspace.publish_from(
            Topic::WindowFocusChanged,
            EngineId::Perception,
            serde_json::json!({"title": title}),
        );
    }

    for _ in 0..20 {
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        if watcher.lock().unwrap().episode_count() >= 5 {
            break;
        }
    }

    let w = watcher.lock().unwrap();
    assert_eq!(w.episode_count(), 5);

    let eps = w.episodes();
    for (i, title) in titles.iter().enumerate() {
        assert_eq!(
            eps[i].event.payload["title"], *title,
            "Episode {} should have title '{}'",
            i, title
        );
    }
}
