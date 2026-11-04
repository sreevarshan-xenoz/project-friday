//! Internal indexing layer for the Experience Layer.
//!
//! The indexer maintains two searchable structures over the append-only timeline:
//!
//! 1. **Chronological index** — goal_id → ordered entries, workflows, reflections, outcome.
//! 2. **Semantic index** — term-frequency vectors for fuzzy goal matching.
//!
//! The `ExperienceService` owns an `ExperienceIndexer`. When you later switch
//! from TF-IDF to embeddings, or from in-memory to Tantivy/vector search,
//! only the indexer changes.
//!
//! Indexes are rebuildable from the append-only timeline. Changing embeddings
//! later means regenerating indexes, not touching history.

use std::collections::{BTreeMap, HashMap, HashSet};

use chrono::{DateTime, Utc};

use super::{GoalOutcome, ReflectionRecord, TimelineEntry, Workflow};

// ---------------------------------------------------------------------------
// Chronological index
// ---------------------------------------------------------------------------

/// A snapshot of all data related to one goal attempt.
#[derive(Debug, Clone)]
pub struct GoalRecord {
    pub goal_id: GoalKey,
    pub title: String,
    pub entries: Vec<TimelineEntry>,
    pub workflows: Vec<Workflow>,
    pub reflections: Vec<ReflectionRecord>,
    pub outcome: GoalOutcome,
    pub last_activity: DateTime<Utc>,
}

/// Composite key for a goal: (goal_id, plan_id) — multiple plans can share
/// the same goal_id.
pub type GoalKey = (uuid::Uuid, uuid::Uuid);

// ---------------------------------------------------------------------------
// Semantic index
// ---------------------------------------------------------------------------

/// Tokenized representation of a goal for fuzzy matching.
#[derive(Debug, Clone)]
struct GoalVector {
    terms: HashMap<String, f64>,
}

impl GoalVector {
    fn from_text(text: &str) -> Self {
        let mut terms = HashMap::new();
        for token in tokenize(text) {
            *terms.entry(token).or_insert(0.0) += 1.0;
        }
        // Normalize by document length.
        let len = terms.len().max(1) as f64;
        for v in terms.values_mut() {
            *v /= len;
        }
        Self { terms }
    }

    fn cosine_similarity(&self, other: &GoalVector) -> f64 {
        let mut dot = 0.0;
        let mut norm_a = 0.0;
        let mut norm_b = 0.0;

        for (term, &a) in &self.terms {
            let b = other.terms.get(term).copied().unwrap_or(0.0);
            dot += a * b;
            norm_a += a * a;
        }
        for &b in other.terms.values() {
            norm_b += b * b;
        }

        let denom = (norm_a * norm_b).sqrt();
        if denom < 1e-10 {
            0.0
        } else {
            dot / denom
        }
    }
}

fn tokenize(text: &str) -> Vec<String> {
    text.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|s| s.len() > 2)
        .map(|s| s.to_string())
        .collect()
}

// ---------------------------------------------------------------------------
// Indexer
// ---------------------------------------------------------------------------

/// Maintains chronological and semantic indexes over the timeline.
///
/// The indexer is a pure derivation — it reads the timeline and produces
/// searchable structures. It never mutates the timeline.
pub struct ExperienceIndexer {
    /// goal_key → GoalRecord
    chronological: BTreeMap<GoalKey, GoalRecord>,
    /// goal_key → GoalVector (for semantic search)
    semantic: HashMap<GoalKey, GoalVector>,
    /// Inverted index: term → set of goal_keys that contain it
    inverted: HashMap<String, HashSet<GoalKey>>,
    /// Total documents for IDF calculation
    doc_count: usize,
}

impl ExperienceIndexer {
    pub fn new() -> Self {
        Self {
            chronological: BTreeMap::new(),
            semantic: HashMap::new(),
            inverted: HashMap::new(),
            doc_count: 0,
        }
    }

