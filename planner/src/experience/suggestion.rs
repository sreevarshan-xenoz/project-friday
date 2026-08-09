//! Workflow Suggestion Engine (M12.3).
//!
//! The engine is a **policy layer** over mined patterns. It decides whether
//! a pattern is good enough to recommend. Pattern mining is descriptive
//! (what happened); suggestions are normative (what should be recommended).
//!
//! Pipeline:
//!
//! ```text
//! Timeline → PatternMiner → Pattern → SuggestionEngine → WorkflowSuggestion
//! ```
//!
//! Every suggestion is explainable: it answers "why am I seeing this?"

use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};

use super::miner::Pattern;

/// A suggested workflow, ready for the planner to consider.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowSuggestion {
    /// The underlying pattern.
    pub pattern_id: uuid::Uuid,
    /// Semantic capability sequence: `["filesystem.write", "git.commit"]`.
    pub capabilities: Vec<String>,
    /// Human-readable explanation of why this was suggested.
    pub reasons: Vec<String>,
    /// Overall confidence in this suggestion (0.0–1.0).
    pub confidence: f64,
    /// When this suggestion was generated.
    pub generated_at: DateTime<Utc>,
    /// How many times the pattern was observed.
    pub frequency: u32,
    /// Success rate of the pattern (0.0–1.0).
    pub success_rate: f64,
    /// When the pattern was last seen.
    pub last_seen: DateTime<Utc>,
}

/// Configuration for the suggestion engine. Conservative defaults prevent
/// recommending workflows based on lucky coincidences.
#[derive(Debug, Clone)]
pub struct SuggestionPolicy {
    /// Minimum times a pattern must have been observed.
    pub min_frequency: u32,
    /// Minimum success rate required (0.0–1.0).
    pub min_success_rate: f64,
    /// Maximum age of last observation (e.g. 30 days).
    pub max_age: Duration,
    /// Maximum number of suggestions to return.
    pub max_suggestions: usize,
}

impl Default for SuggestionPolicy {
    fn default() -> Self {
        Self {
            min_frequency: 5,
            min_success_rate: 0.90,
            max_age: Duration::days(30),
            max_suggestions: 10,
        }
    }
}

impl SuggestionPolicy {
    /// Create a strict policy (fewer suggestions, higher quality).
    pub fn strict() -> Self {
        Self {
            min_frequency: 10,
            min_success_rate: 0.95,
            max_age: Duration::days(14),
            max_suggestions: 5,
        }
    }

    /// Create a lenient policy (more suggestions, lower threshold).
    pub fn lenient() -> Self {
        Self {
            min_frequency: 3,
            min_success_rate: 0.80,
            max_age: Duration::days(60),
            max_suggestions: 20,
        }
    }
}

/// The Suggestion Engine: decides whether patterns are good enough to recommend.
pub struct SuggestionEngine {
    policy: SuggestionPolicy,
}

impl SuggestionEngine {
    pub fn new(policy: SuggestionPolicy) -> Self {
        Self { policy }
    }

