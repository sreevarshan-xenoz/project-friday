//! 8B step 5: production `MemoryStore` adapter.
//!
//! Binds the hermetic autonomy [`MemoryStore`](crate::memory::MemoryStore) trait
//! (7C) to the real `friday-lesson-store` [`LessonStore`]. The `action` string in
//! a 7C [`LessonCondition`] is mapped to a production `action_class` via the
//! `CapabilityResolver`; if it cannot be resolved, the lesson is dropped rather
//! than inventing a class (phase-8 locked decision).

use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use friday_lesson_store::{
    build_lesson, CapabilityResolver, LessonKey, LessonProvenance, LessonStore, MatchPolicy,
    PruneConfig, ProductionLesson,
};

use crate::memory::{Experience, Lesson, LessonCondition, MemoryStore};

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Translate a 7C condition into a production key. Returns `None` when the
/// `action` cannot be resolved to a capability class (no fabrication).
fn to_key(
    condition: &LessonCondition,
    resolver: &dyn CapabilityResolver,
) -> Option<LessonKey> {
    let action_class = resolver.resolve(&condition.action)?;
    Some(LessonKey::new(
        action_class,
        condition.signals.iter().cloned(),
    ))
}

/// Translate a production lesson back into the 7C shape, recovering the raw
/// action from provenance.
fn from_lesson(pl: ProductionLesson) -> Lesson {
    Lesson {
        condition: LessonCondition {
            action: pl.provenance.action_raw,
            signals: pl.key.signals.iter().cloned().collect(),
        },
        claim: pl.claim,
        confidence: pl.confidence,
        support_count: pl.support_count,
        first_observed_ms: pl.first_observed_ms,
        last_observed_ms: pl.last_observed_ms,
    }
}

/// Production memory adapter: implements the autonomous [`MemoryStore`] over the
/// real `friday-lesson-store` [`LessonStore`]. Pluggable into the orchestrator
/// via `with_memory_store(Box::new(adapter))`.
pub struct ProductionLessonStore {
    inner: Arc<dyn LessonStore>,
    resolver: Arc<dyn CapabilityResolver>,
    config: PruneConfig,
}

impl ProductionLessonStore {
    pub fn new(
        inner: Arc<dyn LessonStore>,
        resolver: Arc<dyn CapabilityResolver>,
        config: PruneConfig,
    ) -> Self {
        Self {
            inner,
            resolver,
            config,
        }
    }
}

impl MemoryStore for ProductionLessonStore {
    fn record_experience(&self, _experience: &Experience) {
        // Telemetry only. Raw episode persistence is friday-memory's job; lesson
        // admission here is driven by `admit_lesson` / `strengthen_lesson` (the
        // ExperienceExtractor owns corroboration buffering in its own state).
    }

    fn find_similar_lesson(&self, condition: &LessonCondition) -> Option<Lesson> {
        let key = to_key(condition, &*self.resolver)?;
        let hits = self.inner.find_similar(&key, MatchPolicy::Exact).ok()?;
        hits.into_iter().next().map(from_lesson)
    }

    fn admit_lesson(&self, lesson: Lesson) {
        if let Some(key) = to_key(&lesson.condition, &*self.resolver) {
            let provenance = LessonProvenance {
                action_raw: lesson.condition.action.clone(),
                trace_id: None,
                sample_signals: lesson.condition.signals.clone(),
            };
            let pl = build_lesson(
                key,
                lesson.claim.clone(),
                lesson.support_count,
                lesson.first_observed_ms,
                lesson.last_observed_ms,
                provenance,
                &self.config,
            );
            let _ = self.inner.admit(pl);
        }
    }

