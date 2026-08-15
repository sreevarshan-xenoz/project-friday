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
/// focus + spoken turns); the match is generic, so more senses slot into the
/// same path without touching the loop or the Executive.
///
/// A spoken turn is folded **only** when it carries a transcript — that is the
/// user actually saying something FRIDAY can reason about. A bare VAD segment
/// (`transcript: null`, no ASR backend) is ignored honestly, never coerced
/// into a fabricated observation. The fold records *what the user said*; it
/// does not decide what it means — the existing goal pipeline does that.
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
            // S8-A: a spoken turn with a transcript — the user actually said
            // something. The fold records it in the WorldState and surfaces it
            // as attention ("user said: …"), but the utterance is raw text,
            // not a plan: the existing goal pipeline decides what it means. A
            // turn without a transcript (no ASR attached) is a bare VAD
            // segment — ignored honestly, never invented into a goal.
            Topic::TurnDetected => {
                let transcript = event
                    .payload
                    .get("transcript")
                    .and_then(|t| t.as_str())
                    .map(str::trim)
                    .filter(|t| !t.is_empty());
                match transcript {
                    Some(text) => ("spoken_utterance", format!("user said: {text}")),
                    None => continue,
                }
            }
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

    #[test]
    fn fold_folds_transcripted_voice_turn_into_world_and_attention() {
        // S8-A: a spoken turn with a transcript is a real observation — it lands
        // in the WorldState and surfaces as attention ("user said: …"), while
        // staying raw text (no plan is created by perception).
        let base = WorldState::new();
        let events = vec![WorkspaceEvent::new(
            Topic::TurnDetected,
            serde_json::json!({
                "frames": 12,
                "duration_ms": 240,
                "sample_rate": 16_000,
                "asr": "mock",
                "transcript": "open Chrome",
            }),
        )];

        let (state, descriptions) = fold_observation(base, &events);

        assert_eq!(
            state.variables["spoken_utterance"]["transcript"],
            "open Chrome"
        );
        assert_eq!(state.variables["spoken_utterance"]["asr"], "mock");
        assert_eq!(state.variables["spoken_utterance"]["duration_ms"], 240);
        assert_eq!(
            descriptions,
            vec!["user said: open Chrome".to_string()],
            "the attention item says what the user said, nothing more"
        );
    }

    #[test]
    fn fold_ignores_voice_turn_without_transcript_honestly() {
        // A VAD segment with no ASR transcript is not something we can act on —
        // the fold ignores it, never fabricating a spoken observation.
        let base = WorldState::new();
        let events = vec![
            WorkspaceEvent::new(
                Topic::TurnDetected,
                serde_json::json!({
                    "frames": 12,
                    "duration_ms": 240,
                    "sample_rate": 16_000,
                    "asr": serde_json::Value::Null,
                    "transcript": serde_json::Value::Null,
                }),
            ),
            WorkspaceEvent::new(
                Topic::TurnDetected,
                serde_json::json!({
                    "frames": 6,
                    "duration_ms": 120,
                    "sample_rate": 16_000,
                    "asr": "mock",
                    "transcript": "   ",
                }),
            ),
        ];

        let (state, descriptions) = fold_observation(base, &events);

        assert!(
            !state.variables.contains_key("spoken_utterance"),
            "no transcript → no invented spoken observation"
        );
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
