//! 7A orchestrator types — the `AutonomousTick` record and the stage boundaries.
//!
//! These are orchestrator-side concepts. None of them leak into `CognitiveTrace`.

use serde::{Deserialize, Serialize};

/// Why a tick fired. `Scheduled` exists as a variant for the future recurring
/// scheduler (7E); 7A only ever uses `Manual` / `Startup`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TickTrigger {
    Manual,
    Startup,
    Scheduled,
}

/// The observable outcome of a single tick.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TickOutcome {
    /// No actionable goal was found. Still recorded as an episode.
    NoWork,
    Success,
    Failure,
}

/// What the tick decided to do next. 7A always returns `Stop`; the recurring
/// scheduler (7E) is explicitly a later phase and is intentionally not wired here.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum NextAction {
    Stop,
    /// Deferred scheduling hint for a future scheduler; 7A ignores it.
    Defer(u64),
}

/// The snapshot an `ObservationSource` returns. Typed, not a free-form blob, so
/// goal selection stays a real decision rather than string-matching JSON.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Observation {
    pub summary: String,
    pub signals: Vec<String>,
}

/// A plan produced by a `Planner`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Plan {
    pub goal: String,
    pub steps: Vec<String>,
}

/// The result of executing a plan.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExecutionResult {
    pub success: bool,
    pub detail: String,
}

/// The orchestrator's record of one bounded tick.
///
/// This is the tick's own artifact. It references the episode it created via
/// `trace_id` but contains no `CognitiveTrace` semantics — the trace layer stays
/// frozen and sovereign.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CognitiveTick {
    pub tick_id: u64,
    pub trigger: TickTrigger,
    pub input_state: Observation,
    pub selected_goal: Option<String>,
    /// The single root cognitive episode this tick produced (one trace_id per tick).
    pub trace_id: friday_core::trace::TraceId,
    pub outcome: TickOutcome,
    pub next_action: NextAction,
}
