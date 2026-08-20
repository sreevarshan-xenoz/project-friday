//! 7C memory writeback — hermetic, selective experience -> lesson pipeline.
//!
//! Mirrors the existing FRIDAY memory concepts (friday-memory / friday-knowledge)
//! in a dependency-light form so the controlled tick can learn from one experience
//! without wiring a real memory backend. The real engines implement [MemoryStore]
//! later, exactly like the [crate::world::WorldModel] seam.
//!
//! Pipeline (per the 7C design):
//!
//! Episode evidence -> Experience -> ExperienceExtractor -> MemoryStore
//!                                    (admission policy)     (admit/strengthen)
//!
//! Admission policy:
//! * Correct / low-surprise -> no long-term memory (just telemetry).
//! * Moderate error -> accumulate evidence (corroboration).
//! * Repeated error -> admit / strengthen a Lesson.
//! * High-surprise error -> immediate Lesson candidate (escape hatch).
//! * Unknown reality -> no lesson; FRIDAY does not fabricate certainty.
//!
//! A Lesson is raw learning, NOT a skill — skill compounding is 7D.

use crate::world::{PredictionOutcome, WorldSnapshot, WorldStateDiff};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::sync::Mutex;

/// The concrete, serializable condition a lesson is keyed on. A real memory system
/// can map this onto its own query shape later; we keep it explicit (no unconstrained
/// generic C) so admission matching stays deterministic and inspectable.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct LessonCondition {
    /// The action/goal the prediction was about.
    pub action: String,
    /// The world-state signals that diverged (variable/flag keys from the diff).
    pub signals: Vec<String>,
}

/// A generalizable lesson — the useful unit of memory, distinct from a raw
/// Experience. For example: "prediction for X is unreliable when signals S diverge".
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Lesson {
    pub condition: LessonCondition,
    /// Human-readable, generalizable claim derived from the prediction error.
    pub claim: String,
    /// Belief strength in the lesson (rises with corroboration).
    pub confidence: f32,
    /// How many corroborating experiences back this lesson.
    pub support_count: u64,
    pub first_observed_ms: u64,
    pub last_observed_ms: u64,
}

/// One tick's raw episode evidence. Telemetry, not memory.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Experience {
    pub action: String,
    pub predicted: WorldSnapshot,
    pub actual: Option<WorldSnapshot>,
    /// None when the assessment was Correct (no diff) or Unknown (no diff known).
    pub prediction_error: Option<WorldStateDiff>,
    pub assessment: PredictionOutcome,
    pub observed_at_ms: u64,
}

/// Severity of a prediction error = number of diverging world-state keys.
fn severity(diff: &WorldStateDiff) -> usize {
    diff.added.len() + diff.removed.len() + diff.changed.len()
}

/// The keys that diverged between predicted and actual, used as lesson signals.
fn diverging_keys(diff: &WorldStateDiff) -> Vec<String> {
    let mut keys: HashSet<String> = HashSet::new();
    keys.extend(diff.added.iter().cloned());
    keys.extend(diff.removed.iter().cloned());
    keys.extend(diff.changed.iter().cloned());
    let mut v: Vec<String> = keys.into_iter().collect();
    v.sort();
    v
}

/// The production seam for memory. SimpleMemory is the test/fake; the real
/// friday-memory engine implements this trait later.
pub trait MemoryStore: Send + Sync {
    /// Persist raw episode evidence (telemetry). Always called.
    fn record_experience(&self, experience: &Experience);
    /// Find an already-admitted lesson matching a condition, if any.
    fn find_similar_lesson(&self, condition: &LessonCondition) -> Option<Lesson>;
    /// Admit a new lesson (corroborated or high-surprise).
    fn admit_lesson(&self, lesson: Lesson);
    /// Strengthen an existing lesson (bump support + refresh recency).
    fn strengthen_lesson(&self, condition: &LessonCondition) -> Option<Lesson>;
}

