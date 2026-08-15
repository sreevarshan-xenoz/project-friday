//! FRIDAY's ears on the live host (S8-A): the voice intake.
//!
//! The microphone already has a home — [`AudioSensor`] (WASAPI capture → VAD →
//! turn detection → [`Topic::TurnDetected`]) behind the frozen
//! [`SpeechToText`] seam. What was missing is **intake**: the portal drives the
//! goal loop by publishing `plan.goal`, but nothing on the laptop turned a
//! spoken utterance into one. This adapter closes exactly that gap — the same
//! sensor, the same event vocabulary, the same `plan.goal` intake path the
//! portal already uses. It is **not** a second voice architecture and it does
//! **not** plan: it hands the user's raw words to the existing goal pipeline,
//! which decides what they mean.
//!
//! Honesty rules:
//! - a turn with a non-empty transcript → `plan.goal { goal: <transcript>,
//!   session_id, source: "voice" }`
//! - a turn with no transcript (no ASR backend attached) → nothing. Never a
//!   fabricated goal from a bare VAD segment.
//!
//! `feed_pcm` drives the mic's pure pipeline for tests and offline harnesses;
//! the live host just polls (S8-C wires the daemon to the real capture device).

use std::time::Duration;

use friday_cognitive::workspace::Topic;
use friday_core::event::{Event, EventBus};
use friday_sensors::audio::{AudioSensor, Turn};
use friday_sensors::Sensor;
use serde_json::json;

/// One voice turn → one `plan.goal` on the existing goal intake.
pub struct VoiceIntake {
    bus: EventBus,
    mic: AudioSensor,
    session_id: String,
    /// Turns closed by `process_pcm` (feed) but not yet folded into the goal
    /// intake (poll). Mirrors how the capture path buffers raw samples between
    /// reads: the pure pipeline hands the caller closed turns, so the intake
    /// holds them until the next poll.
    pending_turns: Vec<Turn>,
}

impl VoiceIntake {
    /// Stable session id for the voice channel, so the GoalRunner keeps spoken
    /// turns continuous (per-session context) without clashing with the
    /// portal/phone sessions.
    pub const DEFAULT_SESSION_ID: &'static str = "voice";

    pub fn new(bus: EventBus, mic: AudioSensor) -> Self {
        Self {
            bus,
            mic,
            session_id: Self::DEFAULT_SESSION_ID.into(),
            pending_turns: Vec::new(),
        }
    }

    /// Override the session this intake addresses (default `voice`).
    pub fn with_session_id(mut self, session_id: impl Into<String>) -> Self {
        self.session_id = session_id.into();
        self
    }

    /// Feed synthetic PCM into the mic's pure pipeline (tests and offline
    /// harnesses). Any turn it closes is queued and folded by the next `poll`.
    /// The live host feeds nothing here — it polls the real capture device.
    pub fn feed_pcm(&mut self, samples: &[i16]) -> Vec<Turn> {
        let closed = self.mic.process_pcm(samples);
        let returned = closed.clone();
        self.pending_turns.extend(closed);
        returned
    }

    /// Poll the mic once and publish `plan.goal` for every transcripted turn.
    /// Returns the transcripts that became goals this poll (audit + tests).
    pub async fn poll(&mut self) -> Vec<String> {
        let mut events = self.mic.poll().await;
        let queued = std::mem::take(&mut self.pending_turns);
        events.extend(self.mic.turns_to_events(queued).await);
        let mut admitted = Vec::new();
        for event in &events {
            if event.topic != Topic::TurnDetected {
                continue;
            }
            let transcript = event
                .payload
                .get("transcript")
                .and_then(|t| t.as_str())
                .map(str::trim)
                .filter(|t| !t.is_empty());
            let Some(transcript) = transcript else {
                continue;
            };
            let _ = self
                .bus
                .publish(Event::new(
                    "plan.goal",
                    "voice",
                    json!({
                        "goal": transcript,
                        "session_id": self.session_id,
                        "source": "voice",
                    }),
                ))
                .await;
            admitted.push(transcript.to_string());
        }
        admitted
    }

