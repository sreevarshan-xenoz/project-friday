//! # FRIDAY Core Engine
//!
//! The minimal, language-agnostic heart of the FRIDAY desktop intelligence
//! platform. It provides:
//!
//! * an async [`event`] bus for fire-and-forget communication,
//! * a [`plugin`] SDK so every capability is a swappable module,
//! * a layered [`memory`] system (working → session → long-term → graph → vector),
//! * a [`protocol`] for correlated request/response between modules,
//! * and the [`runtime`] that wires them together.
//!
//! The core contains no intelligence of its own. All behaviour lives in
//! plugins. See `docs/Architecture.md` for the full design.

pub mod error;
pub mod event;
pub mod memory;
pub mod plugin;
pub mod protocol;
pub mod runtime;
pub mod services;
pub mod telemetry;

pub use error::{FridayError, Result};
pub use event::{Event, EventBus, Priority, MAX_HOPS};
pub use memory::{cosine_similarity, LongTermMemory, SessionMemory, VectorMemory, WorkingMemory};
pub use memory::{ExecutionExperience, ExperienceEngine, ExperienceTask};
pub use memory::{MemoryEngine, MemoryEntry, MemoryLayer, MemoryLayerKind};
pub use plugin::{Plugin, PluginContext, PluginManager, PluginMetadata, PluginMetadataBuilder};
pub use protocol::{ModuleId, ProtocolMessage, Transport};
pub use runtime::Friday;
pub use services::Service;
pub use telemetry::init as init_telemetry;
