use crate::world::WorldState;
use friday_cognitive::workspace::Topic;
use friday_cognitive::WorkspaceEvent;
use friday_sensors::SensorLoop;

// ── S6: the laptop Observe seam ─────────────────────────────────────

/// The thin observation adapter (S6): owns the existing [`SensorLoop`] and
/// polls it on the Executive's Observe phase.
///
/// This is **not** a second perception engine — it reuses the exact sensors,
/// the exact [`SensorLoop`], and the exact `WorkspaceEvent` vocabulary the
/// cognitive runtime already has. The only new thing is *where* the events
/// land: the laptop's own [`CognitiveBlackboard`](crate::CognitiveBlackboard)
/// `WorldState` instead of the cognitive runtime's `GlobalWorkspace`. The
/// cognitive runtime's perception handler stays where it is; nothing here
/// duplicates it.
///
/// ```text
/// Existing sensors (WindowsScreenSensor / WindowsFocusSensor / …)
///     ↓
/// SensorLoop.poll_all_events()          ← one loop, no publishing
///     ↓
/// Observer                              ← this adapter (thin)
///     ↓
/// fold_observation → existing WorldState ← one world state
///     ↓
/// Executive Observe → attention → action
/// ```
///
/// `None` on the Executive means the exact pre-S6 behavior: no sensor, no
/// observation, no attention item — every existing cycle is untouched.
pub struct Observer {
    sensors: SensorLoop,
}

impl Observer {
    /// Wrap a sensor loop. The loop is reused as-is (existing sensors, the
    /// same `SensorLoop` type) — nothing is rebuilt.
    pub fn new(sensors: SensorLoop) -> Self {
        Self { sensors }
    }

    /// How many sensors this observer polls (audit/debug).
    pub fn sensor_count(&self) -> usize {
        self.sensors.sensor_count()
    }

    /// Poll the attached sensors once. `Vec::new()` means nothing observable
    /// fired this cycle — the sensors deduplicate against their own last
    /// observation, so the adapter never invents events.
    pub async fn poll(&mut self) -> Vec<WorkspaceEvent> {
        self.sensors.poll_all_events().await
    }
}

/// Fold observed sensor events into the blackboard `WorldState` snapshot.
///
/// Returns the updated state plus **one human-readable description per
/// observable change** — the attention item the Observe phase surfaces. Only
/// the senses the laptop observer wires today are folded (screen + window
/// focus); the match is generic, so audio/vision objects slot into the same
/// path later without touching the loop or the Executive.
///
/// The fold is **pure**: the returned state is a copy with the new
/// observations written into `variables` — the same `WorldState` the planner
/// already reasons against, never a parallel world abstraction.
pub fn fold_observation(
    mut state: WorldState,
    events: &[WorkspaceEvent],
) -> (WorldState, Vec<String>) {
    let mut descriptions = Vec::new();
    for event in events {
        let (key, description) = match &event.topic {
            Topic::Screen => (
                "screen",
                format!(
                    "screen: {} — {} ({})",
                    event
                        .payload
                        .get("process")
                        .and_then(|v| v.as_str())
                        .unwrap_or("unknown"),
                    event
                        .payload
                        .get("change")
                        .and_then(|v| v.as_str())
                        .unwrap_or("change"),
                    event
                        .payload
                        .get("title")
                        .and_then(|v| v.as_str())
                        .unwrap_or("untitled"),
                ),
            ),
            Topic::WindowFocusChanged => (
                "active_window",
                format!(
                    "window focus: {}",
                    event
                        .payload
                        .get("title")
                        .and_then(|v| v.as_str())
                        .unwrap_or("unknown")
                ),
            ),
            // Not a sense the laptop observer folds — ignored honestly, never
            // coerced into a fabricated observation.
            _ => continue,
        };
        state
            .variables
            .insert(key.to_string(), event.payload.clone());
        descriptions.push(description);
    }
    (state, descriptions)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Capability, WorldState};
    use friday_sensors::MockSensor;

    #[test]
    fn fold_maps_screen_and_focus_events_and_ignores_unknown_topics() {
        let base = WorldState::new().with_tool(Capability::ReadFile);
        let events = vec![
            WorkspaceEvent::new(
                Topic::Screen,
                serde_json::json!({
                    "title": "Terminal",
                    "pid": 123,
                    "process": "terminal",
                    "change": "content",
                }),
            ),
            WorkspaceEvent::new(
                Topic::WindowFocusChanged,
                serde_json::json!({ "title": "Code", "pid": 456 }),
            ),
            WorkspaceEvent::new(Topic::Mouse, serde_json::json!({ "x": 1, "y": 2 })),
        ];

        let (state, descriptions) = fold_observation(base, &events);

        assert_eq!(state.variables["screen"]["process"], "terminal");
        assert_eq!(state.variables["screen"]["change"], "content");
        assert_eq!(state.variables["active_window"]["title"], "Code");
        assert!(
            state.has_tool(&Capability::ReadFile),
            "the fold preserves the existing world state — tools never drop"
        );
        assert_eq!(descriptions.len(), 2, "unknown topics never surface");
        assert!(descriptions[0].contains("terminal"));
        assert!(descriptions[1].contains("Code"));
    }

    #[test]
    fn fold_with_no_observable_events_is_the_identity() {
        let base = WorldState::new().with_network(true);
        let (state, descriptions) = fold_observation(base.clone(), &[]);
        assert_eq!(state, base, "nothing observed → nothing changes");
        assert!(descriptions.is_empty());
    }

    #[tokio::test]
    async fn observer_polls_the_attached_sensor_loop_verbatim() {
        let mut loop_ = SensorLoop::new();
        loop_.add(Box::new(MockSensor::new(
            "mock_screen",
            vec![WorkspaceEvent::new(
                Topic::Screen,
                serde_json::json!({ "title": "a", "process": "a", "change": "focus" }),
            )],
        )));
        let mut observer = Observer::new(loop_);
        assert_eq!(observer.sensor_count(), 1);

        let events = observer.poll().await;
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].topic, Topic::Screen);
        assert_eq!(events[0].payload["title"], "a");

        // The sensor is exhausted — the adapter reports nothing, never invents.
        assert!(observer.poll().await.is_empty());
    }
}