/// The admission decision for a single experience (internal to the extractor).
enum Admission {
    /// Correct or unknown — never a lesson.
    Discard,
    /// Moderate error not yet corroborated — accumulate, wait.
    Accumulate,
    /// High-surprise or just-corroborated — admit a fresh lesson.
    AdmitNew,
}

/// Decides which experiences become lessons, and how. Holds the corroboration
/// buffer (pending moderate errors) so repetition across ticks is tracked. Uses
/// interior mutability so the tick can stay &self and bounded.
pub struct ExperienceExtractor {
    /// Severity at or above this (on the first occurrence) admits a lesson immediately.
    pub high_surprise_severity: usize,
    /// Number of accumulated moderate errors needed before a lesson is admitted.
    pub corroboration_count: u64,
    pending: Mutex<HashMap<LessonCondition, u64>>,
}

impl Default for ExperienceExtractor {
    fn default() -> Self {
        Self {
            // Severity >= 5 distinct diverging keys => interesting enough to remember now.
            high_surprise_severity: 5,
            // Three corroborating moderate errors => a lesson is worth keeping.
            corroboration_count: 3,
            pending: Mutex::new(HashMap::new()),
        }
    }
}

impl ExperienceExtractor {
    pub fn new(high_surprise_severity: usize, corroboration_count: u64) -> Self {
        Self {
            high_surprise_severity,
            corroboration_count,
            pending: Mutex::new(HashMap::new()),
        }
    }

    /// Build the lesson condition from a mismatch experience.
    pub fn condition_of(experience: &Experience) -> LessonCondition {
        let signals = match &experience.prediction_error {
            Some(diff) => diverging_keys(diff),
            None => Vec::new(),
        };
        LessonCondition {
            action: experience.action.clone(),
            signals,
        }
    }

    fn confidence_for(count: u64) -> f32 {
        (0.5 + (count as f32).min(10.0) * 0.05).min(0.99)
    }

    fn build_lesson(condition: &LessonCondition, count: u64, observed_at_ms: u64) -> Lesson {
        let claim = format!(
            "prediction for '{}' unreliable when signals {:?} diverge",
            condition.action, condition.signals
        );
        Lesson {
            condition: condition.clone(),
            claim,
            confidence: Self::confidence_for(count),
            support_count: count,
            first_observed_ms: observed_at_ms,
            last_observed_ms: observed_at_ms,
        }
    }

    /// Classify an experience into an admission decision.
    fn classify(&self, experience: &Experience) -> Admission {
        match &experience.assessment {
            PredictionOutcome::Correct | PredictionOutcome::Unknown => Admission::Discard,
            PredictionOutcome::Mismatch { diff } => {
                let sev = severity(diff);
                if sev >= self.high_surprise_severity {
                    Admission::AdmitNew
                } else {
                    Admission::Accumulate
                }
            }
        }
    }

    /// Run one experience through the admission policy and update the store.
    /// Always records the experience (telemetry); only conditionally admits/strengthens.
    pub fn process(&self, store: &dyn MemoryStore, experience: &Experience) {
        store.record_experience(experience);

        let decision = self.classify(experience);
        let condition = Self::condition_of(experience);

        match decision {
            // Correct / unknown -> no lesson, ever.
            Admission::Discard => {}
            // High-surprise on first sight -> admit immediately.
            Admission::AdmitNew => {
                store.admit_lesson(Self::build_lesson(&condition, 1, experience.observed_at_ms));
                self.pending
                    .lock()
                    .expect("extractor poisoned")
                    .remove(&condition);
            }
            // Moderate -> accumulate, admit once corroborated.
            Admission::Accumulate => {
                if store.find_similar_lesson(&condition).is_some() {
                    store.strengthen_lesson(&condition);
                    return;
                }
                let count = {
                    let mut pending = self.pending.lock().expect("extractor poisoned");
                    let e = pending.entry(condition.clone()).or_insert(0);
                    *e += 1;
                    *e
                };
                if count >= self.corroboration_count {
                    store.admit_lesson(Self::build_lesson(
                        &condition,
                        count,
                        experience.observed_at_ms,
                    ));
                    self.pending
                        .lock()
                        .expect("extractor poisoned")
                        .remove(&condition);
                }
            }
        }
    }
}

