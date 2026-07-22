//! Observational metrics for operational validation (M12.5).
//!
//! `ExperienceMetrics` is a pure analysis artifact — it accumulates counts
//! and ratios from scenario execution but **never influences planner
//! behavior**. The planner and experience service have no dependency on
//! this module.
//!
//! ```text
//! Scenario → ExperienceMetrics → Operational Report
//! ```
//!
//! Metrics are read-only observations. They flow into the M12.5 report,
//! not into planning decisions.

use super::{ExperienceMatch, Pattern, WorkflowSuggestion};
use super::replay::WorkflowExecutionReport;

/// Accumulated session-level metrics from experience pipeline execution.
///
/// This struct is **observational only**. It is not used by the planner,
/// experience service, or any runtime component. It exists solely to
/// produce evidence for the operational validation report.
#[derive(Debug, Clone, Default)]
pub struct ExperienceMetrics {
    /// Total queries made to ExperienceService.
    pub queries: usize,
    /// Matches with trust_score > 0.5.
    pub useful_matches: usize,
    /// Patterns mined from timeline.
    pub patterns_mined: usize,
    /// Workflow suggestions generated.
    pub suggestions_generated: usize,
    /// Total replay reports produced.
    pub replay_reports: usize,
    /// Reuse ratios from each replay report.
    pub replay_reuse_ratios: Vec<f64>,
    /// Total planner-side queries (queries originating from planning code).
    pub planner_queries: usize,
    /// Applicability checks that passed.
    pub replay_applicable: usize,
    /// Applicability checks that blocked replay.
    pub replay_blocked: usize,
}

/// Summary statistics derived from accumulated metrics.
#[derive(Debug, Clone)]
pub struct MetricsSummary {
    pub total_queries: usize,
    pub useful_match_rate: f64,
    pub avg_reuse_ratio: f64,
    pub replay_applicable_rate: f64,
    pub patterns_mined: usize,
    pub suggestions_generated: usize,
}

impl ExperienceMetrics {
    pub fn new() -> Self {
        Self::default()
    }

    /// Record a query and its returned matches.
    pub fn record_query(&mut self, matches: &[ExperienceMatch]) {
        self.queries += 1;
        self.useful_matches += matches.iter().filter(|m| m.trust_score > 0.5).count();
    }

    /// Record a planner-side query (originates from planning code).
    pub fn record_planner_query(&mut self, matches: &[ExperienceMatch]) {
        self.planner_queries += 1;
        self.queries += 1;
        self.useful_matches += matches.iter().filter(|m| m.trust_score > 0.5).count();
    }

    /// Record patterns mined from a timeline.
    pub fn record_patterns(&mut self, patterns: &[Pattern]) {
        self.patterns_mined = patterns.len();
    }

    /// Record suggestions generated.
    pub fn record_suggestions(&mut self, suggestions: &[WorkflowSuggestion]) {
        self.suggestions_generated = suggestions.len();
    }

    /// Record a replay report.
    pub fn record_replay(&mut self, report: &WorkflowExecutionReport) {
        self.replay_reports += 1;
        self.replay_reuse_ratios.push(report.reuse_ratio());
    }

    /// Record an applicability check result.
    pub fn record_applicability(&mut self, applicable: bool) {
        if applicable {
            self.replay_applicable += 1;
        } else {
            self.replay_blocked += 1;
        }
    }

