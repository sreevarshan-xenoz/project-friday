//! Experience retrieval service for the Experience Layer.
//!
//! The service exposes question-oriented methods that the planner calls.
//! Internally it delegates to the `ExperienceIndexer` — the planner never
//! learns how information was retrieved.
//!
//! Success criteria from ADR-0008:
//! 1. Have I done something similar before?  → `similar()`
//! 2. What happened last time?               → `last_attempt()`
//! 3. Which workflow succeeded?              → `successful_workflows()`
//! 4. Why did it succeed/fail?               → `explain_previous_outcome()`
//! 5. How strongly should I trust this?      → `ExperienceMatch.confidence`

use super::indexer::ExperienceIndexer;
use super::{ExperienceMatch, GoalOutcome, MatchReason, ReflectionRecord, TimelineEntry, Workflow};

/// Answer builder: constructs a structured `ExperienceMatch` from an indexer
/// record and a match reason.
struct MatchBuilder;

impl MatchBuilder {
    fn build(
        record: &super::indexer::GoalRecord,
        reason: MatchReason,
        confidence: f64,
    ) -> ExperienceMatch {
        let artifacts = record
            .entries
            .iter()
            .filter(|e| e.event_kind == "artifact.created")
            .filter_map(|e| e.metadata.get("path")?.as_str().map(String::from))
            .collect();

        let commit_hash = record
            .entries
            .iter()
            .find_map(|e| e.metadata.get("commit_hash")?.as_str().map(String::from));

        ExperienceMatch {
            confidence,
            match_reason: reason,
            goal_id: record.goal_id.0,
            workflow_id: record.workflows.first().map(|w| w.id),
            outcome: record.outcome.clone(),
            reflection: record.reflections.clone(),
            artifacts,
            commit_hash,
            goal_title: record.title.clone(),
        }
    }
}

/// The Experience Service: answers planner questions about past experience.
///
/// The planner calls question-oriented methods. Internally the service uses
/// chronological and semantic indexes, but the planner never sees this.
pub struct ExperienceService {
    indexer: ExperienceIndexer,
}

impl ExperienceService {
    pub fn new() -> Self {
        Self {
            indexer: ExperienceIndexer::new(),
        }
    }

    /// Rebuild indexes from timeline data. Call after loading or when the
    /// timeline has grown.
    pub fn rebuild(
        &mut self,
        entries: &[TimelineEntry],
        workflows: &[Workflow],
        reflections: &[ReflectionRecord],
    ) {
        self.indexer.build(entries, workflows, reflections);
    }

    /// #1 "Have I done something similar before?"
    ///
    /// Returns up to `max_results` matches sorted by confidence descending.
    pub fn similar(&self, goal_title: &str, max_results: usize) -> Vec<ExperienceMatch> {
        self.indexer
            .similar(goal_title, max_results)
            .into_iter()
            .map(|(key, score)| {
                let record = self.indexer.get_record(key.0).unwrap();
                MatchBuilder::build(record, MatchReason::SimilarGoal, score)
            })
            .collect()
    }

    /// #2 "What happened last time?"
    ///
    /// Returns the most recent attempt for a goal, with full context.
    pub fn last_attempt(&self, goal_id: uuid::Uuid) -> Option<ExperienceMatch> {
        let record = self.indexer.last_attempt(goal_id)?;
        let confidence = if record.outcome == GoalOutcome::Success {
            0.9
        } else {
            0.7
        };
        Some(MatchBuilder::build(
            record,
            MatchReason::RecentAttempt,
            confidence,
        ))
    }

    /// #3 "Which workflow succeeded?"
    ///
    /// Returns all successful workflows, sorted by recency.
    pub fn successful_workflows(&self, goal_title: &str) -> Vec<super::indexer::GoalRecord> {
        self.indexer
            .all_records()
            .into_iter()
            .filter(|r| r.outcome == GoalOutcome::Success)
            .filter(|r| {
                ExperienceIndexer::text_similarity(goal_title, &r.title) > 0.1
            })
            .cloned()
            .collect()
    }

    /// #4 "Why did it succeed/fail?"
    ///
    /// Returns reflections and outcome for a specific goal attempt.
    pub fn explain_previous_outcome(
        &self,
        goal_id: uuid::Uuid,
    ) -> Option<(GoalOutcome, Vec<ReflectionRecord>)> {
        let record = self.indexer.last_attempt(goal_id)?;
        Some((record.outcome.clone(), record.reflections.clone()))
    }

    /// #5 "How strongly should I trust this?"
    ///
    /// Trust is initially derived from confidence, but can later incorporate
    /// recency, repetition, or other signals.
    pub fn trust_score(&self, goal_id: uuid::Uuid) -> f64 {
        let Some(record) = self.indexer.last_attempt(goal_id) else {
            return 0.0;
        };

        let base_confidence = match record.outcome {
            GoalOutcome::Success => 0.9,
            GoalOutcome::PartiallyCompleted => 0.5,
            GoalOutcome::Failure => 0.3,
            GoalOutcome::Cancelled => 0.2,
            GoalOutcome::Unknown => 0.1,
        };

        // Boost for recency (within last 24h).
        let hours_ago = (chrono::Utc::now() - record.last_activity).num_hours() as f64;
        let recency_bonus: f64 = if hours_ago < 24.0 {
            0.05
        } else if hours_ago < 168.0 {
            0.02
        } else {
            0.0
        };

        (base_confidence + recency_bonus).min(1.0)
    }

    /// Number of indexed goals.
    pub fn goal_count(&self) -> usize {
        self.indexer.goal_count()
    }
}

impl Default for ExperienceService {
    fn default() -> Self {
        Self::new()
    }
}