    /// Build indexes from a collection of timeline entries, workflows, and
    /// reflections. This is the "rebuild from timeline" path — indexes are
    /// always derivable from the append-only source of truth.
    pub fn build(
        &mut self,
        entries: &[TimelineEntry],
        workflows: &[Workflow],
        reflections: &[ReflectionRecord],
    ) {
        self.chronological.clear();
        self.semantic.clear();
        self.inverted.clear();

        // Group entries by goal_key.
        for entry in entries {
            let key = (entry.goal_id, entry.plan_id);
            let record = self.chronological.entry(key).or_insert_with(|| GoalRecord {
                goal_id: key,
                title: extract_title(&entry.metadata),
                entries: Vec::new(),
                workflows: Vec::new(),
                reflections: Vec::new(),
                outcome: GoalOutcome::Unknown,
                last_activity: entry.timestamp,
            });
            record.entries.push(entry.clone());
            if entry.timestamp > record.last_activity {
                record.last_activity = entry.timestamp;
            }
        }

        // Attach workflows.
        for workflow in workflows {
            let key = (workflow.source_goal_id, uuid::Uuid::nil());
            if let Some(record) = self.chronological.get_mut(&key) {
                record.workflows.push(workflow.clone());
            }
        }

        // Attach reflections.
        for reflection in reflections {
            // Find the goal this reflection belongs to by looking up the entry.
            let entry_id = reflection.timeline_entry_id;
            for entry in entries {
                if entry.id == entry_id {
                    let key = (entry.goal_id, entry.plan_id);
                    if let Some(record) = self.chronological.get_mut(&key) {
                        record.reflections.push(reflection.clone());
                    }
                    break;
                }
            }
        }

        // Derive outcomes from timeline entries.
        for record in self.chronological.values_mut() {
            record.outcome = derive_outcome(&record.entries);
        }

        // Build semantic index.
        self.doc_count = self.chronological.len();
        for (key, record) in &self.chronological {
            let mut text = record.title.clone();
            for entry in &record.entries {
                text.push(' ');
                text.push_str(&entry.capability);
            }
            for reflection in &record.reflections {
                text.push(' ');
                text.push_str(&reflection.insight);
            }
            let vector = GoalVector::from_text(&text);
            for term in vector.terms.keys() {
                self.inverted.entry(term.clone()).or_default().insert(*key);
            }
            self.semantic.insert(*key, vector);
        }
    }

    /// Find goals with similar titles/capabilities. Returns results sorted
    /// by cosine similarity descending.
    pub fn similar(&self, query: &str, max_results: usize) -> Vec<(GoalKey, f64)> {
        let query_vec = GoalVector::from_text(query);

        // Candidate retrieval via inverted index.
        let mut candidates: HashSet<GoalKey> = HashSet::new();
        for term in query_vec.terms.keys() {
            if let Some(keys) = self.inverted.get(term) {
                candidates.extend(keys);
            }
        }

        // Score candidates.
        let mut scored: Vec<(GoalKey, f64)> = candidates
            .iter()
            .filter_map(|key| {
                let goal_vec = self.semantic.get(key)?;
                let score = query_vec.cosine_similarity(goal_vec);
                Some((*key, score))
            })
            .filter(|(_, score)| *score > 0.0)
            .collect();

        scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        scored.truncate(max_results);
        scored
    }

    /// Get the chronological record for a specific goal+plan key.
    pub fn get_record_by_key(&self, key: GoalKey) -> Option<&GoalRecord> {
        self.chronological.get(&key)
    }

    /// Get the chronological record for a goal (any plan).
    /// When multiple plans exist for the same goal, returns the first match
    /// (by BTreeMap key order). Prefer `get_record_by_key` when the caller
    /// has the full GoalKey.
    pub fn get_record(&self, goal_id: uuid::Uuid) -> Option<&GoalRecord> {
        for (key, record) in &self.chronological {
            if key.0 == goal_id {
                return Some(record);
            }
        }
        None
    }

    /// Get the most recent attempt for a goal.
    pub fn last_attempt(&self, goal_id: uuid::Uuid) -> Option<&GoalRecord> {
        let mut candidates: Vec<&GoalRecord> = self
            .chronological
            .values()
            .filter(|r| r.goal_id.0 == goal_id)
            .collect();
        candidates.sort_by_key(|r| std::cmp::Reverse(r.last_activity));
        candidates.first().copied()
    }

    /// All goal records, sorted by last activity descending.
    pub fn all_records(&self) -> Vec<&GoalRecord> {
        let mut records: Vec<&GoalRecord> = self.chronological.values().collect();
        records.sort_by_key(|r| std::cmp::Reverse(r.last_activity));
        records
    }

