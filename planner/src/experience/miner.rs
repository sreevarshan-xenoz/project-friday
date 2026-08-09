//! Pattern mining over semantic capability sequences.
//!
//! The miner operates on timeline entries and produces `Pattern` objects
//! that describe frequently-observed capability sequences. This is
//! **descriptive** (what happened), not **prescriptive** (what to do).
//!
//! Mining is a pure derivation: it reads timeline entries and produces
//! patterns. It never mutates the timeline.
//!
//! Pattern objects are stored in the `TimelineStore` for fast lookup,
//! and the `ExperienceService` exposes them to the planner.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::TimelineEntry;

/// A mined pattern from capability sequences in the timeline.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Pattern {
    /// Unique pattern identifier.
    pub id: Uuid,
    /// Semantic capability sequence: `["filesystem.write", "git.status", "git.commit"]`.
    pub capabilities: Vec<String>,
    /// How many times this exact sequence was observed.
    pub frequency: u32,
    /// Proportion of observations that succeeded (0.0–1.0).
    pub success_rate: f64,
    /// When this pattern was last observed.
    pub last_seen: DateTime<Utc>,
    /// Average duration across observations (seconds). `None` if only one observation.
    pub average_duration_secs: Option<f64>,
    /// Goal IDs that produced this pattern (for provenance).
    pub source_goal_ids: Vec<Uuid>,
}

/// N-gram extracted from a timeline entry sequence.
#[derive(Debug, Clone, Hash, Eq, PartialEq)]
struct NGram {
    capabilities: Vec<String>,
}

/// Outcome of a single observation of a capability sequence.
#[derive(Debug, Clone)]
struct Observation {
    success: bool,
    timestamp: DateTime<Utc>,
    duration_secs: Option<f64>,
    goal_id: Uuid,
}

/// The Pattern Miner: extracts frequent capability sequences from the timeline.
pub struct PatternMiner {
    /// Minimum observations required to consider a pattern.
    pub min_frequency: u32,
    /// Minimum n-gram length.
    pub min_ngram: usize,
    /// Maximum n-gram length.
    pub max_ngram: usize,
}

impl PatternMiner {
    pub fn new() -> Self {
        Self {
            min_frequency: 2,
            min_ngram: 2,
            max_ngram: 6,
        }
    }

