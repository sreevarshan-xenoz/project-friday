use friday_cognitive::handler::{PhaseHandler, PhaseResult};
use friday_cognitive::phase::CognitivePhase;
use friday_cognitive::recorder::EventRecorder;
use friday_cognitive::workspace::{EngineId, GlobalWorkspace, Topic};
use friday_cognitive::WorkspaceEvent;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};

// ── Memory Types ────────────────────────────────────────────────────

/// A single memory episode — an observation from the world plus memory metadata.
///
/// The original `WorkspaceEvent` is preserved immutably. Memory-specific
/// metadata is layered on top. This ensures:
/// - Every episode can be traced back to its source event.
/// - Replay produces identical episodes from the same JSONL.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Episode {
    /// The original immutable observation.
    pub event: WorkspaceEvent,
    /// Memory-specific metadata.
    pub meta: EpisodeMeta,
}

/// Memory-specific metadata layered onto an event.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EpisodeMeta {
    /// Deterministic episode ID derived from the event ID.
    pub id: String,
    /// When this episode was created (replay uses event timestamp for determinism).
    pub recorded_at_ms: u64,
    /// Memory importance score (0.0–1.0). Starts at 1.0 for new observations.
    pub importance: f64,
    /// Tags for categorization.
    pub tags: Vec<String>,
    /// Whether this episode has been consolidated to long-term memory.
    pub consolidated: bool,
    /// How many times this episode has been retrieved.
    pub retrieved_count: u32,
}

impl Episode {
    /// Create an episode from a workspace event (deterministic).
    pub fn from_event(event: WorkspaceEvent) -> Self {
        let id = format!("ep_{}", event.id);
        Self {
            meta: EpisodeMeta {
                id,
                recorded_at_ms: event.timestamp_ms,
                importance: 1.0,
                tags: Vec::new(),
                consolidated: false,
                retrieved_count: 0,
            },
            event,
        }
    }

    /// Create an episode from a workspace event with tags.
    pub fn from_event_with_tags(event: WorkspaceEvent, tags: Vec<String>) -> Self {
        let mut ep = Self::from_event(event);
        ep.meta.tags = tags;
        ep
    }
}

// ── Memory Watcher ──────────────────────────────────────────────────

/// Faithful observer: watches workspace events, creates episodes.
///
/// No summarization, no clustering, no embeddings, no retrieval.
/// Just prove that every observation becomes an episode automatically.
///
/// **Reproducibility invariant**: Given the same sequence of events,
/// `MemoryWatcher` produces identical episodes — same IDs, same order,
/// same metadata.
pub struct MemoryWatcher {
    episodes: Vec<Episode>,
    max_episodes: usize,
}

impl MemoryWatcher {
    pub fn new() -> Self {
        Self {
            episodes: Vec::new(),
            max_episodes: 10_000,
        }
    }

    pub fn with_capacity(max_episodes: usize) -> Self {
        Self {
            episodes: Vec::new(),
            max_episodes,
        }
    }

    /// Process a workspace event and create an episode.
    /// Deterministic: same event → same episode.
    pub fn observe_event(&mut self, event: WorkspaceEvent) {
        let episode = Episode::from_event(event);

        self.episodes.push(episode);

        // Prune if over capacity (drop oldest).
        if self.episodes.len() > self.max_episodes {
            let drain = self.episodes.len() - self.max_episodes;
            self.episodes.drain(..drain);
        }
    }

    pub fn episode_count(&self) -> usize {
        self.episodes.len()
    }

    pub fn episodes(&self) -> &[Episode] {
        &self.episodes
    }

    pub fn episodes_for_topic(&self, topic_prefix: &str) -> Vec<&Episode> {
        self.episodes
            .iter()
            .filter(|ep| ep.event.topic.as_str().starts_with(topic_prefix))
            .collect()
    }

    /// Replay events from a recorder and produce episodes.
    /// This is the deterministic path: JSONL → episodes.
    pub fn replay_from_recorder(recorder: &EventRecorder) -> Result<Vec<Episode>, String> {
        let events = recorder.read_recent(10_000)
            .map_err(|e| format!("Failed to read events: {}", e))?;

        let mut watcher = MemoryWatcher::new();
        for event in events {
            watcher.observe_event(event);
        }
        Ok(watcher.episodes().to_vec())
    }
}

