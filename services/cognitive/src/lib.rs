//! FRIDAY Cognitive Runtime — the heartbeat.
//!
//! This crate is the smallest possible cognitive loop. It proves the
//! Global Workspace pattern works without any AI, planner, memory, or tools.
//!
//! **Invariant:** This crate must compile and run with every other engine disabled.
//!
//! # Architecture
//!
//! ```text
//! CognitiveRuntime
//!     │
//!     ├── PhaseRegistry (who participates in each phase)
//!     │
//!     ├── GlobalWorkspace (shared state, event bus)
//!     │
//!     └── Scheduler (runs Observe → Understand → Plan → Act → Verify → Reflect → Learn)
//! ```
//!
//! # Usage
//!
//! ```rust,no_run
//! use friday_cognitive::*;
//! use std::sync::Arc;
//!
//! # async fn example() {
//! let workspace = GlobalWorkspace::new();
//! let mut registry = PhaseRegistry::new();
//! let config = RuntimeConfig::default();
//! let mut runtime = CognitiveRuntime::new(config, registry, workspace);
//!
//! let (tick_id, duration) = runtime.tick(1).await;
//! # }
//! ```

pub mod autonomy;
pub mod handler;
pub mod journal;
pub mod phase;
pub mod recorder;
pub mod registry;
pub mod runtime;
pub mod workspace;

pub use autonomy::{
    ActionExecution, ActionExecutor, ActionOutcome, ActionVerifier, AutonomousTick,
    DecisionFunction, DecisionState, Salience, SalienceDecision, TickAction,
};
pub use handler::{FnHandler, PhaseHandler, PhaseResult};
pub use journal::{
    ActionJournal, ActionJournalRecord, BeginAction, JournalState, JsonActionJournal,
};
pub use phase::{CognitivePhase, CognitiveTick, TickId};
pub use recorder::EventRecorder;
pub use registry::PhaseRegistry;
pub use runtime::{CognitiveRuntime, RuntimeConfig, RuntimeMetrics};
pub use workspace::{
    AutonomyEvent, AutonomyReadModel, EngineId, GlobalWorkspace, Priority, Topic, WorkspaceEvent,
};
