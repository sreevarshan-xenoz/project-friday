//! The production [`LessonStore`] trait and its in-memory implementation.
//!
//! Design invariants (8B):
//! * `find_similar` is strictly **read-only** — no pruning, no counters.
//! * `prune_stale` is an explicit maintenance operation.
//! * Contradiction handling is deterministic: the *policy* for detecting
//!   contradictions lives in the autonomy orchestrator; this crate only records
//!   the `Contradicted` status.

use std::collections::HashMap;
use std::sync::Mutex;

use crate::error::LessonStoreError;
use crate::types::{
    recompute_confidence, LessonKey, LessonStatus, MatchPolicy, ProductionLesson, PruneConfig,
};

const MS_PER_DAY: f64 = 86_400_000.0;

/// Production lesson store. Implemented by `InMemoryLessonStore` (hermetic) and, in
/// a later step, a `redb`-backed durable store reusing FRIDAY's embedded standard.
pub trait LessonStore: Send + Sync {
    /// Admit a new lesson. Errors if the key already exists (call `strengthen`).
    /// Confidence is normalized via the store's [`PruneConfig`] so the formula stays
    /// a single owned seam.
    fn admit(&self, lesson: ProductionLesson) -> Result<(), LessonStoreError>;

    /// Strengthen an existing lesson: bump support, refresh recency, recompute
    /// confidence. Returns the updated lesson, or `None` if the key is unknown.
    fn strengthen(
        &self,
        key: &LessonKey,
        now_ms: u64,
    ) -> Result<Option<ProductionLesson>, LessonStoreError>;

    /// Read-only retrieval of lessons matching `key` under `policy`. Never mutates.
    fn find_similar(
        &self,
        key: &LessonKey,
        policy: MatchPolicy,
    ) -> Result<Vec<ProductionLesson>, LessonStoreError>;

    /// Direct keyed read (read-only, returns regardless of status). For explicit
    /// inspection / status assertions; the planner consultation should prefer
    /// `find_similar` (which excludes `Archived`).
    fn get(&self, key: &LessonKey) -> Result<Option<ProductionLesson>, LessonStoreError>;

    /// Mark a lesson contradicted (and, if present, its counterpart). Deterministic.
    fn mark_contradicted(&self, key: &LessonKey, with: &LessonKey) -> Result<(), LessonStoreError>;

    /// Explicit maintenance: apply age/decay transitions. Returns the number of
    /// lessons whose status changed. Never called from `find_similar`.
    fn prune_stale(&self, now_ms: u64, config: &PruneConfig) -> Result<usize, LessonStoreError>;

    /// Record one verified-successful execution against an existing lesson
    /// (L1). Bumps the durable `verification_count`; a no-op when no lesson
    /// exists for the key — evidence attaches to claims, never banked for
    /// absent ones and never creating lessons by itself.
    fn record_verification(&self, key: &LessonKey) -> Result<u64, LessonStoreError>;
}

/// True when a stored lesson (given its key) matches a query key under `policy`.
pub(crate) fn signals_match(stored: &LessonKey, query: &LessonKey, policy: MatchPolicy) -> bool {
    if stored.action_class != query.action_class {
        return false;
    }
    match policy {
        MatchPolicy::Exact => stored.signals == query.signals,
        MatchPolicy::Subset => stored.signals.is_subset(&query.signals),
        MatchPolicy::Jaccard(t) => {
            if stored.signals.is_empty() || query.signals.is_empty() {
                return false;
            }
            let inter = stored.signals.intersection(&query.signals).count();
            let union = stored.signals.union(&query.signals).count();
            (inter as f64 / union as f64) >= t
        }
    }
}

/// Hermetic in-memory `LessonStore`. Mirrors the 7C admission/learning semantics
/// for tests and as the reference adapter until the durable backend lands.
pub struct InMemoryLessonStore {
    config: PruneConfig,
    lessons: Mutex<HashMap<LessonKey, ProductionLesson>>,
    /// Verification evidence banked before the condition's lesson exists
    /// (L1); folded into the row on admission.
    pending_verifications: Mutex<HashMap<LessonKey, u64>>,
}

impl InMemoryLessonStore {
    pub fn new() -> Self {
        Self::with_config(PruneConfig::default())
    }

    pub fn with_config(config: PruneConfig) -> Self {
        Self {
            config,
            lessons: Mutex::new(HashMap::new()),
            pending_verifications: Mutex::new(HashMap::new()),
        }
    }