    /// Mine patterns from a collection of timeline entries.
    ///
    /// Returns patterns sorted by frequency descending, filtered by
    /// `min_frequency`.
    pub fn mine(&self, entries: &[TimelineEntry]) -> Vec<Pattern> {
        if entries.is_empty() {
            return Vec::new();
        }

        // Group entries by goal_id.
        let mut goal_groups: Vec<Vec<&TimelineEntry>> = Vec::new();
        let mut current_goal: Option<Uuid> = None;
        let mut current_group: Vec<&TimelineEntry> = Vec::new();

        // Sort by timestamp to ensure correct ordering.
        let mut sorted = entries.to_vec();
        sorted.sort_by(|a, b| a.timestamp.cmp(&b.timestamp));

        for entry in &sorted {
            if current_goal != Some(entry.goal_id) {
                if !current_group.is_empty() {
                    goal_groups.push(current_group);
                }
                current_goal = Some(entry.goal_id);
                current_group = Vec::new();
            }
            current_group.push(entry);
        }
        if !current_group.is_empty() {
            goal_groups.push(current_group);
        }

        // Extract n-grams and observations.
        let mut ngram_observations: std::collections::HashMap<NGram, Vec<Observation>> =
            std::collections::HashMap::new();

        for group in &goal_groups {
            // Extract semantic capabilities (excluding goal.completed, etc.).
            let capabilities: Vec<String> = group
                .iter()
                .filter(|e| {
                    !e.capability.is_empty()
                        && (e.event_kind.starts_with("exec.")
                            || e.event_kind == "artifact.created"
                            || e.event_kind.starts_with("git."))
                })
                .map(|e| e.capability.clone())
                .collect();

            if capabilities.len() < self.min_ngram {
                continue;
            }

            // Determine success.
            let success = group.iter().any(|e| e.event_kind == "goal.completed");
            let goal_id = group[0].goal_id;
            let timestamp = group.last().unwrap().timestamp;

            // Compute duration if possible.
            let duration_secs = if group.len() >= 2 {
                let start = group.first().unwrap().timestamp;
                let end = group.last().unwrap().timestamp;
                Some((end - start).num_milliseconds() as f64 / 1000.0)
            } else {
                None
            };

            let obs = Observation {
                success,
                timestamp,
                duration_secs,
                goal_id,
            };

            // Extract all n-grams of size min_ngram..=max_ngram.
            for n in self.min_ngram..=self.max_ngram.min(capabilities.len()) {
                for window in capabilities.windows(n) {
                    let ngram = NGram {
                        capabilities: window.to_vec(),
                    };
                    ngram_observations
                        .entry(ngram)
                        .or_default()
                        .push(obs.clone());
                }
            }
        }

        // Build patterns from n-grams meeting min_frequency.
        let mut patterns: Vec<Pattern> = ngram_observations
            .into_iter()
            .filter(|(_, obs)| obs.len() as u32 >= self.min_frequency)
            .map(|(ngram, obs)| {
                let frequency = obs.len() as u32;
                let successes = obs.iter().filter(|o| o.success).count() as f64;
                let success_rate = if frequency > 0 {
                    successes / frequency as f64
                } else {
                    0.0
                };
                let last_seen = obs.iter().map(|o| o.timestamp).max().unwrap();
                let durations: Vec<f64> = obs.iter().filter_map(|o| o.duration_secs).collect();
                let average_duration_secs = if durations.is_empty() {
                    None
                } else {
                    Some(durations.iter().sum::<f64>() / durations.len() as f64)
                };
                let mut goal_ids: Vec<Uuid> = obs.iter().map(|o| o.goal_id).collect();
                goal_ids.sort();
                goal_ids.dedup();

                Pattern {
                    id: Uuid::new_v4(),
                    capabilities: ngram.capabilities,
                    frequency,
                    success_rate,
                    last_seen,
                    average_duration_secs,
                    source_goal_ids: goal_ids,
                }
            })
            .collect();

        patterns.sort_by(|a, b| b.frequency.cmp(&a.frequency));
        patterns
    }
}

impl Default for PatternMiner {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn entry(goal_id: Uuid, cap: &str, event_kind: &str, ts: DateTime<Utc>) -> TimelineEntry {
        TimelineEntry {
            version: 1,
            id: Uuid::new_v4(),
            timestamp: ts,
            goal_id,
            plan_id: Uuid::new_v4(),
            execution_id: Uuid::new_v4(),
            event_kind: event_kind.to_string(),
            capability: cap.to_string(),
            metadata: serde_json::json!({}),
            parent: None,
            tags: vec![],
            annotations: HashMap::new(),
        }
    }

    #[test]
    fn mine_finds_repeated_sequences() {
        let goal1 = Uuid::new_v4();
        let goal2 = Uuid::new_v4();
        let base = Utc::now();

        let entries = vec![
            entry(goal1, "filesystem.write", "exec.action", base),
            entry(
                goal1,
                "git.commit",
                "exec.action",
                base + chrono::Duration::seconds(1),
            ),
            entry(
                goal1,
                "",
                "goal.completed",
                base + chrono::Duration::seconds(2),
            ),
            entry(
                goal2,
                "filesystem.write",
                "exec.action",
                base + chrono::Duration::seconds(10),
            ),
            entry(
                goal2,
                "git.commit",
                "exec.action",
                base + chrono::Duration::seconds(11),
            ),
            entry(
                goal2,
                "",
                "goal.completed",
                base + chrono::Duration::seconds(12),
            ),
        ];

        let miner = PatternMiner::new();
        let patterns = miner.mine(&entries);

        assert!(!patterns.is_empty(), "should find at least one pattern");
        // The [filesystem.write, git.commit] pattern should appear twice.
        let p = patterns.iter().find(|p| p.capabilities.len() == 2).unwrap();
        assert_eq!(p.frequency, 2);
        assert_eq!(p.success_rate, 1.0);
    }