    /// Derive summary statistics from accumulated metrics.
    pub fn summary(&self) -> MetricsSummary {
        let useful_match_rate = if self.queries > 0 {
            self.useful_matches as f64 / self.queries as f64
        } else {
            0.0
        };

        let avg_reuse_ratio = if self.replay_reuse_ratios.is_empty() {
            0.0
        } else {
            self.replay_reuse_ratios.iter().sum::<f64>() / self.replay_reuse_ratios.len() as f64
        };

        let total_applicability = self.replay_applicable + self.replay_blocked;
        let replay_applicable_rate = if total_applicability > 0 {
            self.replay_applicable as f64 / total_applicability as f64
        } else {
            0.0
        };

        MetricsSummary {
            total_queries: self.queries,
            useful_match_rate,
            avg_reuse_ratio,
            replay_applicable_rate,
            patterns_mined: self.patterns_mined,
            suggestions_generated: self.suggestions_generated,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::{
        GoalOutcome, MatchReason, TrustComponents, ExperienceMatch,
    };
    use super::super::replay::WorkflowExecutionReport;
    use uuid::Uuid;

    fn make_match(trust: f64) -> ExperienceMatch {
        ExperienceMatch {
            similarity_score: 0.8,
            trust_score: trust,
            trust_components: TrustComponents {
                similarity: 0.8,
                recency: 0.9,
                outcome_quality: 0.9,
                repetition: 0.5,
            },
            match_reason: MatchReason::SimilarGoal,
            goal_id: Uuid::new_v4(),
            workflow_id: None,
            outcome: GoalOutcome::Success,
            reflection: Vec::new(),
            artifacts: Vec::new(),
            commit_hash: None,
            goal_title: "test".into(),
        }
    }

    fn make_report(reused: usize, adapted: usize, skipped: usize, new: usize) -> WorkflowExecutionReport {
        WorkflowExecutionReport {
            workflow_id: None,
            pattern_id: None,
            reused_steps: (0..reused).map(|i| format!("r{i}")).collect(),
            adapted_steps: (0..adapted).map(|i| format!("a{i}")).collect(),
            skipped_steps: (0..skipped).map(|i| format!("s{i}")).collect(),
            new_steps: (0..new).map(|i| format!("n{i}")).collect(),
            completed_at: chrono::Utc::now(),
            outcome: GoalOutcome::Success,
        }
    }

    #[test]
    fn metrics_starts_empty() {
        let m = ExperienceMetrics::new();
        assert_eq!(m.queries, 0);
        assert_eq!(m.useful_matches, 0);
        assert_eq!(m.patterns_mined, 0);
        assert_eq!(m.suggestions_generated, 0);
        assert_eq!(m.replay_reports, 0);
        assert!(m.replay_reuse_ratios.is_empty());
    }

    #[test]
    fn record_query_counts_useful() {
        let mut m = ExperienceMetrics::new();
        m.record_query(&[make_match(0.7), make_match(0.3), make_match(0.6)]);
        assert_eq!(m.queries, 1);
        assert_eq!(m.useful_matches, 2); // 0.7 and 0.6 are > 0.5
    }

    #[test]
    fn record_replay_accumulates_ratios() {
        let mut m = ExperienceMetrics::new();
        // reused:3, adapted:1, skipped:1 → original=5, kept=4 → reuse=0.8
        m.record_replay(&make_report(3, 1, 1, 0));
        // reused:4, adapted:0, skipped:0 → original=4, kept=4 → reuse=1.0
        m.record_replay(&make_report(4, 0, 0, 1));
        assert_eq!(m.replay_reports, 2);
        assert!((m.replay_reuse_ratios[0] - 0.8).abs() < 0.01);
        assert!((m.replay_reuse_ratios[1] - 1.0).abs() < 0.01);
    }

    #[test]
    fn record_applicability_tracks_both() {
        let mut m = ExperienceMetrics::new();
        m.record_applicability(true);
        m.record_applicability(true);
        m.record_applicability(false);
        assert_eq!(m.replay_applicable, 2);
        assert_eq!(m.replay_blocked, 1);
    }

    #[test]
    fn summary_computes_rates() {
        let mut m = ExperienceMetrics::new();
        m.record_query(&[make_match(0.8)]);
        m.record_query(&[make_match(0.3)]);
        // reused:4, adapted:1, skipped:0 → original=5, kept=5 → reuse=1.0
        m.record_replay(&make_report(4, 1, 0, 0));
        m.record_applicability(true);
        m.record_applicability(false);

        let s = m.summary();
        assert_eq!(s.total_queries, 2);
        assert!((s.useful_match_rate - 0.5).abs() < 0.01);
        assert!((s.avg_reuse_ratio - 1.0).abs() < 0.01);
        assert!((s.replay_applicable_rate - 0.5).abs() < 0.01);
    }

    #[test]
    fn summary_zero_division_safe() {
        let m = ExperienceMetrics::new();
        let s = m.summary();
        assert_eq!(s.total_queries, 0);
        assert_eq!(s.useful_match_rate, 0.0);
        assert_eq!(s.avg_reuse_ratio, 0.0);
        assert_eq!(s.replay_applicable_rate, 0.0);
    }
}
