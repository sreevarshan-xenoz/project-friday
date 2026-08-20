//! FRIDAY 7A — Single bounded autonomous cognitive tick.
//!
//! An autonomous tick is an **orchestrator concept above the trace layer**. It
//! *creates* `CognitiveTrace` records; it never becomes one and never extends the
//! frozen contract (see docs/architecture/cognitive-trace-v1.md, locked at 5F).
//!
//! Hard boundary for 7A:
//!
//! ```text
//! AutonomousTick
//!       │
//!       └── creates CognitiveTrace
//! ```
//!
//! Allowed: inspect state, select a bounded goal, plan, execute already-permitted
//! capabilities, verify, record outcome, STOP.
//!
//! Not yet (these are later phases / would make the tick unsafe):
//!   * infinite self-scheduling
//!   * unrestricted filesystem mutation
//!   * arbitrary network actions
//!   * permanent self-modification
//!   * auto-approving dangerous permissions
//!   * recursively spawning cognitive ticks
//!
//! The orchestrator writes to a single `TraceCollector`. That same collector
//! fans out to OTel (6A/6B), `CognitiveMetrics` (6C) and the viewer (6D) — one
//! semantic source, as in the observability design.

pub mod orchestrator;
pub mod tick;
pub mod world;

pub use orchestrator::{AutonomousTick, Executor, GoalSelector, ObservationSource, Planner, Verifier};
pub use tick::{
    CognitiveTick, ExecutionResult, NextAction, Observation, Plan, TickOutcome, TickTrigger,
};