    /// Evaluate patterns and produce suggestions for those meeting the policy.
    pub fn suggest(&self, patterns: &[Pattern]) -> Vec<WorkflowSuggestion> {
        let now = Utc::now();

        let mut suggestions: Vec<WorkflowSuggestion> = patterns
            .iter()
            .filter_map(|p| self.evaluate(p, now))
            .collect();

        // Sort by confidence descending.
        suggestions.sort_by(|a, b| {
            b.confidence
                .partial_cmp(&a.confidence)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        suggestions.truncate(self.policy.max_suggestions);
        suggestions
    }

    /// Evaluate a single pattern against the policy.
    fn evaluate(&self, pattern: &Pattern, now: DateTime<Utc>) -> Option<WorkflowSuggestion> {
        let mut reasons = Vec::new();
        let mut passed = true;

        // Frequency check.
        if pattern.frequency >= self.policy.min_frequency {
            reasons.push(format!("executed {} times", pattern.frequency));
        } else {
            passed = false;
        }

        // Success rate check.
        if pattern.success_rate >= self.policy.min_success_rate {
            reasons.push(format!("success rate {:.0}%", pattern.success_rate * 100.0));
        } else {
            passed = false;
        }

        // Recency check.
        let age = now - pattern.last_seen;
        if age <= self.policy.max_age {
            if age < Duration::hours(1) {
                reasons.push("used very recently".to_string());
            } else if age < Duration::days(1) {
                reasons.push("used today".to_string());
            } else if age < Duration::days(7) {
                reasons.push(format!("used {} days ago", age.num_days()));
            } else {
                reasons.push(format!("used {} weeks ago", age.num_weeks()));
            }
        } else {
            passed = false;
        }

        if !passed {
            return None;
        }

        // Compute confidence: weighted combination of frequency, success, recency.
        let freq_score = (pattern.frequency as f64 / 20.0).min(1.0);
        let success_score = pattern.success_rate;
        let recency_score =
            1.0 - (age.num_seconds() as f64 / self.policy.max_age.num_seconds() as f64).min(1.0);

        let confidence = freq_score * 0.3 + success_score * 0.5 + recency_score * 0.2;

        Some(WorkflowSuggestion {
            pattern_id: pattern.id,
            capabilities: pattern.capabilities.clone(),
            reasons,
            confidence,
            generated_at: now,
            frequency: pattern.frequency,
            success_rate: pattern.success_rate,
            last_seen: pattern.last_seen,
        })
    }
}

impl Default for SuggestionEngine {
    fn default() -> Self {
        Self::new(SuggestionPolicy::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn pattern(caps: Vec<&str>, freq: u32, success_rate: f64, days_ago: i64) -> Pattern {
        Pattern {
            id: uuid::Uuid::new_v4(),
            capabilities: caps.into_iter().map(String::from).collect(),
            frequency: freq,
            success_rate,
            last_seen: Utc::now() - Duration::days(days_ago),
            average_duration_secs: None,
            source_goal_ids: vec![],
        }
    }

    #[test]
    fn suggests_pattern_meeting_all_thresholds() {
        let engine = SuggestionEngine::new(SuggestionPolicy::default());
        let patterns = vec![pattern(vec!["fs.write", "git.commit"], 10, 1.0, 1)];
        let suggestions = engine.suggest(&patterns);
        assert_eq!(suggestions.len(), 1);
        assert!(suggestions[0].confidence > 0.5);
        assert!(suggestions[0].reasons.len() >= 2);
    }

    #[test]
    fn rejects_pattern_below_frequency() {
        let engine = SuggestionEngine::new(SuggestionPolicy::default());
        let patterns = vec![
            pattern(vec!["fs.write", "git.commit"], 2, 1.0, 1), // freq < 5
        ];
        let suggestions = engine.suggest(&patterns);
        assert!(suggestions.is_empty());
    }

    #[test]
    fn rejects_pattern_below_success_rate() {
        let engine = SuggestionEngine::new(SuggestionPolicy::default());
        let patterns = vec![
            pattern(vec!["fs.write", "git.commit"], 10, 0.7, 1), // 70% < 90%
        ];
        let suggestions = engine.suggest(&patterns);
        assert!(suggestions.is_empty());
    }

    #[test]
    fn rejects_pattern_too_old() {
        let engine = SuggestionEngine::new(SuggestionPolicy::default());
        let patterns = vec![
            pattern(vec!["fs.write", "git.commit"], 10, 1.0, 45), // 45 days > 30
        ];
        let suggestions = engine.suggest(&patterns);
        assert!(suggestions.is_empty());
    }

    #[test]
    fn strict_policy_is_more_conservative() {
        let lenient = SuggestionEngine::new(SuggestionPolicy::lenient());
        let strict = SuggestionEngine::new(SuggestionPolicy::strict());
        let patterns = vec![pattern(vec!["fs.write", "git.commit"], 7, 0.92, 20)];
        assert_eq!(lenient.suggest(&patterns).len(), 1);
        assert!(strict.suggest(&patterns).is_empty());
    }

    #[test]
    fn explanation_lists_all_reasons() {
        let engine = SuggestionEngine::new(SuggestionPolicy::default());
        let patterns = vec![pattern(vec!["fs.write", "git.commit"], 14, 1.0, 0)];
        let suggestions = engine.suggest(&patterns);
        let reasons = &suggestions[0].reasons;
        assert!(reasons.iter().any(|r| r.contains("14 times")));
        assert!(reasons.iter().any(|r| r.contains("100%")));
    }

    #[test]
    fn max_suggestions_respected() {
        let policy = SuggestionPolicy {
            min_frequency: 1,
            min_success_rate: 0.0,
            max_age: Duration::days(365),
            max_suggestions: 2,
        };
        let engine = SuggestionEngine::new(policy);
        let patterns = vec![
            pattern(vec!["a", "b"], 10, 1.0, 1),
            pattern(vec!["b", "c"], 10, 1.0, 1),
            pattern(vec!["c", "d"], 10, 1.0, 1),
        ];
        let suggestions = engine.suggest(&patterns);
        assert_eq!(suggestions.len(), 2);
    }
}
