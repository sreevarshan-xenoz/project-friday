//! Experience Layer: read-mostly architecture for execution history.
//!
//! This module implements the four invariants from ADR-0008:
//!
//! 1. **Adapters annotate, kernel emits** — inherited from M9.3/M10.2.
//! 2. **Experience never changes execution** — observe only, never mutate.
//! 3. **Timeline is immutable** — append-only, no updates, no deletes.
//! 4. **Experience stores semantics** — `filesystem.write`, not "mouse click."
//!
//! The Timeline is the canonical source of truth. Recorder and Reflection are
//! pure derivations that read the timeline and produce output.

pub mod timeline;
pub mod recorder;
pub mod reflection;

pub use timeline::TimelineStore;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// One immutable fact in the execution timeline. Append-only — once created,
/// never modified or deleted.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimelineEntry {
    pub id: Uuid,
    pub timestamp: DateTime<Utc>,
    pub goal_id: Uuid,
    pub plan_id: Uuid,
    pub execution_id: Uuid,
    /// The event kind from the bus: `"exec.action"`, `"exec.result"`,
    /// `"artifact.created"`, `"git.commit.created"`, etc.
    pub event_kind: String,
    /// Semantic capability identifier: `"filesystem.write"`, `"git.commit"`,
    /// etc. Never raw UI events.
    pub capability: String,
    /// Event payload (the original event payload, or enriched metadata).
    pub metadata: serde_json::Value,
    /// Links to the preceding entry in the same execution, forming a linked
    /// list. `None` for the first entry of a goal.
    pub parent: Option<Uuid>,
}

/// A successful semantic workflow extracted by the Recorder from a completed
/// goal's timeline entries.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Workflow {
    pub id: Uuid,
    pub name: String,
    /// Semantic action sequence: `["filesystem.write", "git.commit"]`.
    pub actions: Vec<String>,
    /// Full capability strings: `["mcp:filesystem.write_file", "mcp:git.git_commit"]`.
    pub capabilities: Vec<String>,
    pub created_at: DateTime<Utc>,
    pub source_goal_id: Uuid,
}

/// A reflection insight derived from a timeline entry. Reflection never edits
/// the timeline — it produces these records as separate artifacts.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReflectionRecord {
    pub id: Uuid,
    pub timeline_entry_id: Uuid,
    pub insight: String,
    pub category: ReflectionCategory,
    pub timestamp: DateTime<Utc>,
}

/// Categories of reflection insights.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ReflectionCategory {
    /// Action succeeded — useful for workflow recording.
    Success,
    /// Action failed — useful for debugging.
    Failure,
    /// Permission denied — useful for policy tuning.
    PermissionDenied,
    /// Artifact was produced — useful for tracking output.
    ArtifactProduced,
    /// Git operation completed — useful for change tracking.
    GitOperation,
    /// Informational observation.
    Observation,
}