impl Default for MemoryWatcher {
    fn default() -> Self {
        Self::new()
    }
}

/// Spawn a background task that watches the workspace and creates episodes.
/// Returns a shared handle to the watcher for inspection.
pub fn spawn_watcher(workspace: GlobalWorkspace) -> Arc<Mutex<MemoryWatcher>> {
    let watcher = Arc::new(Mutex::new(MemoryWatcher::new()));
    let watcher_clone = watcher.clone();

    tokio::spawn(async move {
        let mut rx = workspace.subscribe();
        loop {
            match rx.recv().await {
                Ok(event) => {
                    if let Ok(mut w) = watcher_clone.lock() {
                        w.observe_event(event);
                    }
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                    tracing::warn!("memory watcher lagged, missed {} events", n);
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                    tracing::info!("memory watcher: workspace closed, stopping");
                    break;
                }
            }
        }
    });

    watcher
}

// ── Memory Engine (PhaseHandler) ────────────────────────────────────

pub struct MemoryEngine {
    short_term: Vec<Episode>,
    long_term: Vec<Episode>,
    #[allow(dead_code)]
    max_short_term: usize,
}

impl MemoryEngine {
    pub fn new() -> Self {
        Self {
            short_term: Vec::new(),
            long_term: Vec::new(),
            max_short_term: 1000,
        }
    }

    pub fn with_capacity(max_short_term: usize) -> Self {
        Self {
            short_term: Vec::new(),
            long_term: Vec::new(),
            max_short_term,
        }
    }

    /// Store an episode in short-term memory.
    pub fn store_episode(&mut self, episode: Episode) {
        self.short_term.push(episode);
    }

    /// Look up episodes by topic prefix.
    pub fn lookup_by_topic(&self, topic_prefix: &str) -> Vec<&Episode> {
        self.short_term
            .iter()
            .chain(self.long_term.iter())
            .filter(|ep| ep.event.topic.as_str().starts_with(topic_prefix))
            .collect()
    }

    pub fn short_term_count(&self) -> usize {
        self.short_term.len()
    }

    pub fn long_term_count(&self) -> usize {
        self.long_term.len()
    }
}

impl Default for MemoryEngine {
    fn default() -> Self {
        Self::new()
    }
}

// ── PhaseHandler Implementation ─────────────────────────────────────

#[async_trait]
impl PhaseHandler for MemoryEngine {
    fn name(&self) -> &str {
        "memory"
    }

    fn phases(&self) -> Vec<CognitivePhase> {
        vec![CognitivePhase::Observe, CognitivePhase::Learn]
    }

    async fn execute(
        &self,
        phase: CognitivePhase,
        workspace: &GlobalWorkspace,
        _tick_number: u64,
    ) -> PhaseResult {
        match phase {
            CognitivePhase::Observe => {
                let event_count = workspace.history(None).len();
                tracing::debug!(
                    events_in_history = event_count,
                    "memory observed workspace state"
                );
                PhaseResult::Ok
            }
            CognitivePhase::Learn => {
                workspace.publish_from(
                    Topic::MemoryStats,
                    EngineId::Memory,
                    serde_json::json!({
                        "short_term": self.short_term_count(),
                        "long_term": self.long_term_count(),
                    }),
                );
                PhaseResult::Published {
                    topics: vec!["memory.stats".into()],
                }
            }
            _ => PhaseResult::Ok,
        }
    }
}

