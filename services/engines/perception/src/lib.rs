use friday_cognitive::handler::{PhaseHandler, PhaseResult};
use friday_cognitive::phase::CognitivePhase;
use friday_cognitive::workspace::{EngineId, GlobalWorkspace, Topic};
use async_trait::async_trait;
use rand::{Rng, SeedableRng};

/// Perception Engine
///
/// Runs during the Observe phase. Publishes synthetic sensor events
/// to the workspace to prove the engine→workspace pipeline works.
///
/// In production this would read from real sensors (mouse, keyboard,
/// screen, audio). For now it simulates random input events.
pub struct PerceptionEngine {
    /// Optional fixed seed for reproducible tests.
    seed: Option<u64>,
}

impl PerceptionEngine {
    /// Create a new PerceptionEngine with random behavior.
    pub fn new() -> Self {
        Self { seed: None }
    }

    /// Create a PerceptionEngine with a fixed seed (for deterministic tests).
    pub fn with_seed(seed: u64) -> Self {
        Self { seed: Some(seed) }
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
            SensorEvent::Screen { .. } => (Topic::Custom("sensor.screen".into()), "sensor.screen"),
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

        let result = engine
            .execute(CognitivePhase::Observe, &workspace, 1)
            .await;

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

            if let PhaseResult::Published { topics } =
                engine.execute(CognitivePhase::Observe, &workspace, seed).await
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
}
