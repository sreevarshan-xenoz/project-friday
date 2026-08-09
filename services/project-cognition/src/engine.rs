//! The [`ProjectCognition`] engine — long-term goal memory.
//!
//! Owns the project narrative and nothing else: it renders deterministic
//! prose ([`ProjectCognition::status_briefing`]), produces the reflection
//! digest ([`ProjectCognition::reflection_digest`]), and persists an atomic,
//! schema-versioned JSON file. No LLM inside — a model may *write* events
//! through the API, never decide them.

use std::fs;
use std::path::{Path, PathBuf};

use crate::state::{
    DecisionRecord, MilestoneTransition, ProjectEvent, ProjectFile, ProjectState, SCHEMA_VERSION,
};
use tracing::{debug, warn};

/// Default persistence location — override with `FRIDAY_PROJECT_STATE_PATH`.
pub fn default_state_path() -> PathBuf {
    std::env::var_os("FRIDAY_PROJECT_STATE_PATH")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(".friday/project-state.json"))
}

fn now_ms() -> u64 {
    chrono::Utc::now().timestamp_millis() as u64
}

/// Long-term goal memory for one project.
///
/// ```text
/// Goal → Milestone → Progress → Blockers → Decisions
///              │                        │
///              └────  journal (immutable) ────┘
///                         │        │
///              status_briefing()  reflection_digest()
/// ```
pub struct ProjectCognition {
    file: ProjectFile,
    path: Option<PathBuf>,
}

impl ProjectCognition {
    /// A fresh, in-memory cognition. Call [`Self::with_path`] before
    /// [`Self::save`] to back it with a file.
    pub fn new(
        project_id: impl Into<String>,
        name: impl Into<String>,
        primary_goal: impl Into<String>,
    ) -> Self {
        let now = now_ms();
        Self {
            file: ProjectFile {
                schema: SCHEMA_VERSION,
                state: ProjectState::new(project_id, name, primary_goal, now),
                journal: Vec::new(),
                decisions: Vec::new(),
                // Nothing digested yet: the very first digest reports every
                // event created before it. Only `reflection_digest()` moves
                // the watermark.
                digest_watermark: 0,
            },
            path: None,
        }
    }

    /// Attach a persistence path (builder).
    pub fn with_path(mut self, path: impl Into<PathBuf>) -> Self {
        self.path = Some(path.into());
        self
    }