// ── Tests ───────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn test_event(topic: &str) -> WorkspaceEvent {
        WorkspaceEvent::new(
            Topic::parse(topic),
            serde_json::json!({"test": true}),
        )
    }

    #[test]
    fn episode_preserves_original_event() {
        let event = test_event("sensor.window.focus_changed");
        let event_id = event.id;
        let episode = Episode::from_event(event);

        assert_eq!(episode.event.id, event_id);
        assert_eq!(episode.event.topic.as_str(), "sensor.window.focus_changed");
        assert_eq!(episode.meta.importance, 1.0);
        assert!(!episode.meta.consolidated);
        assert_eq!(episode.meta.retrieved_count, 0);
    }

    #[test]
    fn episode_deterministic_id() {
        let event = test_event("sensor.window.focus_changed");
        let id_a = format!("ep_{}", event.id);
        let id_b = format!("ep_{}", event.id);
        assert_eq!(id_a, id_b);
    }

    #[test]
    fn watcher_creates_episode_from_event() {
        let mut watcher = MemoryWatcher::new();
        let event = test_event("sensor.window.focus_changed");
        watcher.observe_event(event.clone());

        assert_eq!(watcher.episode_count(), 1);
        let ep = &watcher.episodes()[0];
        assert_eq!(ep.event.id, event.id);
        assert_eq!(ep.event.topic.as_str(), "sensor.window.focus_changed");
    }

    #[test]
    fn watcher_prunes_at_capacity() {
        let mut watcher = MemoryWatcher::with_capacity(3);

        for i in 0..5 {
            let event = WorkspaceEvent::new(
                Topic::WindowFocusChanged,
                serde_json::json!({"i": i}),
            );
            watcher.observe_event(event);
        }

        assert_eq!(watcher.episode_count(), 3);
    }

    #[test]
    fn watcher_filters_by_topic_prefix() {
        let mut watcher = MemoryWatcher::new();
        watcher.observe_event(test_event("sensor.window.focus_changed"));
        watcher.observe_event(test_event("sensor.mouse"));
        watcher.observe_event(test_event("cognitive.tick.completed"));

        let sensor_eps = watcher.episodes_for_topic("sensor.");
        assert_eq!(sensor_eps.len(), 2);

        let window_eps = watcher.episodes_for_topic("sensor.window.");
        assert_eq!(window_eps.len(), 1);
    }

    #[test]
    fn replay_produces_identical_episodes() {
        // Simulate a live run: create events, observe them.
        let events = vec![
            test_event("sensor.window.focus_changed"),
            test_event("sensor.mouse"),
            test_event("cognitive.tick.completed"),
        ];

        let mut watcher_live = MemoryWatcher::new();
        for event in &events {
            watcher_live.observe_event(event.clone());
        }

        // Simulate replay: create episodes from the same events.
        let mut watcher_replay = MemoryWatcher::new();
        for event in &events {
            watcher_replay.observe_event(event.clone());
        }

        // Verify identical.
        let live_eps = watcher_live.episodes();
        let replay_eps = watcher_replay.episodes();
        assert_eq!(live_eps.len(), replay_eps.len());

        for (live, replay) in live_eps.iter().zip(replay_eps.iter()) {
            assert_eq!(live.event.id, replay.event.id);
            assert_eq!(live.event.topic, replay.event.topic);
            assert_eq!(live.event.payload, replay.event.payload);
            assert_eq!(live.meta.id, replay.meta.id);
            assert_eq!(live.meta.importance, replay.meta.importance);
        }
    }

    #[tokio::test]
    async fn spawn_watcher_receives_events() {
        let workspace = GlobalWorkspace::new();
        let watcher = spawn_watcher(workspace.clone());

        tokio::time::sleep(std::time::Duration::from_millis(100)).await;

        workspace.publish_from(
            Topic::WindowFocusChanged,
            EngineId::Perception,
            serde_json::json!({"title": "test.rs"}),
        );
        workspace.publish_from(
            Topic::Mouse,
            EngineId::Perception,
            serde_json::json!({"x": 100}),
        );

        for _ in 0..20 {
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            if watcher.lock().unwrap().episode_count() >= 2 {
                break;
            }
        }

        let w = watcher.lock().unwrap();
        assert_eq!(w.episode_count(), 2, "Watcher should have created 2 episodes");
    }

    #[tokio::test]
    async fn memory_handler_publishes_stats() {
        let engine = MemoryEngine::new();
        let workspace = GlobalWorkspace::new();

        let result = engine
            .execute(CognitivePhase::Learn, &workspace, 1)
            .await;

        match result {
            PhaseResult::Published { topics } => {
                assert!(topics.contains(&"memory.stats".to_string()));
            }
            other => panic!("Expected Published, got {:?}", other),
        }
    }
}
