use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Relative importance of an event, used by subscribers to prioritise work.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum Priority {
    Low,
    #[default]
    Normal,
    High,
    Critical,
}

/// A single unit of communication on the FRIDAY event bus.
///
/// Events are fire-and-forget and broadcast to every subscriber. The `kind`
/// field is a dot-namespaced topic (e.g. `voice.transcript`,
/// `plan.task.created`) that subscribers filter on. `payload` is opaque JSON so
/// the bus stays language-agnostic and plugins can evolve independently.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Event {
    pub id: Uuid,
    pub kind: String,
    pub source: String,
    pub payload: serde_json::Value,
    pub timestamp: DateTime<Utc>,
    /// Correlation id shared across every event spawned while handling one
    /// user goal, so the full pipeline can be traced end to end.
    pub trace_id: Uuid,
    pub priority: Priority,
    /// Number of times this event has been re-published by the router. Guards
    /// against accidental feedback loops between plugins.
    pub hops: u8,
}

impl Event {
    pub fn new(kind: impl Into<String>, source: impl Into<String>, payload: serde_json::Value) -> Self {
        Self {
            id: Uuid::new_v4(),
            kind: kind.into(),
            source: source.into(),
            payload,
            timestamp: Utc::now(),
            trace_id: Uuid::new_v4(),
            priority: Priority::Normal,
            hops: 0,
        }
    }

    /// Force a specific trace id (e.g. to continue an existing pipeline).
    pub fn with_trace(mut self, trace_id: Uuid) -> Self {
        self.trace_id = trace_id;
        self
    }

    pub fn with_priority(mut self, priority: Priority) -> Self {
        self.priority = priority;
        self
    }

    /// Convenience for JSON object payloads.
    pub fn json(kind: impl Into<String>, source: impl Into<String>, value: serde_json::Value) -> Self {
        Self::new(kind, source, value)
    }
}