    /// The live loop (the daemon wires this once S8-C attaches the real mic):
    /// poll on an interval and feed the goal intake.
    pub async fn run(mut self, interval: Duration) {
        loop {
            self.poll().await;
            tokio::time::sleep(interval).await;
        }
    }
}

/// Deterministic "speech-like" energy for hermetic tests: 10 frames of 440 Hz
/// tone + 8 silent frames, matching the sensor's own test pipeline so VAD opens
/// a turn and the turn detector closes it. Shared by the intake tests and the
/// S8-A acceptance test in the goal runner.
#[cfg(test)]
pub(crate) fn test_turn_pcm() -> Vec<i16> {
    let sample_rate = friday_sensors::audio::DEFAULT_SAMPLE_RATE;
    let frame_samples = friday_sensors::audio::FRAME_SAMPLES;
    let mut pcm: Vec<i16> = (0..10 * frame_samples)
        .map(|i| {
            let t = i as f64 / f64::from(sample_rate);
            (0.3 * (2.0 * std::f64::consts::PI * 440.0 * t).sin() * i16::MAX as f64) as i16
        })
        .collect();
    pcm.extend_from_slice(&vec![0i16; 8 * frame_samples]);
    pcm
}

#[cfg(test)]
mod tests {
    use super::*;
    use friday_core::event::EventBus;
    use friday_sensors::audio::{MockTranscriber, SpeechToText};
    use std::sync::Arc;

    #[tokio::test]
    async fn transcripted_turn_publishes_plan_goal() {
        let bus = EventBus::new();
        let mut rx = bus.subscribe();
        let asr: Arc<dyn SpeechToText> = Arc::new(MockTranscriber {
            phrase: "open Chrome".into(),
        });
        let mut intake = VoiceIntake::new(bus, AudioSensor::without_capture().with_asr(asr));

        intake.feed_pcm(&test_turn_pcm());
        let admitted = intake.poll().await;

        assert_eq!(admitted, vec!["open Chrome".to_string()]);
        let ev = rx.recv().await.expect("plan.goal must be published");
        assert_eq!(ev.kind, "plan.goal");
        assert_eq!(ev.payload["goal"], "open Chrome");
        assert_eq!(ev.payload["session_id"], VoiceIntake::DEFAULT_SESSION_ID);
        assert_eq!(ev.payload["source"], "voice");
        assert_eq!(ev.source, "voice", "the intake is the publisher");
    }

    #[tokio::test]
    async fn turn_without_transcript_publishes_nothing() {
        // No ASR backend → the sensor honestly emits `transcript: null`. The
        // intake must not invent a goal from a bare VAD segment.
        let bus = EventBus::new();
        let mut intake = VoiceIntake::new(bus.clone(), AudioSensor::without_capture());

        intake.feed_pcm(&test_turn_pcm());
        let admitted = intake.poll().await;

        assert!(admitted.is_empty(), "no transcript → no goal: {admitted:?}");
        // Give a publish that must not exist a moment to (not) arrive.
        let timeout = tokio::time::timeout(Duration::from_millis(50), bus.subscribe().recv()).await;
        assert!(
            timeout.is_err(),
            "nothing must be published for a null transcript"
        );
    }

    #[tokio::test]
    async fn whitespace_transcript_is_treated_as_silence() {
        let bus = EventBus::new();
        let asr: Arc<dyn SpeechToText> = Arc::new(MockTranscriber {
            phrase: "   ".into(),
        });
        let mut intake =
            VoiceIntake::new(bus.clone(), AudioSensor::without_capture().with_asr(asr));

        intake.feed_pcm(&test_turn_pcm());
        let admitted = intake.poll().await;

        assert!(admitted.is_empty());
        let timeout = tokio::time::timeout(Duration::from_millis(50), bus.subscribe().recv()).await;
        assert!(timeout.is_err(), "whitespace is not a goal");
    }
}
