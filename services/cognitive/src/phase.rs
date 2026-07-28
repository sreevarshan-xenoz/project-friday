use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::time::Duration;
use uuid::Uuid;

/// The seven phases of the cognitive loop.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum CognitivePhase {
    Observe,
    Understand,
    Plan,
    Act,
    Verify,
    Reflect,
    Learn,
}

impl CognitivePhase {
    /// All phases in cycle order.
    pub fn all() -> &'static [CognitivePhase] {
        &[
            CognitivePhase::Observe,
            CognitivePhase::Understand,
            CognitivePhase::Plan,
            CognitivePhase::Act,
            CognitivePhase::Verify,
            CognitivePhase::Reflect,
            CognitivePhase::Learn,
        ]
    }

    /// Phase name as a string.
    pub fn as_str(&self) -> &'static str {
        match self {
            CognitivePhase::Observe => "observe",
            CognitivePhase::Understand => "understand",
            CognitivePhase::Plan => "plan",
            CognitivePhase::Act => "act",
            CognitivePhase::Verify => "verify",
            CognitivePhase::Reflect => "reflect",
            CognitivePhase::Learn => "learn",
        }
    }
}

impl std::fmt::Display for CognitivePhase {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// A unique identifier for a single cognitive tick.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TickId(pub Uuid);

impl TickId {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

impl Default for TickId {
    fn default() -> Self {
        Self::new()
    }
}

/// A single heartbeat of the cognitive loop.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CognitiveTick {
    pub id: TickId,
    pub started_at: DateTime<Utc>,
    pub phase: CognitivePhase,
    pub phase_started_at: DateTime<Utc>,
    pub tick_number: u64,
}

impl CognitiveTick {
    /// Create a new tick for the given phase.
    pub fn new(phase: CognitivePhase, tick_number: u64) -> Self {
        let now = Utc::now();
        Self {
            id: TickId::new(),
            started_at: now,
            phase,
            phase_started_at: now,
            tick_number,
        }
    }

    /// How long since this phase started.
    pub fn elapsed(&self) -> Duration {
        Utc::now()
            .signed_duration_since(self.phase_started_at)
            .to_std()
            .unwrap_or_default()
    }
}