    fn strengthen_lesson(&self, condition: &LessonCondition) -> Option<Lesson> {
        let key = to_key(condition, &*self.resolver)?;
        let updated = self.inner.strengthen(&key, now_ms()).ok().flatten()?;
        Some(from_lesson(updated))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::{Experience, ExperienceExtractor};
    use crate::world::{PredictionOutcome, WorldSnapshot, WorldStateDiff};
    use friday_lesson_store::{IdentityResolver, InMemoryLessonStore, RegistryCapabilityResolver};
    use friday_capabilities::CapabilityRegistry;

    fn lesson_store(resolver: Arc<dyn CapabilityResolver>) -> ProductionLessonStore {
        ProductionLessonStore::new(
            Arc::new(InMemoryLessonStore::new()),
            resolver,
            PruneConfig::default(),
        )
    }

    /// Build a mismatch experience whose diverging signals are `signals`.
    fn mismatch_exp(action: &str, signals: &[&str], at: u64) -> Experience {
        let diff = WorldStateDiff {
            added: signals.iter().map(|s| s.to_string()).collect(),
            removed: vec![],
            changed: vec![],
        };
        let mut predicted = WorldSnapshot::new();
        for s in signals {
            predicted
                .variables
                .insert(s.to_string(), serde_json::json!(null));
        }
        Experience {
            action: action.to_string(),
            predicted,
            actual: None,
            prediction_error: Some(diff.clone()),
            assessment: PredictionOutcome::Mismatch { diff },
            observed_at_ms: at,
        }
    }

    #[test]
    fn admits_and_finds_via_production_store() {
        let adapter = lesson_store(Arc::new(IdentityResolver));
        let extractor = ExperienceExtractor::default();
        // High-surprise severity (1 key < 5) ⇒ accumulate; push to corroboration.
        // Use a high-surprise action by forcing 5 signals so it admits immediately.
        let big = mismatch_exp(
            "git.commit",
            &["a", "b", "c", "d", "e"],
            1000,
        );
        extractor.process(&adapter, &big);
        let cond = ExperienceExtractor::condition_of(&big);
        let found = adapter.find_similar_lesson(&cond);
        assert!(found.is_some(), "lesson should be admitted and findable");
        let l = found.unwrap();
        assert_eq!(l.condition.action, "git.commit");
        assert_eq!(l.support_count, 1);
    }

    #[test]
    fn unresolved_action_is_dropped_not_fabricated() {
        // Empty registry resolver ⇒ resolve() returns None ⇒ no admission.
        let registry = Arc::new(CapabilityRegistry::new());
        let adapter = lesson_store(Arc::new(RegistryCapabilityResolver::new(registry)));
        let big = mismatch_exp("unknown.thing", &["a", "b", "c", "d", "e"], 1000);
        let extractor = ExperienceExtractor::default();
        extractor.process(&adapter, &big);
        let cond = ExperienceExtractor::condition_of(&big);
        assert!(
            adapter.find_similar_lesson(&cond).is_none(),
            "unresolvable action must not produce a lesson"
        );
    }

    #[test]
    fn strengthen_increments_support_through_adapter() {
        let adapter = lesson_store(Arc::new(IdentityResolver));
        let big = mismatch_exp("git.commit", &["a", "b", "c", "d", "e"], 1000);
        let extractor = ExperienceExtractor::default();
        extractor.process(&adapter, &big);
        let cond = ExperienceExtractor::condition_of(&big);
        let before = adapter.find_similar_lesson(&cond).unwrap();
        assert_eq!(before.support_count, 1);
        let after = adapter.strengthen_lesson(&cond);
        assert!(after.is_some());
        assert_eq!(after.unwrap().support_count, 2);
    }

    #[test]
    fn corroboration_path_admits_after_threshold() {
        // Moderate error (1 signal < high_surprise_severity=5) ⇒ accumulate 3×
        // then admit. Exercises the full ExperienceExtractor → adapter path.
        let adapter = lesson_store(Arc::new(IdentityResolver));
        let extractor = ExperienceExtractor::new(5, 3);
        let moderate = mismatch_exp("git.commit", &["dirty_tree"], 1000);
        extractor.process(&adapter, &moderate);
        assert!(adapter.find_similar_lesson(&ExperienceExtractor::condition_of(&moderate)).is_none());
        extractor.process(&adapter, &moderate);
        assert!(adapter.find_similar_lesson(&ExperienceExtractor::condition_of(&moderate)).is_none());
        extractor.process(&adapter, &moderate);
        let found = adapter.find_similar_lesson(&ExperienceExtractor::condition_of(&moderate));
        assert!(found.is_some(), "3rd corroborating error must admit a lesson");
        assert_eq!(found.unwrap().support_count, 3);
    }
}