/// Hermetic in-memory MemoryStore used by 7C tests and as the reference adapter.
pub struct SimpleMemory {
    experiences: Mutex<Vec<Experience>>,
    lessons: Mutex<HashMap<LessonCondition, Lesson>>,
}

impl SimpleMemory {
    pub fn new() -> Self {
        Self {
            experiences: Mutex::new(Vec::new()),
            lessons: Mutex::new(HashMap::new()),
        }
    }

    /// All recorded experiences (telemetry inspection).
    pub fn experiences(&self) -> Vec<Experience> {
        self.experiences.lock().expect("memory poisoned").clone()
    }

    /// All admitted lessons.
    pub fn lessons(&self) -> Vec<Lesson> {
        self.lessons
            .lock()
            .expect("memory poisoned")
            .values()
            .cloned()
            .collect()
    }

    /// Number of admitted lessons (dedup check).
    pub fn lesson_count(&self) -> usize {
        self.lessons.lock().expect("memory poisoned").len()
    }
}

impl Default for SimpleMemory {
    fn default() -> Self {
        Self::new()
    }
}

impl MemoryStore for SimpleMemory {
    fn record_experience(&self, experience: &Experience) {
        self.experiences
            .lock()
            .expect("memory poisoned")
            .push(experience.clone());
    }

    fn find_similar_lesson(&self, condition: &LessonCondition) -> Option<Lesson> {
        self.lessons
            .lock()
            .expect("memory poisoned")
            .get(condition)
            .cloned()
    }

    fn admit_lesson(&self, lesson: Lesson) {
        self.lessons
            .lock()
            .expect("memory poisoned")
            .insert(lesson.condition.clone(), lesson);
    }

