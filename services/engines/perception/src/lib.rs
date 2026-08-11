use async_trait::async_trait;
use friday_cognitive::handler::{PhaseHandler, PhaseResult};
use friday_cognitive::phase::CognitivePhase;
use friday_cognitive::workspace::{EngineId, GlobalWorkspace, Topic};
use friday_sensors::SensorLoop;
use rand::{Rng, SeedableRng};
use tokio::sync::Mutex as AsyncMutex;

/// Perception Engine
///
/// Runs during the Observe phase. Publishes sensor events to the workspace.
///
/// Two operating modes:
///
/// - **Real sensors** — when built with [`PerceptionEngine::with_sensors`],
///   Observe polls the attached [`SensorLoop`] (screen, window focus, …) and
///   publishes whatever the environment actually reports. This is the
///   production path; the engine reports exactly the channels that fired.
/// - **Synthetic fallback** — when built with [`PerceptionEngine::new`] or
///   [`PerceptionEngine::with_seed`], it simulates random input events to
///   prove the engine→workspace pipeline works on machines with no sensor
///   harness attached.
pub struct PerceptionEngine {
    /// Optional fixed seed for reproducible synthetic tests.
    seed: Option<u64>,
    /// Real sensor loop. `None` means synthetic mode.
    sensors: Option<AsyncMutex<SensorLoop>>,
}

impl PerceptionEngine {
    /// Create a new PerceptionEngine with random behavior.
    pub fn new() -> Self {
        Self {
            seed: None,
            sensors: None,
        }
    }

    /// Create a PerceptionEngine with a fixed seed (for deterministic tests).
    pub fn with_seed(seed: u64) -> Self {
        Self {
            seed: Some(seed),
            sensors: None,
        }
    }

    /// Create a PerceptionEngine backed by a real sensor loop. On every
    /// Observe tick the attached sensors are polled and their events
    /// published to the workspace. A `Mutex` keeps the engine `Send + Sync`
    /// (the `PhaseHandler` bound) while `SensorLoop` is only `Send`.
    pub fn with_sensors(loop_: SensorLoop) -> Self {
        Self {
            seed: None,
            sensors: Some(AsyncMutex::new(loop_)),
        }
    }

    /// Whether this engine reads from a real sensor loop.
    pub fn sensors_enabled(&self) -> bool {
        self.sensors.is_some()
    }
}

impl Default for PerceptionEngine {
    fn default() -> Self {
        Self::new()
    }
}

/// Types of synthetic sensor data the Perception engine can emit.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum SensorEvent {
    Mouse {
        x: i32,
        y: i32,
        button: String,
    },
    Keyboard {
        key: String,
        action: String,
    },
    Screen {
        region: String,
        change_detected: bool,
    },
    Timer {
        source: String,
        interval_ms: u64,
    },
}

#[async_trait]
impl PhaseHandler for PerceptionEngine {
    fn name(&self) -> &str {
        "perception"
    }

    fn phases(&self) -> Vec<CognitivePhase> {
        vec![CognitivePhase::Observe]
    }

