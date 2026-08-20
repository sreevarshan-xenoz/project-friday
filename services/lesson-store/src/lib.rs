//! 8B production lesson store.
//!
//! A `Lesson` is a *prediction-error* generalization (7C semantics): "under
//! condition S, action X repeatedly produces error". This is deliberately distinct
//! from `friday-knowledge`'s behavioral usage-patterns. The crate is a pure data
//! layer: the durable backend reuses FRIDAY's embedded store standard (`redb`,
//! enabled in the durable impl), and the in-memory backend is the hermetic default.
//!
//! Design invariants (8B):
//! * `find_similar` is strictly **read-only** — no pruning, no counters.
//! * `prune_stale` is an explicit maintenance operation.
//! * Contradiction *policy* (when two lessons conflict) lives in the autonomy
//!   orchestrator; this crate only records the status deterministically.
//!
//! This keeps the autonomy `MemoryStore` seam intact while the production
//! implementation may use two specialized backends (raw episodes in
//! `friday-memory`, lessons here).

pub mod error;
pub mod resolver;
pub mod store;
pub mod types;

pub use error::LessonStoreError;
pub use resolver::{CapabilityResolver, IdentityResolver, RegistryCapabilityResolver};
pub use store::{InMemoryLessonStore, LessonStore};
pub use types::{
    LessonKey, LessonProvenance, LessonStatus, MatchPolicy, PruneConfig, ProductionLesson,
};