    fn strengthen_lesson(&self, condition: &LessonCondition) -> Option<Lesson> {
        let mut lessons = self.lessons.lock().expect("memory poisoned");
        if let Some(l) = lessons.get_mut(condition) {
            l.support_count += 1;
            l.last_observed_ms = chrono::Utc::now().timestamp_millis() as u64;
            Some(l.clone())
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::{SimpleWorldModel, WorldModel};
    use serde_json::json;

    fn snapshot(values: &[(&str, i64)]) -> WorldSnapshot {
        let mut s = WorldSnapshot::new();
        for (k, v) in values {
            s = s.with_variable(*k, json!(v));
        }
        s
    }

    fn mismatch_exp(action: &str, predicted: WorldSnapshot, actual: WorldSnapshot) -> Experience {
        let diff = predicted.diff(&actual);
        Experience {
            action: action.into(),
            predicted,
            actual: Some(actual),
            prediction_error: Some(diff.clone()),
            assessment: PredictionOutcome::Mismatch { diff },
            observed_at_ms: 1_000,
        }
    }

    // 7C.1: correct prediction -> experience, no lesson
    #[test]
    fn correct_prediction_records_experience_no_lesson() {
        let store = SimpleMemory::new();
        let ext = ExperienceExtractor::default();
        let p = snapshot(&[("x", 1)]);
        let e = Experience {
            action: "act".into(),
            predicted: p.clone(),
            actual: Some(p),
            prediction_error: None,
            assessment: PredictionOutcome::Correct,
            observed_at_ms: 1,
        };
        ext.process(&store, &e);

        assert_eq!(store.experiences().len(), 1);
        assert_eq!(store.lesson_count(), 0);
    }

    // 7C.2: single moderate error -> experience, no lesson
    #[test]
    fn single_moderate_error_accumulates_no_lesson() {
        let store = SimpleMemory::new();
        let ext = ExperienceExtractor::default();
        let e = mismatch_exp("act", snapshot(&[("x", 1)]), snapshot(&[("x", 2)]));
        assert_eq!(severity(e.prediction_error.as_ref().unwrap()), 1);

        ext.process(&store, &e);

        assert_eq!(store.experiences().len(), 1);
        assert_eq!(store.lesson_count(), 0);
    }

    // 7C.3: repeated equivalent error -> lesson admitted, support grows
    #[test]
    fn repeated_equivalent_error_admits_lesson() {
        let store = SimpleMemory::new();
        let ext = ExperienceExtractor::default();
        let e = mismatch_exp("act", snapshot(&[("x", 1)]), snapshot(&[("x", 2)]));

        ext.process(&store, &e);
        ext.process(&store, &e);
        ext.process(&store, &e);

        assert_eq!(store.lesson_count(), 1);
        let lessons = store.lessons();
        assert_eq!(lessons[0].support_count, 3);
        assert_eq!(lessons[0].condition.signals, vec!["x".to_string()]);
    }

    // 7C.4: high-surprise error -> immediate lesson
    #[test]
    fn high_surprise_error_admits_immediately() {
        let store = SimpleMemory::new();
        let ext = ExperienceExtractor::default();
        let predicted = snapshot(&[("a", 1), ("b", 1), ("c", 1), ("d", 1), ("e", 1)]);
        let actual = snapshot(&[("a", 9), ("b", 9), ("c", 9), ("d", 9), ("e", 9)]);
        let e = mismatch_exp("act", predicted, actual);
        assert_eq!(severity(e.prediction_error.as_ref().unwrap()), 5);

        ext.process(&store, &e);

        assert_eq!(store.lesson_count(), 1);
        assert_eq!(store.lessons()[0].support_count, 1);
    }

    // 7C.5: unknown reality -> uncertain experience, no lesson
    #[test]
    fn unknown_reality_no_lesson() {
        let store = SimpleMemory::new();
        let ext = ExperienceExtractor::default();
        let predicted = snapshot(&[("x", 1)]);
        let e = Experience {
            action: "act".into(),
            predicted,
            actual: None,
            prediction_error: None,
            assessment: PredictionOutcome::Unknown,
            observed_at_ms: 1,
        };
        ext.process(&store, &e);

        assert_eq!(store.experiences().len(), 1);
        assert_eq!(store.lesson_count(), 0);
    }

    // 7C.6: lesson strengthening, no duplicate lessons
    #[test]
    fn lesson_strengthens_not_duplicates() {
        let store = SimpleMemory::new();
        let ext = ExperienceExtractor::default();
        let e = mismatch_exp("act", snapshot(&[("x", 1)]), snapshot(&[("x", 2)]));

        ext.process(&store, &e);
        ext.process(&store, &e);
        ext.process(&store, &e);
        ext.process(&store, &e);

        assert_eq!(store.lesson_count(), 1, "still a single lesson");
        assert_eq!(store.lessons()[0].support_count, 4);
    }

    // Sanity: SimpleWorldModel shapes feed ExperienceExtractor cleanly
    #[test]
    fn world_model_shapes_drive_extractor() {
        let wm = SimpleWorldModel::new(snapshot(&[("v", 1)]), Some(snapshot(&[("v", 1)])));
        let predicted = wm.predict("goal");
        let actual = wm.observe();
        let assessment = wm.update(&predicted, actual.clone());
        assert_eq!(assessment, PredictionOutcome::Correct);

        let store = SimpleMemory::new();
        let ext = ExperienceExtractor::default();
        let e = Experience {
            action: "goal".into(),
            predicted,
            actual,
            prediction_error: None,
            assessment,
            observed_at_ms: 1,
        };
        ext.process(&store, &e);
        assert_eq!(store.lesson_count(), 0);
    }
}