    /// Open a persisted project file, or start fresh (with a warning) if it
    /// is missing, unreadable, corrupted, or from a different schema.
    ///
    /// Contract: `open()` is for an *existing* file. For a brand-new project
    /// prefer [`Self::new`] + [`Self::with_path`]; after `open()` of a
    /// missing path, set identity with [`Self::set_name`]/[`Self::set_project_id`]
    /// before the first [`Self::save`].
    pub fn open(path: impl Into<PathBuf>) -> Self {
        let path = path.into();
        let file = match fs::read_to_string(&path) {
            Ok(raw) => match serde_json::from_str::<ProjectFile>(&raw) {
                Ok(mut f) => {
                    sanitize(&mut f, &path);
                    f
                }
                Err(e) => {
                    warn!(path = %path.display(), error = %e, "project state unreadable — starting fresh");
                    ProjectFile::blank()
                }
            },
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                debug!(path = %path.display(), "no project state yet — starting fresh");
                ProjectFile::blank()
            }
            Err(e) => {
                warn!(path = %path.display(), error = %e, "project state unreadable — starting fresh");
                ProjectFile::blank()
            }
        };
        Self {
            file,
            path: Some(path),
        }
    }

    /// Atomically persist to the attached path (temp file + rename, so a
    /// crash never leaves a truncated state file).
    pub fn save(&self) -> Result<(), String> {
        let path = self.path.as_ref().ok_or_else(|| {
            "project cognition has no backing path — use with_path() first".to_string()
        })?;
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                fs::create_dir_all(parent)
                    .map_err(|e| format!("create dir {}: {e}", parent.display()))?;
            }
        }
        let json = serde_json::to_vec_pretty(&self.file).map_err(|e| e.to_string())?;
        // Unique temp name per process: two concurrent saves to the same path
        // can never interleave on the same tmp file.
        let tmp = path.with_extension(format!("json.tmp.{}", std::process::id()));
        fs::write(&tmp, json).map_err(|e| format!("write {}: {e}", tmp.display()))?;
        fs::rename(&tmp, path)
            .map_err(|e| format!("rename {} → {}: {e}", tmp.display(), path.display()))?;
        Ok(())
    }

    // ── State ────────────────────────────────────────────────────────

    pub fn state(&self) -> &ProjectState {
        &self.file.state
    }

    pub fn journal(&self) -> &[ProjectEvent] {
        &self.file.journal
    }

    pub fn decisions(&self) -> &[DecisionRecord] {
        &self.file.decisions
    }

    /// Currently open decisions (the projection `closed_at_ms == None`).
    pub fn open_decisions(&self) -> Vec<&str> {
        self.file
            .decisions
            .iter()
            .filter(|d| d.is_open())
            .map(|d| d.text.as_str())
            .collect()
    }

    /// Milestone history, derived from the journal (oldest first).
    pub fn transitions(&self) -> Vec<MilestoneTransition> {
        self.file
            .journal
            .iter()
            .filter_map(|e| match e {
                ProjectEvent::MilestoneAdvanced {
                    from,
                    to,
                    reason,
                    at_ms,
                } => Some(MilestoneTransition {
                    from: from.clone(),
                    to: to.clone(),
                    reason: reason.clone(),
                    at_ms: *at_ms,
                }),
                _ => None,
            })
            .collect()
    }

    // ── Mutations (each appends an immutable journal entry) ─────────

    pub fn set_primary_goal(&mut self, goal: impl Into<String>) {
        let now = self.next_event_ms();
        let goal = goal.into();
        self.file.state.primary_goal = goal.clone();
        self.touch(now);
        self.file
            .journal
            .push(ProjectEvent::PrimaryGoalSet { goal, at_ms: now });
    }

    /// Set the display name (soft identity field — not journaled).
    pub fn set_name(&mut self, name: impl Into<String>) {
        self.file.state.name = name.into();
    }

    /// Set the project id (soft identity field — not journaled).
    pub fn set_project_id(&mut self, project_id: impl Into<String>) {
        self.file.state.project_id = project_id.into();
    }

    pub fn set_progress(&mut self, pct: f32) {
        let now = self.next_event_ms();
        if !pct.is_finite() {
            warn!(pct, "ignoring non-finite progress update");
            return;
        }
        let pct = pct.clamp(0.0, 100.0);
        let milestone = self.file.state.current_milestone.clone();
        self.file.state.progress_pct = pct;
        self.touch(now);
        self.file.journal.push(ProjectEvent::ProgressUpdated {
            pct,
            milestone,
            at_ms: now,
        });
    }

    /// Advance to the next milestone. Records the transition (with reason)
    /// in the journal and resets the clock on the current milestone. If the
    /// advanced-to milestone was the declared `next_milestone`, it is
    /// consumed.
    pub fn advance_milestone(&mut self, to: impl Into<String>, reason: impl Into<String>) {
        let now = self.next_event_ms();
        let to = to.into();
        let from = if self.file.state.current_milestone.is_empty() {
            None
        } else {
            Some(self.file.state.current_milestone.clone())
        };
        let reason = reason.into();
        self.file.state.current_milestone = to.clone();
        self.file.state.milestone_started_at_ms = now;
        // Progress is per-milestone ("82% through the X milestone") — a new
        // milestone starts at 0. Journaled so the load-time reconcile stays
        // exact.
        self.file.state.progress_pct = 0.0;
        if self.file.state.next_milestone.as_deref() == Some(to.as_str()) {
            self.file.state.next_milestone = None;
        }
        self.touch(now);
        self.file.journal.push(ProjectEvent::MilestoneAdvanced {
            from,
            to: to.clone(),
            reason: reason.clone(),
            at_ms: now,
        });
        self.file.journal.push(ProjectEvent::ProgressUpdated {
            pct: 0.0,
            milestone: to,
            at_ms: now,
        });
    }

    pub fn set_next_milestone(&mut self, next: impl Into<String>) {
        self.file.state.next_milestone = Some(next.into());
    }

    pub fn clear_next_milestone(&mut self) {
        self.file.state.next_milestone = None;
    }

    pub fn add_blocker(&mut self, desc: impl Into<String>) {
        let now = self.next_event_ms();
        let desc = desc.into();
        if self.file.state.blocked_by.contains(&desc) {
            return;
        }
        self.file.state.blocked_by.push(desc.clone());
        self.touch(now);
        self.file.journal.push(ProjectEvent::BlockerAdded {
            text: desc,
            at_ms: now,
        });
    }

    /// Remove a blocker. Returns `false` if it was not present.
    pub fn resolve_blocker(&mut self, desc: &str) -> bool {
        let now = self.next_event_ms();
        if let Some(pos) = self.file.state.blocked_by.iter().position(|b| b == desc) {
            self.file.state.blocked_by.remove(pos);
            self.touch(now);
            self.file.journal.push(ProjectEvent::BlockerResolved {
                text: desc.to_string(),
                at_ms: now,
            });
            true
        } else {
            false
        }
    }

    pub fn open_decision(&mut self, text: impl Into<String>) {
        let now = self.next_event_ms();
        let text = text.into();
        if self
            .file
            .decisions
            .iter()
            .any(|d| d.is_open() && d.text == text)
        {
            return;
        }
        self.file.decisions.push(DecisionRecord {
            text: text.clone(),
            opened_at_ms: now,
            closed_at_ms: None,
            resolution: None,
        });
        self.touch(now);
        self.file
            .journal
            .push(ProjectEvent::DecisionOpened { text, at_ms: now });
    }

    /// Close an open decision with a resolution. Returns `false` if there is
    /// no open decision with that text.
    pub fn close_decision(&mut self, text: &str, resolution: impl Into<String>) -> bool {
        let now = self.next_event_ms();
        let resolution = resolution.into();
        let rec = self
            .file
            .decisions
            .iter_mut()
            .find(|d| d.is_open() && d.text == text);
        match rec {
            Some(r) => {
                r.closed_at_ms = Some(now);
                r.resolution = Some(resolution.clone());
            }
            None => return false,
        }
        self.touch(now);
        self.file.journal.push(ProjectEvent::DecisionClosed {
            text: text.to_string(),
            resolution,
            at_ms: now,
        });
        true
    }

    // ── The "FRIDAY speaks" moment ───────────────────────────────────

    /// The proactive project briefing — the paragraph FRIDAY opens with
    /// before the user says anything:
    ///
    /// > "We're 82% through the Agent Runtime milestone. 1 blocker remains:
    /// > the Desktop Agent. 2 decisions are still open: provider routing,
    /// > persona blending. Next up: Web Agent."
    ///
    /// Deterministic prose derived from the state snapshot; a model renders
    /// it, never decides it.
    pub fn status_briefing(&self) -> String {
        let s = &self.file.state;
        let mut parts: Vec<String> = Vec::new();

        if s.current_milestone.is_empty() {
            parts.push("No active milestone yet.".to_string());
        } else if s.progress_pct >= 100.0 {
            parts.push(format!(
                "The {} milestone is complete.",
                s.current_milestone
            ));
        } else {
            parts.push(format!(
                "We're {:.0}% through the {} milestone.",
                s.progress_pct, s.current_milestone
            ));
        }

        match s.blocked_by.len() {
            0 => {}
            1 => parts.push(format!("1 blocker remains: {}.", s.blocked_by[0])),
            n => parts.push(format!(
                "{} blockers remain: {}.",
                n,
                s.blocked_by.join(", ")
            )),
        }

        let open = self.open_decisions();
        if !open.is_empty() {
            let noun = if open.len() == 1 {
                "decision is".to_string()
            } else {
                "decisions are".to_string()
            };
            parts.push(format!(
                "{} {noun} still open: {}.",
                open.len(),
                open.join(", ")
            ));
        }

        if let Some(next) = &s.next_milestone {
            parts.push(format!("Next up: {}.", next));
        }

        parts.join(" ")
    }

    // ── Reflection digest ────────────────────────────────────────────

    /// Facts since `since_ms` — what changed in the project narrative.
    /// Deterministic; this is the input the nightly reflection pass (or any
    /// scheduled introspection) consumes.
    pub fn facts_since(&self, since_ms: u64) -> Vec<String> {
        self.file
            .journal
            .iter()
            .filter(|e| e.at_ms() > since_ms)
            .map(ProjectEvent::fact)
            .collect()
    }

    /// All facts since the last digest, then advance the watermark so the
    /// next call only reports *new* changes. A fresh cognition whose journal
    /// is empty yields `[]`.
    pub fn reflection_digest(&mut self) -> Vec<String> {
        // Index-based watermark: exact even when many mutations and a digest
        // share one wall-clock millisecond.
        let facts = self.file.journal[self.file.digest_watermark..]
            .iter()
            .map(ProjectEvent::fact)
            .collect::<Vec<_>>();
        self.file.digest_watermark = self.file.journal.len();
        facts
    }

    /// Strictly increasing event timestamp: `max(now, last event + 1)`.
    /// Guarantees exactly-once digest semantics even for multiple mutations
    /// in the same wall-clock millisecond, and survives small clock
    /// regressions within a process.
    fn next_event_ms(&self) -> u64 {
        let now = now_ms();
        let last = self
            .file
            .journal
            .last()
            .map(ProjectEvent::at_ms)
            .unwrap_or(0);
        now.max(last.saturating_add(1))
    }

    fn touch(&mut self, now_ms: u64) {
        self.file.state.updated_at_ms = now_ms;
    }
}

