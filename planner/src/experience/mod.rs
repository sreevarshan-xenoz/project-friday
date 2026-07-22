//! Experience Layer: read-mostly architecture for execution history.
//!
//! This module implements the five invariants from ADR-0008:
//!
//! 1. **Adapters annotate, kernel emits** — inherited from M9.3/M10.2.
//! 2. **Experience never changes execution** — observe only, never mutate.
//! 3. **Timeline is immutable** — append-only, no updates, no deletes.
//! 4. **Experience stores semantics** — `filesystem.write`, not "mouse click."
//! 5. **Reflection never edits history** — produces records, not mutations.
//!
//! The Timeline is the canonical source of truth. Recorder, Reflection,
//! Indexer, and Service are pure derivations over the timeline.

pub mod timeline;
pub mod recorder;
pub mod reflection;
pub mod indexer;
pub mod service;
pub mod miner;
pub mod suggestion;
pub mod replay;

pub use timeline::TimelineStore;
pub use indexer::ExperienceIndexer;
pub use service::ExperienceService;
pub use miner::{PatternMiner, Pattern};
pub use suggestion::{SuggestionEngine, SuggestionPolicy, WorkflowSuggestion};
pub use replay::{ApplicabilityChecker, ApplicabilityResult, ReplayEngine, WorkflowExecutionReport};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use std::collections::HashMap;

// ---------------------------------------------------------------------------
// Timeline types
// ---------------------------------------------------------------------------

/// Schema version for forward-compatible evolution. Increment when the
/// `TimelineEntry` format changes; old entries keep their version.
pub const TIMELINE_VERSION: u32 = 1;

/// One immutable fact in the execution timeline. Append-only — once created,
/// never modified or deleted.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimelineEntry {
    pub version: u32,
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
    /// Free-form tags for future enrichment (e.g. `["readme", "docs"]`).
    /// Empty by default; the indexer may populate these.
    #[serde(default)]
    pub tags: Vec<String>,
    /// Opaque annotations for future components. Keys are component names,
    /// values are arbitrary JSON. Empty by default.
    #[serde(default)]
    pub annotations: HashMap<String, serde_json::Value>,
}

// ---------------------------------------------------------------------------
// Workflow types
// ---------------------------------------------------------------------------

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

// ---------------------------------------------------------------------------
// Reflection types
// ---------------------------------------------------------------------------

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

// ---------------------------------------------------------------------------
// Retrieval types (M12.1)
// ---------------------------------------------------------------------------

/// Outcome of a past goal attempt.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum GoalOutcome {
    Success,
    Failure,
    PartiallyCompleted,
    Cancelled,
    Unknown,
}

/// Why an experience matched the current query.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum MatchReason {
    /// Same goal title or intent.
    SameGoal,
    /// Similar goal title (fuzzy match).
    SimilarGoal,
    /// Same workflow was used.
    SameWorkflow,
    /// Overlapping capability sequence.
    SimilarCapabilities,
    /// Most recent attempt at this goal.
    RecentAttempt,
}

/// A structured match from the experience layer. The planner receives these
/// without knowing how retrieval worked.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExperienceMatch {
    /// How alike is this to the current query? (0.0–1.0)
    pub similarity_score: f64,
    /// How much should the planner trust this? (0.0–1.0)
    pub trust_score: f64,
    /// Decomposed trust factors (for tuning; planner may ignore).
    pub trust_components: TrustComponents,
    /// Why this matched.
    pub match_reason: MatchReason,
    /// The goal this experience relates to.
    pub goal_id: Uuid,
    /// If a workflow was used, which one.
    pub workflow_id: Option<Uuid>,
    /// What happened.
    pub outcome: GoalOutcome,
    /// Reflections from this execution.
    pub reflection: Vec<ReflectionRecord>,
    /// Artifact paths produced.
    pub artifacts: Vec<String>,
    /// If a git commit was made, which hash.
    pub commit_hash: Option<String>,
    /// The goal title/intent for display.
    pub goal_title: String,
}

/// Decomposed trust factors. The planner receives `trust_score` (a single
/// value), but these components are available for tuning and debugging.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrustComponents {
    /// How similar was the goal? (from semantic index)
    pub similarity: f64,
    /// How recent was the execution? (0.0–1.0)
    pub recency: f64,
    /// How successful was the outcome? (0.0–1.0)
    pub outcome_quality: f64,
    /// How many times has this pattern been observed? (0.0–1.0, normalized)
    pub repetition: f64,
}

impl TrustComponents {
    /// Aggregate components into a single trust score.
    /// Weights: similarity 30%, outcome 40%, recency 20%, repetition 10%.
    pub fn aggregated(&self) -> f64 {
        let score = self.similarity * 0.3
            + self.outcome_quality * 0.4
            + self.recency * 0.2
            + self.repetition * 0.1;
        score.clamp(0.0, 1.0)
    }
}
