//! Skill lifecycle events (spec `specs/skill-engine.md` §11).
//!
//! Emitted by the [`SkillLoader`](crate::SkillLoader) (scan/load/remove) and the
//! [`SkillRegistry`](crate::SkillRegistry) (enable/disable). The planner and
//! observability layers subscribe to these instead of polling the registry.

use crate::registry::SkillSource;

/// The complete skill lifecycle event vocabulary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SkillEvent {
    /// `skill.scanned` — a source directory was scanned.
    Scanned { source: SkillSource, count: usize },
    /// `skill.loaded` — a valid skill was registered.
    Loaded { skill_id: String, version: String },
    /// `skill.enabled` — a skill became active.
    Enabled { skill_id: String },
    /// `skill.disabled` — a skill was turned off.
    Disabled { skill_id: String },
    /// `skill.removed` — a skill was deregistered.
    Removed { skill_id: String },
    /// `skill.error` — a manifest failed to load.
    Error { skill_id: String, error: String },
    /// `skill.activated` — the context builder chose a skill for a goal.
    Activated { skill_id: String, goal: String },
    /// `skill.deactivated` — a skill is no longer the active skill.
    Deactivated { skill_id: String },
}

/// Consumer of [`SkillEvent`]s. The registry and loader hold one sink each.
pub trait SkillEventSink: Send + Sync {
    fn emit(&self, event: SkillEvent);
}

/// In-memory sink for tests and in-process subscribers.
pub struct VecSink {
    pub events: std::sync::Mutex<Vec<SkillEvent>>,
}

impl VecSink {
    pub fn new() -> Self {
        Self {
            events: std::sync::Mutex::new(Vec::new()),
        }
    }
}

impl Default for VecSink {
    fn default() -> Self {
        Self::new()
    }
}

impl SkillEventSink for VecSink {
    fn emit(&self, event: SkillEvent) {
        self.events.lock().expect("event sink poisoned").push(event);
    }
}

/// Sink that mirrors lifecycle events into the tracing log.
pub struct LoggingSink;

impl SkillEventSink for LoggingSink {
    fn emit(&self, event: SkillEvent) {
        match &event {
            SkillEvent::Error { skill_id, error } => {
                tracing::warn!(skill_id, error, "skill.error");
            }
            _ => tracing::info!(?event, "skill lifecycle"),
        }
    }
}