    pub fn len(&self) -> usize {
        self.lessons.lock().expect("lesson store poisoned").len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl Default for InMemoryLessonStore {
    fn default() -> Self {
        Self::new()
    }
}

impl LessonStore for InMemoryLessonStore {
    fn admit(&self, mut lesson: ProductionLesson) -> Result<(), LessonStoreError> {
        let mut lessons = self.lessons.lock().expect("lesson store poisoned");
        if lessons.contains_key(&lesson.key) {
            return Err(LessonStoreError::AlreadyExists(lesson.key.clone()));
        }
        let age_days = (lesson
            .last_observed_ms
            .saturating_sub(lesson.first_observed_ms)) as f64
            / MS_PER_DAY;
        lesson.confidence = recompute_confidence(lesson.support_count, age_days, &self.config);
        lesson.status = LessonStatus::Active;
        // L1 — fold banked pre-admission verification evidence into the row.
        if let Some(v) = self
            .pending_verifications
            .lock()
            .expect("lesson store poisoned")
            .remove(&lesson.key)
        {
            lesson.verification_count += v;
        }
        lessons.insert(lesson.key.clone(), lesson);
        Ok(())
    }

    fn strengthen(
        &self,
        key: &LessonKey,
        now_ms: u64,
    ) -> Result<Option<ProductionLesson>, LessonStoreError> {
        let mut lessons = self.lessons.lock().expect("lesson store poisoned");
        match lessons.get_mut(key) {
            None => Ok(None),
            Some(l) => {
                l.support_count += 1;
                l.last_observed_ms = now_ms;
                let age_days = (now_ms.saturating_sub(l.first_observed_ms)) as f64 / MS_PER_DAY;
                l.confidence = recompute_confidence(l.support_count, age_days, &self.config);
                l.status = LessonStatus::Active; // a strengthen refreshes staleness
                Ok(Some(l.clone()))
            }
        }
    }

    fn find_similar(
        &self,
        key: &LessonKey,
        policy: MatchPolicy,
    ) -> Result<Vec<ProductionLesson>, LessonStoreError> {
        let lessons = self.lessons.lock().expect("lesson store poisoned");
        let out: Vec<ProductionLesson> = lessons
            .values()
            .filter(|l| l.status != LessonStatus::Archived)
            .filter(|l| signals_match(&l.key, key, policy))
            .cloned()
            .collect();
        Ok(out)
    }

    fn get(&self, key: &LessonKey) -> Result<Option<ProductionLesson>, LessonStoreError> {
        let lessons = self.lessons.lock().expect("lesson store poisoned");
        Ok(lessons.get(key).cloned())
    }

    fn mark_contradicted(&self, key: &LessonKey, with: &LessonKey) -> Result<(), LessonStoreError> {
        let mut lessons = self.lessons.lock().expect("lesson store poisoned");
        for k in [key, with] {
            if let Some(l) = lessons.get_mut(k) {
                l.status = LessonStatus::Contradicted;
            }
        }
        Ok(())
    }

    fn prune_stale(&self, now_ms: u64, config: &PruneConfig) -> Result<usize, LessonStoreError> {
        let mut lessons = self.lessons.lock().expect("lesson store poisoned");
        let mut changed = 0;
        for l in lessons.values_mut() {
            if l.status == LessonStatus::Contradicted {
                continue;
            }
            let age_days = (now_ms.saturating_sub(l.last_observed_ms)) as f64 / MS_PER_DAY;
            let conf = recompute_confidence(l.support_count, age_days, config);
            l.confidence = conf;
            if conf < config.confidence_floor {
                if l.status != LessonStatus::Archived {
                    l.status = LessonStatus::Archived;
                    changed += 1;
                }
            } else if now_ms.saturating_sub(l.last_observed_ms) > config.ttl_ms
                && l.status != LessonStatus::Stale
            {
                l.status = LessonStatus::Stale;
                changed += 1;
            }
        }
        Ok(changed)
    }

    fn record_verification(&self, key: &LessonKey) -> Result<u64, LessonStoreError> {
        let mut lessons = self.lessons.lock().expect("lesson store poisoned");
        match lessons.get_mut(key) {
            Some(l) => {
                l.verification_count += 1;
                Ok(l.verification_count)
            }
            // Claim not admitted yet: bank in-memory; folded at admission.
            None => {
                let mut pending = self
                    .pending_verifications
                    .lock()
                    .expect("lesson store poisoned");
                let c = pending.entry(key.clone()).or_insert(0);
                *c += 1;
                Ok(*c)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{build_lesson, LessonProvenance};

    fn prov(raw: &str) -> LessonProvenance {
        LessonProvenance {
            action_raw: raw.to_string(),
            trace_id: None,
            sample_signals: vec![],
        }
    }

    fn lesson(key: LessonKey, support: u64, first: u64, last: u64) -> ProductionLesson {
        let claim = format!("prediction for {:?} unreliable", key.signals);
        build_lesson(
            key,
            claim,
            support,
            first,
            last,
            prov("act"),
            &PruneConfig::default(),
        )
    }

    // Subset asymmetry: a general stored lesson applies to a specific observation,
    // but a specific stored lesson does NOT apply to a general observation.
    #[test]
    fn subset_match_is_asymmetric() {
        let store = InMemoryLessonStore::new();
        let general = LessonKey::new("browser.search", ["network_unavailable"]);
        store.admit(lesson(general.clone(), 1, 1000, 1000)).unwrap();

        // stored {network_unavailable} ⊆ query {network_unavailable, browser_restart}
        let specific_q =
            LessonKey::new("browser.search", ["network_unavailable", "browser_restart"]);
        let hits = store
            .find_similar(&specific_q, MatchPolicy::Subset)
            .unwrap();
        assert_eq!(hits.len(), 1);

        // Reverse: stored {network_unavailable, browser_restart} is NOT ⊆ query.
        let specific = LessonKey::new("browser.search", ["network_unavailable", "browser_restart"]);
        store
            .admit(lesson(specific.clone(), 1, 1000, 1000))
            .unwrap();
        let general_q = LessonKey::new("browser.search", ["network_unavailable"]);
        let hits = store.find_similar(&general_q, MatchPolicy::Subset).unwrap();
        assert_eq!(hits.len(), 1, "only the general lesson matches");
    }

    #[test]
    fn strengthen_grows_support_and_is_idempotent_on_admit() {
        let store = InMemoryLessonStore::new();
        let key = LessonKey::new("git.commit", ["missing_repo"]);
        store.admit(lesson(key.clone(), 1, 1000, 1000)).unwrap();
        // Duplicate admit is rejected.
        assert!(store.admit(lesson(key.clone(), 1, 1000, 1000)).is_err());

        let s1 = store.strengthen(&key, 2000).unwrap().unwrap();
        let s2 = store.strengthen(&key, 3000).unwrap().unwrap();
        assert_eq!(s1.support_count, 2);
        assert_eq!(s2.support_count, 3);
    }

    #[test]
    fn strengthen_unknown_returns_none() {
        let store = InMemoryLessonStore::new();
        let key = LessonKey::new("x", ["y"]);
        assert!(store.strengthen(&key, 1000).unwrap().is_none());
    }

    #[test]
    fn find_similar_is_read_only() {
        let store = InMemoryLessonStore::new();
        let key = LessonKey::new("a", ["b"]);
        store.admit(lesson(key.clone(), 1, 1000, 1000)).unwrap();
        // Many reads must not change state (no pruning/counters).
        for _ in 0..5 {
            let hits = store.find_similar(&key, MatchPolicy::Exact).unwrap();
            assert_eq!(hits.len(), 1);
        }
        assert_eq!(store.len(), 1);
    }

    #[test]
    fn contradiction_marks_status_but_does_not_delete() {
        let store = InMemoryLessonStore::new();
        let k1 = LessonKey::new("a", ["b", "c"]);
        let k2 = LessonKey::new("a", ["b", "d"]);
        store.admit(lesson(k1.clone(), 1, 1000, 1000)).unwrap();
        store.admit(lesson(k2.clone(), 1, 1000, 1000)).unwrap();

        store.mark_contradicted(&k1, &k2).unwrap();
        let hits = store.find_similar(&k1, MatchPolicy::Subset).unwrap();
        assert_eq!(hits.len(), 1);
        assert!(!hits[0].is_usable(), "contradicted lesson is not usable");
        assert_eq!(store.len(), 2, "nothing deleted");
    }

    #[test]
    fn prune_marks_stale_and_archived() {
        let cfg = PruneConfig {
            ttl_ms: 1000, // 1s TTL for the test
            confidence_floor: 0.5,
            ..PruneConfig::default()
        };
        let store = InMemoryLessonStore::with_config(cfg.clone());

        // Old, low-support → should archive (confidence < floor).
        let archived = LessonKey::new("old", ["x"]);
        store.admit(lesson(archived.clone(), 1, 0, 0)).unwrap();
        // Old but high support (>= implied by many strengthens) → stale only.
        let stale = LessonKey::new("stale", ["y"]);
        store.admit(lesson(stale.clone(), 5, 0, 0)).unwrap();
        for _ in 0..10 {
            store.strengthen(&stale, 0).unwrap();
        }

        // 10 days of age: decay pushes support=1 lesson (0.55) below 0.5 → Archived;
        // the high-support lesson stays above the floor but exceeds the 1s TTL → Stale.
        let changed = store.prune_stale(864_000_000, &cfg).unwrap();
        assert_eq!(changed, 2);

        let archived_l = store
            .get(&archived)
            .unwrap()
            .expect("archived lesson present");
        assert_eq!(archived_l.status, LessonStatus::Archived);
        let stale_l = store.get(&stale).unwrap().expect("stale lesson present");
        assert_eq!(stale_l.status, LessonStatus::Stale);
    }

    #[test]
    fn jaccard_threshold_filters() {
        let store = InMemoryLessonStore::new();
        let k = LessonKey::new("a", ["b", "c", "d"]);
        store.admit(lesson(k.clone(), 1, 1000, 1000)).unwrap();

        // query shares 1 of 3 → Jaccard 0.33; threshold 0.5 ⇒ no match.
        let q_low = LessonKey::new("a", ["b", "e", "f"]);
        assert_eq!(
            store
                .find_similar(&q_low, MatchPolicy::Jaccard(0.5))
                .unwrap()
                .len(),
            0
        );
        // query shares 2 of 3 → Jaccard 0.5 ⇒ match at threshold 0.5.
        let q_hi = LessonKey::new("a", ["b", "c", "e"]);
        assert_eq!(
            store
                .find_similar(&q_hi, MatchPolicy::Jaccard(0.5))
                .unwrap()
                .len(),
            1
        );
    }
}