/// Sanitize a loaded file: schema gate, clamp/reject bad values, then
/// reconcile the snapshot against the journal. Corrupt files are *never*
/// trusted — and neither is a snapshot that drifts from its own history.
fn sanitize(file: &mut ProjectFile, path: &Path) {
    if file.schema != SCHEMA_VERSION {
        warn!(path = %path.display(), found = file.schema, expected = SCHEMA_VERSION, "project state schema mismatch — starting fresh");
        *file = ProjectFile::blank();
        return;
    }
    let p = &mut file.state.progress_pct;
    if !p.is_finite() {
        warn!(path = %path.display(), "non-finite progress_pct in project state — reset to 0");
        *p = 0.0;
    } else {
        *p = p.clamp(0.0, 100.0);
    }
    let mut seen = Vec::with_capacity(file.state.blocked_by.len());
    file.state.blocked_by.retain(|b| {
        !b.is_empty() && !seen.contains(b) && {
            seen.push(b.clone());
            true
        }
    });
    reconcile(file, path);
}

/// Re-derive the journal-covered fields of the snapshot (and the decision
/// log) from the immutable journal — **the journal is the single source of
/// truth**. A hand-edited or future-written snapshot that drifts from its
/// own history is corrected, with a warning.
///
/// Soft fields that are intentionally not journaled (`next_milestone`, name,
/// project id) are preserved from the snapshot.
fn reconcile(file: &mut ProjectFile, path: &Path) {
    let before_state = file.state.clone();
    let before_decisions = file.decisions.clone();

    let mut primary_goal: Option<String> = None;
    let mut milestone: Option<String> = None;
    let mut milestone_started_at: Option<u64> = None;
    let mut progress: Option<f32> = None;
    let mut blockers: Vec<String> = Vec::new();
    let mut decisions: Vec<DecisionRecord> = Vec::new();

    for event in &file.journal {
        match event {
            ProjectEvent::PrimaryGoalSet { goal, .. } => primary_goal = Some(goal.clone()),
            ProjectEvent::MilestoneAdvanced { to, at_ms, .. } => {
                milestone = Some(to.clone());
                milestone_started_at = Some(*at_ms);
            }
            ProjectEvent::ProgressUpdated { pct, .. } => progress = Some(*pct),
            ProjectEvent::BlockerAdded { text, .. } => {
                if !blockers.contains(text) {
                    blockers.push(text.clone());
                }
            }
            ProjectEvent::BlockerResolved { text, .. } => blockers.retain(|b| b != text),
            ProjectEvent::DecisionOpened { text, at_ms } => decisions.push(DecisionRecord {
                text: text.clone(),
                opened_at_ms: *at_ms,
                closed_at_ms: None,
                resolution: None,
            }),
            ProjectEvent::DecisionClosed {
                text,
                resolution,
                at_ms,
            } => {
                if let Some(open) = decisions
                    .iter_mut()
                    .rev()
                    .find(|d| d.is_open() && d.text == *text)
                {
                    open.closed_at_ms = Some(*at_ms);
                    open.resolution = Some(resolution.clone());
                }
            }
        }
    }

    if let Some(g) = primary_goal {
        file.state.primary_goal = g;
    }
    if let Some(m) = milestone {
        file.state.current_milestone = m;
    }
    if let Some(t) = milestone_started_at {
        file.state.milestone_started_at_ms = t;
    }
    if let Some(p) = progress {
        file.state.progress_pct = p.clamp(0.0, 100.0);
    }
    file.state.blocked_by = blockers;
    file.decisions = decisions;
    if let Some(last) = file.journal.iter().map(ProjectEvent::at_ms).max() {
        file.state.updated_at_ms = last;
    }
    file.digest_watermark = file.digest_watermark.min(file.journal.len());

    if file.state != before_state || file.decisions != before_decisions {
        warn!(path = %path.display(), "project state snapshot drifted from its journal — reconciled from history");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_path(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("friday-pc-{name}-{}", uuid_lite()))
    }

    fn uuid_lite() -> u64 {
        use std::time::{SystemTime, UNIX_EPOCH};
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .subsec_nanos() as u64
    }

    fn fresh() -> ProjectCognition {
        ProjectCognition::new("friday", "FRIDAY", "Build an adaptive cognitive OS")
    }

    // ── Construction & state ─────────────────────────────────────────

    #[test]
    fn fresh_cognition_starts_blank() {
        let pc = fresh();
        assert_eq!(pc.state().current_milestone, "");
        assert_eq!(pc.state().progress_pct, 0.0);
        assert!(pc.state().blocked_by.is_empty());
        assert!(pc.open_decisions().is_empty());
        assert!(pc.transitions().is_empty());
        assert!(pc.journal().is_empty());
    }

    // ── Milestones ───────────────────────────────────────────────────

    #[test]
    fn advance_milestone_records_transition_and_updates_state() {
        let mut pc = fresh();
        pc.advance_milestone("Agent Runtime", "M8 — planner spawns scoped workers");
        pc.advance_milestone("Web Agent", "real browser skill");

        let t = pc.transitions();
        assert_eq!(t.len(), 2);
        assert_eq!(t[0].from, None);
        assert_eq!(t[0].to, "Agent Runtime");
        assert_eq!(t[1].from.as_deref(), Some("Agent Runtime"));
        assert_eq!(t[1].to, "Web Agent");
        assert_eq!(pc.state().current_milestone, "Web Agent");
    }

    #[test]
    fn advance_milestone_consumes_matching_next_milestone() {
        let mut pc = fresh();
        pc.set_next_milestone("Agent Runtime");
        assert_eq!(pc.state().next_milestone.as_deref(), Some("Agent Runtime"));
        pc.advance_milestone("Agent Runtime", "M8");
        assert_eq!(pc.state().next_milestone, None);
    }

    #[test]
    fn progress_is_clamped() {
        let mut pc = fresh();
        pc.set_progress(150.0);
        assert_eq!(pc.state().progress_pct, 100.0);
        pc.set_progress(-5.0);
        assert_eq!(pc.state().progress_pct, 0.0);
        pc.set_progress(82.0);
        assert_eq!(pc.state().progress_pct, 82.0);
    }

    #[test]
    fn non_finite_progress_is_ignored() {
        let mut pc = fresh();
        pc.set_progress(82.0);
        pc.set_progress(f32::NAN);
        assert_eq!(pc.state().progress_pct, 82.0);
    }

    // ── Blockers & decisions ─────────────────────────────────────────

    #[test]
    fn blockers_dedup_and_resolve() {
        let mut pc = fresh();
        pc.add_blocker("Desktop Agent — unresolved issues");
        pc.add_blocker("Desktop Agent — unresolved issues"); // dedup
        assert_eq!(pc.state().blocked_by.len(), 1);
        assert!(!pc.resolve_blocker("does not exist"));
        assert!(pc.resolve_blocker("Desktop Agent — unresolved issues"));
        assert!(pc.state().blocked_by.is_empty());
    }

    #[test]
    fn decisions_open_close_and_derive_open_list() {
        let mut pc = fresh();
        pc.open_decision("Provider routing policy");
        pc.open_decision("Provider routing policy"); // dedup
        pc.open_decision("Persona blending");
        assert_eq!(
            pc.open_decisions(),
            vec!["Provider routing policy", "Persona blending"]
        );

        assert!(!pc.close_decision("never opened", "n/a"));
        assert!(pc.close_decision("Provider routing policy", "capability-first, cost second"));
        assert_eq!(pc.open_decisions(), vec!["Persona blending"]);

        let d = pc.decisions();
        assert_eq!(d.len(), 2);
        assert!(d[0].resolution.is_some() && d[0].closed_at_ms.is_some());
        assert!(d[1].is_open());
    }

    // ── Status briefing ──────────────────────────────────────────────

    #[test]
    fn status_briefing_covers_progress_blockers_decisions_next() {
        let mut pc = fresh();
        pc.advance_milestone("Agent Runtime", "M8");
        pc.set_progress(82.0);
        pc.set_next_milestone("Web Agent");
        pc.add_blocker("Desktop Agent — three unresolved issues");
        pc.open_decision("Provider routing policy");
        pc.open_decision("Persona blending thresholds");

        let b = pc.status_briefing();
        assert!(
            b.contains("We're 82% through the Agent Runtime milestone."),
            "briefing: {b}"
        );
        assert!(
            b.contains("1 blocker remains: Desktop Agent — three unresolved issues."),
            "briefing: {b}"
        );
        assert!(
            b.contains(
                "2 decisions are still open: Provider routing policy, Persona blending thresholds."
            ),
            "briefing: {b}"
        );
        assert!(b.contains("Next up: Web Agent."), "briefing: {b}");
    }

    #[test]
    fn status_briefing_complete_milestone_wording() {
        let mut pc = fresh();
        pc.advance_milestone("Skill Engine", "M5");
        pc.set_progress(100.0);
        let b = pc.status_briefing();
        assert!(
            b.contains("The Skill Engine milestone is complete."),
            "briefing: {b}"
        );
        assert!(!b.contains("100%"), "briefing: {b}");
    }

    #[test]
    fn status_briefing_empty_project() {
        let pc = fresh();
        assert_eq!(pc.status_briefing(), "No active milestone yet.");
    }

    // ── Reflection digest ────────────────────────────────────────────

    #[test]
    fn reflection_digest_renders_facts_and_advances_watermark() {
        let mut pc = fresh();
        pc.set_primary_goal("Ship the autonomous engineer");
        pc.advance_milestone("Skill Engine", "M5");
        pc.set_progress(40.0);
        pc.add_blocker("no web skill");
        pc.resolve_blocker("no web skill");
        pc.open_decision("provider routing");
        pc.close_decision("provider routing", "capability-first");
        let digest = pc.reflection_digest();
        assert_eq!(digest.len(), 8);
        assert_eq!(digest[0], "Primary goal set: Ship the autonomous engineer");
        assert_eq!(digest[1], "Milestone started: Skill Engine (M5)");
        // advance_milestone resets per-milestone progress to 0 (journaled).
        assert_eq!(digest[2], "Progress (Skill Engine): 0%");
        assert_eq!(digest[3], "Progress (Skill Engine): 40%");
        assert_eq!(digest[4], "Blocker added: no web skill");
        assert_eq!(digest[5], "Blocker resolved: no web skill");
        assert_eq!(digest[6], "Decision opened: provider routing");
        assert_eq!(
            digest[7],
            "Decision closed: provider routing (capability-first)"
        );

        // Watermark advanced — a second digest reports nothing new.
        assert!(pc.reflection_digest().is_empty());
    }

    #[test]
    fn facts_since_filters_by_time() {
        let mut pc = fresh();
        pc.advance_milestone("Agent Runtime", "M8");
        // Simulate a watermark after the milestone event: no facts qualify.
        let later = now_ms() + 1;
        assert!(pc.facts_since(later).is_empty());
        assert_eq!(pc.facts_since(0).len(), 2); // milestone + progress reset
    }

    #[test]
    fn digest_is_exactly_once_across_same_millisecond_boundary() {
        let mut pc = fresh();
        pc.set_progress(10.0);
        let first = pc.reflection_digest();
        assert_eq!(first.len(), 1);

        // Mutation in the *same millisecond* as the digest: monotonic event
        // timestamps guarantee it is not swallowed by the watermark.
        pc.set_progress(20.0);
        let second = pc.reflection_digest();
        assert_eq!(second.len(), 1);
        assert_eq!(second[0], "Progress: 20%");

        assert!(pc.reflection_digest().is_empty());
    }

    #[test]
    fn advance_milestone_resets_progress() {
        let mut pc = fresh();
        pc.advance_milestone("Agent Runtime", "M8");
        pc.set_progress(82.0);
        assert_eq!(pc.state().progress_pct, 82.0);

        pc.advance_milestone("Web Agent", "M9");
        assert_eq!(pc.state().progress_pct, 0.0);
        assert!(pc
            .status_briefing()
            .contains("0% through the Web Agent milestone"));
    }

    #[test]
    fn load_reconciles_snapshot_from_journal() {
        // A file whose snapshot was hand-edited to drift from its own journal:
        // the journal wins (with a warning), soft fields are preserved.
        let path = temp_path("reconcile");
        fs::write(
            &path,
            r#"{
  "schema": 1,
  "state": {"project_id": "x", "name": "kept", "primary_goal": "stale goal", "current_milestone": "Stale", "next_milestone": "Web Agent", "progress_pct": 5.0, "milestone_started_at_ms": 0, "blocked_by": ["ghost"], "updated_at_ms": 0},
  "journal": [
    {"kind": "milestone_advanced", "from": null, "to": "Agent Runtime", "reason": "M8", "at_ms": 100},
    {"kind": "progress_updated", "pct": 82.0, "milestone": "Agent Runtime", "at_ms": 200}
  ],
  "decisions": [],
  "digest_watermark": 0
}"#,
        )
        .unwrap();

        let pc = ProjectCognition::open(&path);
        // Journal-derived fields are corrected…
        assert_eq!(pc.state().current_milestone, "Agent Runtime");
        assert_eq!(pc.state().progress_pct, 82.0);
        assert!(pc.state().blocked_by.is_empty());
        // …soft (non-journaled) fields survive.
        assert_eq!(pc.state().name, "kept");
        assert_eq!(pc.state().next_milestone.as_deref(), Some("Web Agent"));
        // The stale snapshot's goal had no journal event, so it is untouched.
        assert_eq!(pc.state().primary_goal, "stale goal");
        let _ = fs::remove_file(&path);
    }

    // ── Persistence ──────────────────────────────────────────────────

    #[test]
    fn save_load_round_trip_preserves_everything() {
        let path = temp_path("roundtrip");
        let mut pc = fresh().with_path(&path);
        pc.advance_milestone("Skill Engine", "M5");
        pc.advance_milestone("Agent Runtime", "M8");
        pc.set_progress(82.0);
        pc.add_blocker("desktop runtime");
        pc.open_decision("persona blending");
        pc.set_primary_goal("One identity, many personas");
        pc.save().unwrap();
        assert!(
            !path.with_extension("json.tmp").exists(),
            "temp file must be cleaned up"
        );

        let loaded = ProjectCognition::open(&path);
        assert_eq!(loaded.state().primary_goal, "One identity, many personas");
        assert_eq!(loaded.state().current_milestone, "Agent Runtime");
        assert_eq!(loaded.state().progress_pct, 82.0);
        assert_eq!(loaded.state().blocked_by, vec!["desktop runtime"]);
        assert_eq!(loaded.open_decisions(), vec!["persona blending"]);
        assert_eq!(loaded.transitions().len(), 2);
        // 2 milestones (each 2 events) + progress + blocker + decision + goal.
        assert_eq!(loaded.journal().len(), 8);

        let _ = fs::remove_file(&path);
    }

    #[test]
    fn corrupt_file_starts_fresh_then_persists() {
        let path = temp_path("corrupt");
        fs::write(&path, "{ not json at all").unwrap();
        let mut pc = ProjectCognition::open(&path);
        assert!(pc.state().primary_goal.is_empty());
        assert!(pc.journal().is_empty());
        pc.advance_milestone("Agent Runtime", "M8");
        pc.save().unwrap();
        let reloaded = ProjectCognition::open(&path);
        assert_eq!(reloaded.state().current_milestone, "Agent Runtime");
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn schema_mismatch_starts_fresh() {
        let path = temp_path("schema");
        fs::write(
            &path,
            r#"{"schema": 99, "state": {"project_id": "x", "name": "x", "primary_goal": "g", "current_milestone": "m", "next_milestone": null, "progress_pct": 10.0, "milestone_started_at_ms": 0, "blocked_by": [], "updated_at_ms": 0}, "journal": [], "decisions": [], "digest_watermark": 0}"#,
        )
        .unwrap();
        let pc = ProjectCognition::open(&path);
        assert!(pc.state().current_milestone.is_empty());
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn out_of_range_progress_in_file_is_clamped() {
        let path = temp_path("clamp");
        // The journal says 250% (out of range). The snapshot drifted (its
        // blockers have no journal events), so the journal wins everywhere.
        fs::write(
            &path,
            r#"{"schema": 1, "state": {"project_id": "x", "name": "x", "primary_goal": "g", "current_milestone": "m", "next_milestone": null, "progress_pct": 5.0, "milestone_started_at_ms": 0, "blocked_by": ["a", "a", ""], "updated_at_ms": 0}, "journal": [{"kind": "progress_updated", "pct": 250.0, "milestone": "m", "at_ms": 100}], "decisions": [], "digest_watermark": 0}"#,
        )
        .unwrap();
        let pc = ProjectCognition::open(&path);
        // Clamped through the journal fold (250 → 100).
        assert_eq!(pc.state().progress_pct, 100.0);
        // Journal wins: no blocker events exist, so blockers are empty.
        assert!(pc.state().blocked_by.is_empty());
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn save_without_path_errors() {
        let pc = fresh();
        assert!(pc.save().is_err());
    }

    #[test]
    fn with_path_attaches_persistence() {
        let path = temp_path("builder");
        let mut pc = fresh().with_path(&path);
        pc.set_progress(50.0);
        pc.save().unwrap();
        assert!(path.exists());
        let _ = fs::remove_file(&path);
    }
}