    /// Compute text similarity between two strings (for external callers).
    pub fn text_similarity(a: &str, b: &str) -> f64 {
        let va = GoalVector::from_text(a);
        let vb = GoalVector::from_text(b);
        va.cosine_similarity(&vb)
    }

    /// Number of indexed goals.
    pub fn goal_count(&self) -> usize {
        self.chronological.len()
    }
}

impl Default for ExperienceIndexer {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Extract a human-readable title from metadata.
fn extract_title(metadata: &serde_json::Value) -> String {
    metadata
        .get("title")
        .or_else(|| metadata.get("goal"))
        .or_else(|| metadata.get("intent"))
        .and_then(|v| v.as_str())
        .unwrap_or("unknown")
        .to_string()
}

/// Derive the outcome of a goal from its timeline entries.
fn derive_outcome(entries: &[TimelineEntry]) -> GoalOutcome {
    let has_completed = entries.iter().any(|e| e.event_kind == "goal.completed");
    let has_failure = entries.iter().any(|e| {
        e.event_kind == "exec.result"
            && e.metadata.get("success").and_then(|v| v.as_bool()) == Some(false)
    });
    let has_permission_denied = entries.iter().any(|e| {
        e.event_kind == "exec.result"
            && e.metadata
                .get("error")
                .and_then(|v| v.as_str())
                .map(|s| s.contains("permission denied"))
                == Some(true)
    });
    let has_success = entries.iter().any(|e| {
        e.event_kind == "exec.result"
            && e.metadata.get("success").and_then(|v| v.as_bool()) == Some(true)
    });

    if has_completed {
        GoalOutcome::Success
    } else if has_permission_denied || has_failure {
        GoalOutcome::Failure
    } else if has_success {
        GoalOutcome::PartiallyCompleted
    } else {
        GoalOutcome::Unknown
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokenize_splits_on_non_alphanumeric() {
        let tokens = tokenize("filesystem.write_file");
        assert!(tokens.contains(&"filesystem".to_string()));
        assert!(tokens.contains(&"write".to_string()));
        assert!(tokens.contains(&"file".to_string()));
    }

    #[test]
    fn cosine_similarity_identical() {
        let a = GoalVector::from_text("filesystem write file");
        let b = GoalVector::from_text("filesystem write file");
        let sim = a.cosine_similarity(&b);
        assert!((sim - 1.0).abs() < 0.01, "expected ~1.0, got {sim}");
    }

    #[test]
    fn cosine_similarity_disjoint() {
        let a = GoalVector::from_text("filesystem write");
        let b = GoalVector::from_text("git commit branch");
        let sim = a.cosine_similarity(&b);
        assert!(sim < 0.01, "expected ~0.0, got {sim}");
    }

    #[test]
    fn cosine_similarity_partial_overlap() {
        let a = GoalVector::from_text("filesystem write file");
        let b = GoalVector::from_text("filesystem read file");
        let sim = a.cosine_similarity(&b);
        assert!(sim > 0.3, "expected >0.3, got {sim}");
    }

    #[test]
    fn derive_outcome_completed_is_success() {
        let entries = vec![TimelineEntry {
            version: 1,
            id: uuid::Uuid::new_v4(),
            timestamp: Utc::now(),
            goal_id: uuid::Uuid::new_v4(),
            plan_id: uuid::Uuid::new_v4(),
            execution_id: uuid::Uuid::new_v4(),
            event_kind: "goal.completed".into(),
            capability: "".into(),
            metadata: serde_json::json!({}),
            parent: None,
            tags: vec![],
            annotations: HashMap::new(),
        }];
        assert_eq!(derive_outcome(&entries), GoalOutcome::Success);
    }

    #[test]
    fn derive_outcome_failure() {
        let entries = vec![TimelineEntry {
            version: 1,
            id: uuid::Uuid::new_v4(),
            timestamp: Utc::now(),
            goal_id: uuid::Uuid::new_v4(),
            plan_id: uuid::Uuid::new_v4(),
            execution_id: uuid::Uuid::new_v4(),
            event_kind: "exec.result".into(),
            capability: "fs.write".into(),
            metadata: serde_json::json!({"success": false, "error": "file not found"}),
            parent: None,
            tags: vec![],
            annotations: HashMap::new(),
        }];
        assert_eq!(derive_outcome(&entries), GoalOutcome::Failure);
    }
}