    #[test]
    fn mine_ignores_single_occurrences() {
        let goal1 = Uuid::new_v4();
        let base = Utc::now();

        let entries = vec![
            entry(goal1, "filesystem.write", "exec.action", base),
            entry(
                goal1,
                "git.commit",
                "exec.action",
                base + chrono::Duration::seconds(1),
            ),
            entry(
                goal1,
                "",
                "goal.completed",
                base + chrono::Duration::seconds(2),
            ),
        ];

        let miner = PatternMiner::new();
        let patterns = miner.mine(&entries);

        // Single observation of [filesystem.write, git.commit] should be filtered.
        let two_gram = patterns.iter().find(|p| p.capabilities.len() == 2);
        assert!(two_gram.is_none(), "single occurrence should be filtered");
    }

    #[test]
    fn mine_computes_success_rate() {
        let goal1 = Uuid::new_v4();
        let goal2 = Uuid::new_v4();
        let goal3 = Uuid::new_v4();
        let base = Utc::now();

        let entries = vec![
            // Goal 1: write → commit → completed (success)
            entry(goal1, "fs.write", "exec.action", base),
            entry(
                goal1,
                "git.commit",
                "exec.action",
                base + chrono::Duration::seconds(1),
            ),
            entry(
                goal1,
                "",
                "goal.completed",
                base + chrono::Duration::seconds(2),
            ),
            // Goal 2: write → commit → completed (success)
            entry(
                goal2,
                "fs.write",
                "exec.action",
                base + chrono::Duration::seconds(10),
            ),
            entry(
                goal2,
                "git.commit",
                "exec.action",
                base + chrono::Duration::seconds(11),
            ),
            entry(
                goal2,
                "",
                "goal.completed",
                base + chrono::Duration::seconds(12),
            ),
            // Goal 3: write → commit (failure, no goal.completed)
            entry(
                goal3,
                "fs.write",
                "exec.action",
                base + chrono::Duration::seconds(20),
            ),
            entry(
                goal3,
                "git.commit",
                "exec.action",
                base + chrono::Duration::seconds(21),
            ),
        ];

        let miner = PatternMiner::new();
        let patterns = miner.mine(&entries);

        let p = patterns
            .iter()
            .find(|p| p.capabilities == vec!["fs.write", "git.commit"])
            .unwrap();
        assert_eq!(p.frequency, 3);
        // 2 out of 3 succeeded.
        assert!((p.success_rate - 2.0 / 3.0).abs() < 0.01);
    }

    #[test]
    fn mine_includes_artifact_and_git_event_kinds() {
        let goal1 = Uuid::new_v4();
        let goal2 = Uuid::new_v4();
        let base = Utc::now();

        // artifact.created and git.* events are included when they have
        // non-empty capabilities. Events with empty capabilities are
        // excluded (they don't represent semantic capabilities).
        let entries = vec![
            entry(goal1, "fs.write", "exec.action", base),
            entry(
                goal1,
                "artifact.write",
                "artifact.created",
                base + chrono::Duration::seconds(1),
            ),
            entry(
                goal1,
                "git.commit",
                "git.commit",
                base + chrono::Duration::seconds(2),
            ),
            entry(
                goal1,
                "",
                "goal.completed",
                base + chrono::Duration::seconds(3),
            ),
            entry(
                goal2,
                "fs.write",
                "exec.action",
                base + chrono::Duration::seconds(10),
            ),
            entry(
                goal2,
                "artifact.write",
                "artifact.created",
                base + chrono::Duration::seconds(11),
            ),
            entry(
                goal2,
                "git.commit",
                "git.commit",
                base + chrono::Duration::seconds(12),
            ),
            entry(
                goal2,
                "",
                "goal.completed",
                base + chrono::Duration::seconds(13),
            ),
        ];

        let miner = PatternMiner::new();
        let patterns = miner.mine(&entries);

        // Should find patterns that include artifact.created and git.* events.
        let cap_sequence: Vec<String> =
            patterns.iter().map(|p| p.capabilities.join(", ")).collect();
        assert!(
            patterns
                .iter()
                .any(|p| p.capabilities.contains(&"artifact.write".to_string())),
            "should find artifact.write in patterns: {cap_sequence:?}"
        );
        assert!(
            patterns
                .iter()
                .any(|p| p.capabilities.contains(&"git.commit".to_string())),
            "should find git.commit in patterns: {cap_sequence:?}"
        );
        // Empty-capability events (like goal.completed) should be excluded.
        assert!(
            !patterns
                .iter()
                .any(|p| p.capabilities.contains(&"".to_string())),
            "should not have empty capabilities in patterns"
        );
    }
}
