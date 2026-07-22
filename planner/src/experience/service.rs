//! Experience retrieval service for the Experience Layer.
//!
//! The service exposes question-oriented methods that the planner calls.
//! Internally it delegates to the `ExperienceIndexer` — the planner never
//! learns how information was retrieved.
//!
//! Success criteria from ADR-0008/0009:
//! 1. Have I done something similar before?  → `similar()`
//! 2. What happened last time?               → `last_attempt()`
//! 3. Which workflow succeeded?              → `successful_workflows()`
//! 4. Why did it succeed/fail?               → `explain_previous_outcome()`
//! 5. How strongly should I trust this?      → `ExperienceMatch.trust_score`

use super::indexer::ExperienceIndexer;
use super::miner::PatternMiner;
use super::provider::ExperienceProvider;
use super::suggestion::{SuggestionEngine, SuggestionPolicy, WorkflowSuggestion};
use super::{
    ExperienceMatch, GoalOutcome, MatchReason, Pattern, ReflectionRecord, TrustComponents,
    TimelineEntry, Workflow,
};

/// Answer builder: constructs a structured `ExperienceMatch` from an indexer
/// record, similarity score, and match reason.
struct MatchBuilder;

impl MatchBuilder {
    fn build(
        record: &super::indexer::GoalRecord,
        reason: MatchReason,
        similarity: f64,
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

        let trust_components = compute_trust(record, similarity);

        ExperienceMatch {
            similarity_score: similarity,
            trust_score: trust_components.aggregated(),
            trust_components,
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

/// Compute decomposed trust factors for a goal record.
fn compute_trust(record: &super::indexer::GoalRecord, similarity: f64) -> TrustComponents {
    // Recency: 1.0 if just happened, decays over 7 days.
    let hours_ago = (chrono::Utc::now() - record.last_activity).num_hours() as f64;
    let recency = if hours_ago < 1.0 {
        1.0
    } else if hours_ago < 24.0 {
        0.9
    } else if hours_ago < 168.0 {
        0.5
    } else {
        0.1
    };

    // Outcome quality: success is high, failure is low.
    let outcome_quality = match record.outcome {
        GoalOutcome::Success => 0.9,
        GoalOutcome::PartiallyCompleted => 0.5,
        GoalOutcome::Failure => 0.3,
        GoalOutcome::Cancelled => 0.2,
        GoalOutcome::Unknown => 0.1,
    };

    // Repetition: more entries = more observed (normalized, capped at 1.0).
    let entry_count = record.entries.len() as f64;
    let repetition = (entry_count / 20.0).min(1.0);

    TrustComponents {
        similarity,
        recency,
        outcome_quality,
        repetition,
    }
}

/// The Experience Service: answers planner questions about past experience.
///
/// The planner calls question-oriented methods. Internally the service uses
/// chronological and semantic indexes, but the planner never sees this.
pub struct ExperienceService {
    indexer: ExperienceIndexer,
    miner: PatternMiner,
    patterns: Vec<Pattern>,
    suggestion_engine: SuggestionEngine,
}

impl ExperienceService {
    pub fn new() -> Self {
        Self {
            indexer: ExperienceIndexer::new(),
            miner: PatternMiner::new(),
            patterns: Vec::new(),
            suggestion_engine: SuggestionEngine::new(SuggestionPolicy::default()),
        }
    }

    /// Create with a custom suggestion policy.
    pub fn with_policy(policy: SuggestionPolicy) -> Self {
        Self {
            indexer: ExperienceIndexer::new(),
            miner: PatternMiner::new(),
            patterns: Vec::new(),
            suggestion_engine: SuggestionEngine::new(policy),
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
        self.patterns = self.miner.mine(entries);
    }

    /// #1 "Have I done something similar before?"
    ///
    /// Returns up to `max_results` matches sorted by similarity descending.
    pub fn similar(&self, goal_title: &str, max_results: usize) -> Vec<ExperienceMatch> {
        self.indexer
            .similar(goal_title, max_results)
            .into_iter()
            .map(|(key, score)| {
                let record = self.indexer.get_record_by_key(key).unwrap();
                MatchBuilder::build(record, MatchReason::SimilarGoal, score)
            })
            .collect()
    }

    /// #2 "What happened last time?"
    ///
    /// Returns the most recent attempt for a goal, with full context.
    pub fn last_attempt(&self, goal_id: uuid::Uuid) -> Option<ExperienceMatch> {
        let record = self.indexer.last_attempt(goal_id)?;
        Some(MatchBuilder::build(
            record,
            MatchReason::RecentAttempt,
            1.0, // recency match is exact by definition
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
            .filter(|r| ExperienceIndexer::text_similarity(goal_title, &r.title) > 0.1)
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
    /// Returns trust components for a specific goal. The planner can use
    /// `trust_score` (aggregated) or inspect individual factors.
    pub fn trust_components(&self, goal_id: uuid::Uuid) -> Option<TrustComponents> {
        let record = self.indexer.last_attempt(goal_id)?;
        let similarity = 0.0; // no query context for standalone trust
        Some(compute_trust(record, similarity))
    }

    /// Number of indexed goals.
    pub fn goal_count(&self) -> usize {
        self.indexer.goal_count()
    }

    // -- Pattern queries (M12.2) --

    /// All mined patterns, sorted by frequency descending.
    pub fn patterns(&self) -> &[Pattern] {
        &self.patterns
    }

    /// Patterns that start with the given capability prefix.
    pub fn patterns_starting_with(&self, capability: &str) -> Vec<&Pattern> {
        self.patterns
            .iter()
            .filter(|p| p.capabilities.first().map(|s| s.as_str()) == Some(capability))
            .collect()
    }

    /// Patterns that contain the given capability at any position.
    pub fn patterns_containing(&self, capability: &str) -> Vec<&Pattern> {
        self.patterns
            .iter()
            .filter(|p| p.capabilities.iter().any(|c| c == capability))
            .collect()
    }

    /// Patterns with success rate above the given threshold.
    pub fn reliable_patterns(&self, min_success_rate: f64) -> Vec<&Pattern> {
        self.patterns
            .iter()
            .filter(|p| p.success_rate >= min_success_rate)
            .collect()
    }

    /// Number of mined patterns.
    pub fn pattern_count(&self) -> usize {
        self.patterns.len()
    }

    // -- Workflow Suggestions (M12.3) --

    /// Generate workflow suggestions from mined patterns.
    /// Returns suggestions sorted by confidence descending.
    pub fn suggestions(&self) -> Vec<WorkflowSuggestion> {
        self.suggestion_engine.suggest(&self.patterns)
    }

    /// Number of workflow suggestions.
    pub fn suggestion_count(&self) -> usize {
        self.suggestion_engine.suggest(&self.patterns).len()
    }
}

impl Default for ExperienceService {
    fn default() -> Self {
        Self::new()
    }
}

impl ExperienceProvider for ExperienceService {
    fn similar(&self, goal_title: &str, max_results: usize) -> Vec<ExperienceMatch> {
        self.indexer
            .similar(goal_title, max_results)
            .into_iter()
            .map(|(key, score)| {
                let record = self.indexer.get_record_by_key(key).unwrap();
                MatchBuilder::build(record, MatchReason::SimilarGoal, score)
            })
            .collect()
    }

    fn last_attempt(&self, goal_id: uuid::Uuid) -> Option<ExperienceMatch> {
        let record = self.indexer.last_attempt(goal_id)?;
        Some(MatchBuilder::build(
            record,
            MatchReason::RecentAttempt,
            1.0,
        ))
    }

    fn successful_workflows(&self, goal_title: &str) -> Vec<super::indexer::GoalRecord> {
        self.indexer
            .all_records()
            .into_iter()
            .filter(|r| r.outcome == GoalOutcome::Success)
            .filter(|r| ExperienceIndexer::text_similarity(goal_title, &r.title) > 0.1)
            .cloned()
            .collect()
    }

    fn suggestions(&self) -> Vec<WorkflowSuggestion> {
        self.suggestion_engine.suggest(&self.patterns)
    }

    fn load_reflections(&self, goal_ids: &[uuid::Uuid]) -> Vec<ReflectionRecord> {
        goal_ids
            .iter()
            .filter_map(|id| self.indexer.get_record(*id))
            .flat_map(|r| r.reflections.clone())
            .collect()
    }
}
