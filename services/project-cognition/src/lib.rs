//! FRIDAY Persistent Project Cognition — long-term goal memory.
//!
//! The piece of the cognitive OS that nothing else owned: the **project
//! narrative**. goal-execution remembers episodes, the reflection engine
//! derives insights from outcomes, and memory stores beliefs — but nothing
//! remembered *where the project is going*: the primary goal, the current
//! milestone, what is blocking it, and which decisions are still open.
//!
//! ```text
//! Goal → Milestone → Progress → Blockers → Decisions
//!              │                      │
//!              └────  journal (immutable) ────┘
//!                         │        │
//!              status_briefing()  reflection_digest()
//! ```
//!
//! Design rules (mirroring the persona/skills "pure data" rule):
//!
//! 1. **History is immutable, intelligence is derived.** Every change appends
//!    a [`ProjectEvent`] to the journal; the [`ProjectState`] snapshot and
//!    the [`MilestoneTransition`] read model are projections, never
//!    independent sources of truth.
//! 2. **No LLM inside.** The engine renders deterministic prose; a model may
//!    *write* events through the API, never decide them.
//! 3. **One owner.** No other crate mutates the project narrative.
//!
//! ```
//! use friday_project_cognition::ProjectCognition;
//!
//! let mut pc = ProjectCognition::new("friday", "FRIDAY", "Build an adaptive cognitive OS");
//! pc.advance_milestone("Agent Runtime", "M8 — planner spawns scoped workers");
//! pc.set_progress(82.0);
//! let briefing = pc.status_briefing();
//! assert!(briefing.contains("82% through the Agent Runtime milestone"));
//! ```

pub mod engine;
pub mod state;

pub use engine::{default_state_path, ProjectCognition};
pub use state::{
    DecisionRecord, MilestoneTransition, ProjectEvent, ProjectFile, ProjectState, SCHEMA_VERSION,
};
