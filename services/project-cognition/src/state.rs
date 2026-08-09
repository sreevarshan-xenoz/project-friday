//! Persistent project cognition data — pure, serde-only, no logic.
//!
//! The model follows the repo's top invariant: **history is immutable,
//! intelligence is derived.** Every change appends a [`ProjectEvent`] to the
//! journal; the [`ProjectState`] snapshot and the [`MilestoneTransition`]
//! read model are projections of that journal, never independent sources of
//! truth.

use serde::{Deserialize, Serialize};

/// Schema version of the persisted project file. A file with a different
/// schema is rejected (warn + start fresh); a migration path is a later
/// concern and belongs in a separate slice.
pub const SCHEMA_VERSION: u32 = 1;

/// The current snapshot of the project's long-term state — FRIDAY's answer
/// to "where are we, and what are we doing?"
///
/// This is the shape the user sketched for long-term goal memory:
///
/// ```text
/// Primary Goal    →  what we are ultimately building
/// Current Milestone → the milestone being worked right now
/// Progress        →  0.0..=100.0 through the current milestone
/// Blocked By      →  unresolved blockers
/// Open Decisions  →  derived from the decision log (see `decisions`)
/// ```
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProjectState {
    pub project_id: String,
    pub name: String,
    pub primary_goal: String,
    pub current_milestone: String,
    pub next_milestone: Option<String>,
    /// 0.0..=100.0 — clamped on write and on load.
    pub progress_pct: f32,
    pub milestone_started_at_ms: u64,
    /// Currently unresolved blockers (deduplicated).
    pub blocked_by: Vec<String>,
    pub updated_at_ms: u64,
}

impl ProjectState {
    pub fn blank() -> Self {
        Self {
            project_id: String::new(),
            name: String::new(),
            primary_goal: String::new(),
            current_milestone: String::new(),
            next_milestone: None,
            progress_pct: 0.0,
            milestone_started_at_ms: 0,
            blocked_by: Vec::new(),
            updated_at_ms: 0,
        }
    }

    pub fn new(
        project_id: impl Into<String>,
        name: impl Into<String>,
        primary_goal: impl Into<String>,
        now_ms: u64,
    ) -> Self {
        let mut s = Self::blank();
        s.project_id = project_id.into();
        s.name = name.into();
        s.primary_goal = primary_goal.into();
        s.milestone_started_at_ms = now_ms;
        s.updated_at_ms = now_ms;
        s
    }
}

/// One milestone transition — the derived read model of the journal.
///
/// ```text
/// Skill Engine ──→ Agent Runtime ──→ Web Agent ──→ …
/// ```
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MilestoneTransition {
    /// `None` for the first milestone (there was no previous one).
    pub from: Option<String>,
    pub to: String,
    pub reason: String,
    pub at_ms: u64,
}

/// The decision log. Open decisions are the projection
/// `closed_at_ms == None`; a closed decision carries its resolution.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DecisionRecord {
    pub text: String,
    pub opened_at_ms: u64,
    pub closed_at_ms: Option<u64>,
    pub resolution: Option<String>,
}

impl DecisionRecord {
    pub fn is_open(&self) -> bool {
        self.closed_at_ms.is_none()
    }
}

/// An immutable journal entry — the append-only history of the project
/// narrative. The reflection digest is a rendering of these entries since a
/// watermark; the state snapshot is their projection.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ProjectEvent {
    PrimaryGoalSet {
        goal: String,
        at_ms: u64,
    },
    MilestoneAdvanced {
        from: Option<String>,
        to: String,
        reason: String,
        at_ms: u64,
    },
    /// Progress within `milestone` (the milestone current at the time of the
    /// update) — context for the reflection digest.
    ProgressUpdated {
        pct: f32,
        milestone: String,
        at_ms: u64,
    },
    BlockerAdded {
        text: String,
        at_ms: u64,
    },
    BlockerResolved {
        text: String,
        at_ms: u64,
    },
    DecisionOpened {
        text: String,
        at_ms: u64,
    },
    DecisionClosed {
        text: String,
        resolution: String,
        at_ms: u64,
    },
}

impl ProjectEvent {
    pub fn at_ms(&self) -> u64 {
        match self {
            Self::PrimaryGoalSet { at_ms, .. }
            | Self::ProgressUpdated { at_ms, .. }
            | Self::BlockerAdded { at_ms, .. }
            | Self::BlockerResolved { at_ms, .. }
            | Self::DecisionOpened { at_ms, .. } => *at_ms,
            Self::MilestoneAdvanced { at_ms, .. } | Self::DecisionClosed { at_ms, .. } => *at_ms,
        }
    }

    /// Render the event as one digest fact — deterministic prose, never a
    /// model decision.
    pub fn fact(&self) -> String {
        match self {
            Self::PrimaryGoalSet { goal, .. } => format!("Primary goal set: {goal}"),
            Self::MilestoneAdvanced {
                from, to, reason, ..
            } => match from {
                Some(f) => format!("Milestone advanced: {f} → {to} ({reason})"),
                None => format!("Milestone started: {to} ({reason})"),
            },
            Self::ProgressUpdated { pct, milestone, .. } => {
                if milestone.is_empty() {
                    format!("Progress: {pct:.0}%")
                } else {
                    format!("Progress ({milestone}): {pct:.0}%")
                }
            }
            Self::BlockerAdded { text, .. } => format!("Blocker added: {text}"),
            Self::BlockerResolved { text, .. } => format!("Blocker resolved: {text}"),
            Self::DecisionOpened { text, .. } => format!("Decision opened: {text}"),
            Self::DecisionClosed {
                text, resolution, ..
            } => format!("Decision closed: {text} ({resolution})"),
        }
    }
}

/// The persisted artifact: schema version, the state snapshot, the immutable
/// journal, and the digest watermark (journal entries at or above it have
/// already been reflected on).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectFile {
    pub schema: u32,
    pub state: ProjectState,
    pub journal: Vec<ProjectEvent>,
    pub decisions: Vec<DecisionRecord>,
    /// Index into `journal` — entries below it are digested. An *index*, not
    /// a timestamp: exact even when many mutations and a digest share one
    /// wall-clock millisecond.
    pub digest_watermark: usize,
}

impl ProjectFile {
    pub fn blank() -> Self {
        Self {
            schema: SCHEMA_VERSION,
            state: ProjectState::blank(),
            journal: Vec::new(),
            decisions: Vec::new(),
            digest_watermark: 0,
        }
    }
}
