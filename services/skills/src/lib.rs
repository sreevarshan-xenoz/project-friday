//! FRIDAY Skill Engine (M5) — the layer that makes abilities installable.
//!
//! A **Skill** is a named, versioned, pure-data declaration of:
//! * capabilities it needs from the Capability Registry,
//! * natural-language actions it can perform (for goal matching),
//! * permissions it requires (gated by user approval),
//! * dependencies on other skills.
//!
//! A Skill contains **no prompts, no instructions, no personality, and no
//! execution logic**. It declares *what* can be done and *how* to plan it; the
//! existing services do the work. This keeps skills portable across LLM
//! providers and testable without a model.
//!
//! This crate is deliberately decoupled from `friday-capabilities` /
//! `friday-planner`: it depends on them only through the abstract
//! [`CapabilityResolver`] / [`RiskResolver`] traits, so the engine is testable
//! in isolation. Wiring to the real registries happens at integration points
//! (e.g. `friday-context`).
//!
//! ## Pipeline
//!
//! ```text
//! scan dirs
//!   → parse skill.json
//!   → validate (id/version/actions/platform/core)
//!   → check dependencies (topological order, cycle detection)
//!   → resolve capabilities (resolved vs. soft-declared)
//!   → register into SkillRegistry
//!   → emit lifecycle events
//! ```

pub mod context;
pub mod error;
pub mod events;
pub mod health;
pub mod loader;
pub mod manifest;
pub mod memory;
pub mod permission;
pub mod qualification;
pub mod registry;

pub use context::{SkillContext, SkillContextSource, SkillSummary};
pub use error::SkillError;
pub use events::{SkillEvent, SkillEventSink, VecSink};
pub use health::{AllHealthyHealthSource, CapHealth, CapabilityHealthSource, SkillHealth};
pub use loader::{
    CapabilityResolver, LoadFailure, LoadReport, SkillLoader, StaticCapabilityResolver,
};
pub use manifest::{CoreVersion, SkillManifest, SkillMemorySpec, FRIDAY_CORE_VERSION};
pub use memory::{MemoryError, MemoryHit, SkillMemory, SkillMemorySource};
pub use permission::{
    AllSafeRiskResolver, PermissionState, RiskResolver, SkillApprovals, SkillPolicy,
    StaticRiskResolver,
};
pub use qualification::{qualify, CheckResult, CheckStatus, QualificationReport};
pub use registry::{
    CapabilityStatus, RankedSkill, SelectedSkill, Skill, SkillRegistry, SkillSource,
};
