//! FRIDAY Cognitive Trace — the nervous system's telemetry.
//!
//! This module re-exports the canonical trace schema from `friday-core`
//! and adds cognitive-specific extensions (the `Organ` enum, etc.).
//!
//! **Canonical schema lives in `friday-core::trace`.**
//! This module is the cognitive runtime's view of it.

// Re-export the canonical trace schema from friday-core.
pub use friday_core::trace::*;

use serde::{Deserialize, Serialize};

/// The FRIDAY organ that performed this operation.
///
/// This is a cognitive-specific extension: the core schema uses a plain
/// `String` for the organ field, and this enum provides type safety
/// within the cognitive runtime.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Organ {
    Perception,
    Memory,
    Planner,
    Agents,
    Verification,
    WorldModel,
    Intent,
    Skills,
    Web,
    Documents,
    Audio,
    Vision,
    Screen,
    Runtime,
}

impl std::fmt::Display for Organ {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Organ::Perception => write!(f, "perception"),
            Organ::Memory => write!(f, "memory"),
            Organ::Planner => write!(f, "planner"),
            Organ::Agents => write!(f, "agents"),
            Organ::Verification => write!(f, "verification"),
            Organ::WorldModel => write!(f, "world_model"),
            Organ::Intent => write!(f, "intent"),
            Organ::Skills => write!(f, "skills"),
            Organ::Web => write!(f, "web"),
            Organ::Documents => write!(f, "documents"),
            Organ::Audio => write!(f, "audio"),
            Organ::Vision => write!(f, "vision"),
            Organ::Screen => write!(f, "screen"),
            Organ::Runtime => write!(f, "runtime"),
        }
    }
}

impl Organ {
    /// Return the string representation (for use with the core `CognitiveTrace`).
    pub fn as_str(&self) -> &'static str {
        match self {
            Organ::Perception => "perception",
            Organ::Memory => "memory",
            Organ::Planner => "planner",
            Organ::Agents => "agents",
            Organ::Verification => "verification",
            Organ::WorldModel => "world_model",
            Organ::Intent => "intent",
            Organ::Skills => "skills",
            Organ::Web => "web",
            Organ::Documents => "documents",
            Organ::Audio => "audio",
            Organ::Vision => "vision",
            Organ::Screen => "screen",
            Organ::Runtime => "runtime",
        }
    }
}

/// Helper to create a `CognitiveTrace` with a typed `Organ`.
///
/// This is a convenience wrapper that converts the `Organ` to a string
/// for the core schema's `organ` field.
pub fn trace_with_organ(
    session_id: SessionId,
    goal_id: GoalId,
    span_id: SpanId,
    organ: Organ,
    operation: impl Into<String>,
) -> CognitiveTrace {
    CognitiveTrace::new(session_id, goal_id, span_id, organ.as_str(), operation)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn organ_display() {
        assert_eq!(Organ::Planner.to_string(), "planner");
        assert_eq!(Organ::Agents.to_string(), "agents");
        assert_eq!(Organ::Verification.to_string(), "verification");
    }

    #[test]
    fn organ_as_str() {
        assert_eq!(Organ::Planner.as_str(), "planner");
        assert_eq!(Organ::Runtime.as_str(), "runtime");
    }

    #[test]
    fn trace_with_organ_helper() {
        let trace = trace_with_organ(
            SessionId::default(),
            GoalId::default(),
            SpanId::default(),
            Organ::Planner,
            "plan",
        );
        assert_eq!(trace.organ, "planner");
        assert_eq!(trace.operation, "plan");
    }

    #[test]
    fn cognitive_trace_structure() {
        let trace = CognitiveTrace::new(
            SessionId::default(),
            GoalId::default(),
            SpanId::default(),
            Organ::Planner.as_str(),
            "plan",
        )
        .with_model("openrouter/gpt-4o", "openrouter")
        .with_tokens(100, 20, 0.003)
        .with_result(ResultType::Success)
        .with_verification(VerificationStatus::Passed)
        .with_confidence(0.95);

        assert_eq!(trace.organ, "planner");
        assert_eq!(trace.operation, "plan");
        assert_eq!(trace.model(), Some("openrouter/gpt-4o"));
        assert_eq!(trace.provider(), Some("openrouter"));
        assert_eq!(trace.tokens_in(), Some(100));
        assert_eq!(trace.tokens_out(), Some(20));
        assert_eq!(trace.cost_usd(), Some(0.003));
        assert_eq!(trace.result, ResultType::Success);
        assert!(trace.verification().is_passed());
        assert_eq!(trace.confidence, Some(0.95));
    }
}