    async fn execute(
        &self,
        _phase: CognitivePhase,
        workspace: &GlobalWorkspace,
        tick_number: u64,
    ) -> PhaseResult {
        // Real-sensor path: poll the environment and publish what actually
        // happened. The published topics are reported verbatim — the runtime
        // never sees a wildcard guess about which channels fired.
        if let Some(loop_) = &self.sensors {
            let mut loop_ = loop_.lock().await;
            let topics = loop_.poll_all_with_topics(workspace).await;
            if topics.is_empty() {
                return PhaseResult::Ok;
            }
            return PhaseResult::Published { topics };
        }

        let mut rng: rand::rngs::StdRng = if let Some(seed) = self.seed {
            rand::rngs::StdRng::seed_from_u64(seed.wrapping_add(tick_number))
        } else {
            rand::rngs::StdRng::from_entropy()
        };

        let event = match rng.gen_range(0..4) {
            0 => SensorEvent::Mouse {
                x: rng.gen_range(0..1920),
                y: rng.gen_range(0..1080),
                button: match rng.gen_range(0..3) {
                    0 => "left".into(),
                    1 => "right".into(),
                    _ => "none".into(),
                },
            },
            1 => SensorEvent::Keyboard {
                key: format!("key_{}", rng.gen_range(0..26)),
                action: if rng.gen_bool(0.5) {
                    "press".into()
                } else {
                    "release".into()
                },
            },
            2 => SensorEvent::Screen {
                region: format!("region_{}", rng.gen_range(0..4)),
                change_detected: rng.gen_bool(0.3),
            },
            _ => SensorEvent::Timer {
                source: "system_clock".into(),
                interval_ms: 1000,
            },
        };

        let (topic, topic_str) = match &event {
            SensorEvent::Mouse { .. } => (Topic::Mouse, "sensor.mouse"),
            SensorEvent::Keyboard { .. } => (Topic::Keyboard, "sensor.keyboard"),
            SensorEvent::Screen { .. } => (Topic::Screen, "sensor.screen"),
            SensorEvent::Timer { .. } => (Topic::Custom("sensor.timer".into()), "sensor.timer"),
        };

        workspace.publish_from(
            topic,
            EngineId::Perception,
            serde_json::to_value(&event).unwrap_or_default(),
        );

        tracing::debug!(
            tick = tick_number,
            event = ?event,
            "perception emitted sensor event"
        );

        PhaseResult::Published {
            topics: vec![topic_str.into()],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn perception_publishes_on_observe_phase() {
        let engine = PerceptionEngine::with_seed(42);
        let workspace = GlobalWorkspace::new();

        let result = engine.execute(CognitivePhase::Observe, &workspace, 1).await;

        match result {
            PhaseResult::Published { topics } => {
                assert!(!topics.is_empty());
                assert!(topics[0].starts_with("sensor."));
            }
            other => panic!("Expected Published, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn perception_emits_various_sensor_types() {
        let mut seen_topics = std::collections::HashSet::new();

        // Run many ticks with different seeds to get diverse events.
        for seed in 0..100 {
            let engine = PerceptionEngine::with_seed(seed);
            let workspace = GlobalWorkspace::new();

            if let PhaseResult::Published { topics } = engine
                .execute(CognitivePhase::Observe, &workspace, seed)
                .await
            {
                for t in topics {
                    seen_topics.insert(t);
                }
            }
        }

        // With 100 different seeds we should see at least 2 distinct sensor topics.
        assert!(
            seen_topics.len() >= 2,
            "Expected diverse sensor types, got: {:?}",
            seen_topics
        );
    }

    #[tokio::test]
    async fn perception_only_runs_on_observe() {
        let engine = PerceptionEngine::new();

        assert_eq!(engine.phases(), vec![CognitivePhase::Observe]);
    }

    #[tokio::test]
    async fn perception_with_real_sensors_publishes_real_events() {
        use friday_cognitive::workspace::Topic;
        use friday_cognitive::WorkspaceEvent;
        use friday_sensors::{MockSensor, SensorLoop};

        let screen_events = vec![WorkspaceEvent::new(
            Topic::Screen,
            serde_json::json!({ "change": "content" }),
        )];
        let mouse_events = vec![WorkspaceEvent::new(
            Topic::Mouse,
            serde_json::json!({ "x": 10, "y": 20 }),
        )];
        let mut loop_ = SensorLoop::new();
        loop_.add(Box::new(MockSensor::new("mock_screen", screen_events)));
        loop_.add(Box::new(MockSensor::new("mock_mouse", mouse_events)));
        let engine = PerceptionEngine::with_sensors(loop_);
        assert!(engine.sensors_enabled());

        let workspace = GlobalWorkspace::new();
        let result = engine.execute(CognitivePhase::Observe, &workspace, 1).await;

        match result {
            PhaseResult::Published { topics } => {
                assert_eq!(topics, vec!["sensor.screen", "sensor.mouse"]);
            }
            other => panic!("Expected Published, got {:?}", other),
        }

        // The workspace really received the events, not just a report.
        assert_eq!(workspace.history(None).len(), 2);
    }

    #[tokio::test]
    async fn perception_with_empty_sensors_reports_ok() {
        use friday_sensors::SensorLoop;

        let engine = PerceptionEngine::with_sensors(SensorLoop::new());
        let workspace = GlobalWorkspace::new();

        let result = engine.execute(CognitivePhase::Observe, &workspace, 1).await;
        assert!(matches!(result, PhaseResult::Ok));
    }
}
