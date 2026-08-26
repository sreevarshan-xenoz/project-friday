//! Durable `LessonStore` backed by FRIDAY's embedded `redb` standard.
//!
//! Layout (matching the convention in `friday-core::memory::backends`):
//! * `LESSONS`      — `lesson_key_json -> ProductionLesson (JSON)`
//! * `SIGNAL_INDEX` — `signal -> JSON array of lesson_key_json strings`
//!
//! Invariants (8B):
//! * `find_similar` is **read-only** and answers Subset/Jaccard via the inverted
//!   index (observed signals → candidate keys → action_class partition → subset
//!   verification). It never loads every lesson to filter.
//! * All mutating ops (`admit`/`strengthen`/`mark_contradicted`/`prune_stale`)
//!   move the lesson record and the index together inside one `redb` transaction,
//!   so the index never points at a missing or half-written lesson.
//! * `redb::Database` is internally locked and `Sync`, so `&self` suffices.

use std::collections::HashSet;
use std::path::Path;

use redb::{Database, ReadableTable, TableDefinition};

use crate::error::LessonStoreError;
use crate::store::signals_match;
use crate::types::{
    recompute_confidence, LessonKey, LessonStatus, MatchPolicy, ProductionLesson, PruneConfig,
};

const MS_PER_DAY: f64 = 86_400_000.0;

const LESSONS: TableDefinition<&str, &str> = TableDefinition::new("lessons");
const SIGNAL_INDEX: TableDefinition<&str, &str> = TableDefinition::new("lesson_signal_index");
/// Verification evidence recorded BEFORE the condition's lesson is first
/// admitted (L1). Folded into the lesson row at admission time, so evidence
/// and lesson share one durable keyspace without ever creating lessons from
/// successes.
const PENDING_VERIFICATIONS: TableDefinition<&str, u64> =
    TableDefinition::new("pending_verifications");
/// Sentinel signal marking a lesson with an empty signal set (matches any Subset query).
const EMPTY_SIGNAL: &str = "__lesson_empty_signals__";

fn key_string(key: &LessonKey) -> Result<String, LessonStoreError> {
    serde_json::to_string(key).map_err(|e| LessonStoreError::Serialization(e.to_string()))
}

fn parse_lesson(json: &str) -> Result<ProductionLesson, LessonStoreError> {
    serde_json::from_str(json).map_err(|e| LessonStoreError::Serialization(e.to_string()))
}

/// Durable lesson store over `redb`.
pub struct RedbLessonStore {
    db: Database,
    config: PruneConfig,
}

impl RedbLessonStore {
    /// Open (or create) the store at `path`.
    pub fn open(path: &Path, config: PruneConfig) -> Result<Self, LessonStoreError> {
        let db = Database::create(path).map_err(|e| LessonStoreError::Storage(e.to_string()))?;
        // Create the table schemas up front so reads on an empty store succeed
        // instead of erroring with "Table does not exist".
        {
            let write = Self::err(db.begin_write())?;
            Self::err(write.open_table(LESSONS))?;
            Self::err(write.open_table(SIGNAL_INDEX))?;
            Self::err(write.open_table(PENDING_VERIFICATIONS))?;
            Self::err(write.commit())?;
        }
        Ok(Self { db, config })
    }

    fn err<T, E: std::fmt::Display>(r: Result<T, E>) -> Result<T, LessonStoreError> {
        r.map_err(|e| LessonStoreError::Storage(e.to_string()))
    }
}

impl crate::store::LessonStore for RedbLessonStore {
    fn admit(&self, mut lesson: ProductionLesson) -> Result<(), LessonStoreError> {
        let key_str = key_string(&lesson.key)?;
        let write = Self::err(self.db.begin_write())?;

        // Reject duplicates inside the same transaction before writing anything.
        let exists = {
            let table = Self::err(write.open_table(LESSONS))?;
            let present = table
                .get(key_str.as_str())
                .map_err(|e| LessonStoreError::Storage(e.to_string()))?;
            present.is_some()
        };
        if exists {
            drop(write);
            return Err(LessonStoreError::AlreadyExists(lesson.key));
        }

        let age_days = (lesson
            .last_observed_ms
            .saturating_sub(lesson.first_observed_ms)) as f64
            / MS_PER_DAY;
        lesson.confidence = recompute_confidence(lesson.support_count, age_days, &self.config);
        lesson.status = LessonStatus::Active;
        let json = serde_json::to_string(&lesson)
            .map_err(|e| LessonStoreError::Serialization(e.to_string()))?;

        {
            let mut table = Self::err(write.open_table(LESSONS))?;
            table
                .insert(key_str.as_str(), json.as_str())
                .map_err(|e| LessonStoreError::Storage(e.to_string()))?;
        }
        // L1 — fold any verification evidence that accumulated before the
        // claim existed into the new row; the pending entry is consumed.
        let read_pending = || -> Result<Option<u64>, LessonStoreError> {
            let pending = Self::err(write.open_table(PENDING_VERIFICATIONS))?;
            let v = pending
                .get(key_str.as_str())
                .map_err(|e| LessonStoreError::Storage(e.to_string()))?
                .map(|v| v.value());
            Ok(v)
        };
        let pending_value = read_pending()?;
        if let Some(v) = pending_value {
            lesson.verification_count += v;
            let json = serde_json::to_string(&lesson)
                .map_err(|e| LessonStoreError::Serialization(e.to_string()))?;
            {
                let mut table = Self::err(write.open_table(LESSONS))?;
                table
                    .insert(key_str.as_str(), json.as_str())
                    .map_err(|e| LessonStoreError::Storage(e.to_string()))?;
            }
            {
                let mut pending = Self::err(write.open_table(PENDING_VERIFICATIONS))?;
                pending
                    .remove(key_str.as_str())
                    .map_err(|e| LessonStoreError::Storage(e.to_string()))?;
            }
        }
        {
            let mut idx = Self::err(write.open_table(SIGNAL_INDEX))?;
            let signals: Vec<String> = if lesson.key.signals.is_empty() {
                vec![EMPTY_SIGNAL.to_string()]
            } else {
                lesson.key.signals.iter().cloned().collect()
            };
            for sig in signals {
                let list_json = match idx
                    .get(sig.as_str())
                    .map_err(|e| LessonStoreError::Storage(e.to_string()))?
                {
                    Some(v) => v.value().to_string(),
                    None => String::from("[]"),
                };
                let mut list: Vec<String> = serde_json::from_str(&list_json)
                    .map_err(|e| LessonStoreError::Serialization(e.to_string()))?;
                if !list.contains(&key_str) {
                    list.push(key_str.clone());
                }
                let j = serde_json::to_string(&list)
                    .map_err(|e| LessonStoreError::Serialization(e.to_string()))?;
                idx.insert(sig.as_str(), j.as_str())
                    .map_err(|e| LessonStoreError::Storage(e.to_string()))?;
            }
        }
        Self::err(write.commit())?;
        Ok(())
    }

    fn strengthen(
        &self,
        key: &LessonKey,
        now_ms: u64,
    ) -> Result<Option<ProductionLesson>, LessonStoreError> {
        let key_str = key_string(key)?;
        let write = Self::err(self.db.begin_write())?;
        let mut updated = None;
        {
            let mut table = Self::err(write.open_table(LESSONS))?;
            let existing = table
                .get(key_str.as_str())
                .map_err(|e| LessonStoreError::Storage(e.to_string()))?
                .map(|v| v.value().to_string());
            if let Some(json_str) = existing {
                let mut lesson = parse_lesson(&json_str)?;
                lesson.support_count += 1;
                lesson.last_observed_ms = now_ms;
                let age_days =
                    (now_ms.saturating_sub(lesson.first_observed_ms)) as f64 / MS_PER_DAY;
                lesson.confidence =
                    recompute_confidence(lesson.support_count, age_days, &self.config);
                lesson.status = LessonStatus::Active; // a strengthen refreshes staleness
                let json = serde_json::to_string(&lesson)
                    .map_err(|e| LessonStoreError::Serialization(e.to_string()))?;
                table
                    .insert(key_str.as_str(), json.as_str())
                    .map_err(|e| LessonStoreError::Storage(e.to_string()))?;
                updated = Some(lesson);
            }
        }
        Self::err(write.commit())?;
        Ok(updated)
    }

    fn find_similar(
        &self,
        key: &LessonKey,
        policy: MatchPolicy,
    ) -> Result<Vec<ProductionLesson>, LessonStoreError> {
        let read = Self::err(self.db.begin_read())?;
        let lessons = Self::err(read.open_table(LESSONS))?;

        // Exact is a direct keyed read — no index scan.
        if matches!(policy, MatchPolicy::Exact) {
            let ks = key_string(key)?;
            return match lessons
                .get(ks.as_str())
                .map_err(|e| LessonStoreError::Storage(e.to_string()))?
            {
                Some(v) => {
                    let l = parse_lesson(v.value())?;
                    if l.status == LessonStatus::Archived {
                        Ok(vec![])
                    } else {
                        Ok(vec![l])
                    }
                }
                None => Ok(vec![]),
            };
        }

        // Subset / Jaccard: gather candidates from the inverted index, then verify.
        let idx = Self::err(read.open_table(SIGNAL_INDEX))?;
        let mut candidates: HashSet<String> = HashSet::new();
        for sig in &key.signals {
            if let Some(v) = idx
                .get(sig.as_str())
                .map_err(|e| LessonStoreError::Storage(e.to_string()))?
            {
                let list: Vec<String> = serde_json::from_str(v.value())
                    .map_err(|e| LessonStoreError::Serialization(e.to_string()))?;
                for k in list {
                    candidates.insert(k);
                }
            }
        }
        // Lessons with an empty signal set match any Subset query.
        if let Some(v) = idx
            .get(EMPTY_SIGNAL)
            .map_err(|e| LessonStoreError::Storage(e.to_string()))?
        {
            let list: Vec<String> = serde_json::from_str(v.value())
                .map_err(|e| LessonStoreError::Serialization(e.to_string()))?;
            for k in list {
                candidates.insert(k);
            }
        }

        let mut out = Vec::new();
        for ks in candidates {
            match lessons
                .get(ks.as_str())
                .map_err(|e| LessonStoreError::Storage(e.to_string()))?
            {
                Some(v) => {
                    let l = parse_lesson(v.value())?;
                    if l.status == LessonStatus::Archived {
                        continue;
                    }
                    if l.key.action_class != key.action_class {
                        continue;
                    }
                    if signals_match(&l.key, key, policy) {
                        out.push(l);
                    }
                }
                None => continue,
            }
        }
        Ok(out)
    }

    fn get(&self, key: &LessonKey) -> Result<Option<ProductionLesson>, LessonStoreError> {
        let read = Self::err(self.db.begin_read())?;
        let lessons = Self::err(read.open_table(LESSONS))?;
        let ks = key_string(key)?;
        match lessons
            .get(ks.as_str())
            .map_err(|e| LessonStoreError::Storage(e.to_string()))?
        {
            Some(v) => Ok(Some(parse_lesson(v.value())?)),
            None => Ok(None),
        }
    }

    fn mark_contradicted(&self, key: &LessonKey, with: &LessonKey) -> Result<(), LessonStoreError> {
        let write = Self::err(self.db.begin_write())?;
        {
            let mut table = Self::err(write.open_table(LESSONS))?;
            for k in [key, with] {
                let ks = key_string(k)?;
                let existing = table
                    .get(ks.as_str())
                    .map_err(|e| LessonStoreError::Storage(e.to_string()))?
                    .map(|v| v.value().to_string());
                if let Some(json_str) = existing {
                    let mut lesson = parse_lesson(&json_str)?;
                    lesson.status = LessonStatus::Contradicted;
                    let json = serde_json::to_string(&lesson)
                        .map_err(|e| LessonStoreError::Serialization(e.to_string()))?;
                    table
                        .insert(ks.as_str(), json.as_str())
                        .map_err(|e| LessonStoreError::Storage(e.to_string()))?;
                }
            }
        }
        Self::err(write.commit())?;
        Ok(())
    }

    fn prune_stale(&self, now_ms: u64, config: &PruneConfig) -> Result<usize, LessonStoreError> {
        let write = Self::err(self.db.begin_write())?;
        let mut changed = 0;
        // Explicit maintenance scan is legitimate here (unlike find_similar).
        let entries: Vec<(String, ProductionLesson)> = {
            let table = Self::err(write.open_table(LESSONS))?;
            let iter = table
                .iter()
                .map_err(|e| LessonStoreError::Storage(e.to_string()))?;
            let mut collected = Vec::new();
            for r in iter {
                let (k, v) = r.map_err(|e| LessonStoreError::Storage(e.to_string()))?;
                let lesson = parse_lesson(v.value())?;
                collected.push((k.value().to_string(), lesson));
            }
            collected
        };
        {
            let mut table = Self::err(write.open_table(LESSONS))?;
            for (ks, mut lesson) in entries {
                if lesson.status == LessonStatus::Contradicted {
                    continue;
                }
                let age_days = (now_ms.saturating_sub(lesson.last_observed_ms)) as f64 / MS_PER_DAY;
                let conf = recompute_confidence(lesson.support_count, age_days, config);
                lesson.confidence = conf;
                let new_status = if conf < config.confidence_floor {
                    LessonStatus::Archived
                } else if now_ms.saturating_sub(lesson.last_observed_ms) > config.ttl_ms {
                    LessonStatus::Stale
                } else {
                    lesson.status
                };
                if new_status != lesson.status {
                    lesson.status = new_status;
                    changed += 1;
                    let json = serde_json::to_string(&lesson)
                        .map_err(|e| LessonStoreError::Serialization(e.to_string()))?;
                    table
                        .insert(ks.as_str(), json.as_str())
                        .map_err(|e| LessonStoreError::Storage(e.to_string()))?;
                }
            }
        }
        Self::err(write.commit())?;
        Ok(changed)
    }

    fn record_verification(&self, key: &LessonKey) -> Result<u64, LessonStoreError> {
        let ks = key_string(key)?;
        let write = Self::err(self.db.begin_write())?;
        // Does a lesson row already exist for this claim?
        let exists = {
            let table = Self::err(write.open_table(LESSONS))?;
            let present = table
                .get(ks.as_str())
                .map_err(|e| LessonStoreError::Storage(e.to_string()))?;
            let found = present.is_some();
            drop(present);
            drop(table);
            found
        };
        if exists {
            let mut table = Self::err(write.open_table(LESSONS))?;
            let json_str = table
                .get(ks.as_str())
                .map_err(|e| LessonStoreError::Storage(e.to_string()))?
                .expect("checked exists above")
                .value()
                .to_string();
            let mut lesson = parse_lesson(&json_str)?;
            lesson.verification_count += 1;
            let new_count = lesson.verification_count;
            let json = serde_json::to_string(&lesson)
                .map_err(|e| LessonStoreError::Serialization(e.to_string()))?;
            table
                .insert(ks.as_str(), json.as_str())
                .map_err(|e| LessonStoreError::Storage(e.to_string()))?;
            drop(table);
            Self::err(write.commit())?;
            return Ok(new_count);
        }
        // Claim not admitted yet: bank the evidence durably; it is folded
        // into the lesson row at admission time. Never creates a lesson.
        let mut pending = Self::err(write.open_table(PENDING_VERIFICATIONS))?;
        let current = pending
            .get(ks.as_str())
            .map_err(|e| LessonStoreError::Storage(e.to_string()))?
            .map(|v| v.value())
            .unwrap_or(0);
        let new_count = current + 1;
        pending
            .insert(ks.as_str(), new_count)
            .map_err(|e| LessonStoreError::Storage(e.to_string()))?;
        drop(pending);
        Self::err(write.commit())?;
        Ok(new_count)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::LessonStore;
    use crate::types::LessonProvenance;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    static COUNTER: AtomicU64 = AtomicU64::new(0);

    fn tmp_path(tag: &str) -> PathBuf {
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        std::env::temp_dir().join(format!(
            "friday_lesson_redb_{}_{}_{}",
            tag,
            std::process::id(),
            n
        ))
    }

    fn prov(raw: &str) -> LessonProvenance {
        LessonProvenance {
            action_raw: raw.to_string(),
            trace_id: None,
            sample_signals: vec![],
        }
    }

    fn lesson(key: LessonKey, support: u64, first: u64, last: u64) -> ProductionLesson {
        let sigs = key.signals.clone();
        crate::types::build_lesson(
            key,
            format!("prediction for {:?} unreliable", sigs),
            support,
            first,
            last,
            prov("act"),
            &PruneConfig::default(),
        )
    }

    // 1. round-trip persistence: every field survives JSON ↔ redb ↔ struct.
    #[test]
    fn round_trip_preserves_all_fields() {
        let path = tmp_path("roundtrip");
        let key = LessonKey::new("git.commit", ["missing_repo", "dirty_tree"]);
        let original = {
            let mut l = lesson(key.clone(), 3, 1000, 2000);
            l.claim = "prediction for git.commit unreliable under repo signals".into();
            l
        };
        {
            let store = RedbLessonStore::open(&path, PruneConfig::default()).unwrap();
            store.admit(original.clone()).unwrap();
        }
        {
            let store = RedbLessonStore::open(&path, PruneConfig::default()).unwrap();
            let recovered = store.get(&key).unwrap().expect("present after reopen");
            assert_eq!(recovered.key, original.key);
            assert_eq!(recovered.claim, original.claim);
            assert_eq!(recovered.support_count, original.support_count);
            assert_eq!(recovered.first_observed_ms, original.first_observed_ms);
            assert_eq!(recovered.last_observed_ms, original.last_observed_ms);
            assert!((recovered.confidence - original.confidence).abs() < 1e-6);
            assert_eq!(recovered.status, LessonStatus::Active);
            assert_eq!(recovered.provenance.action_raw, "act");
        }
    }

    // 2. reopen persistence: lesson admitted, store dropped, reopened, recovered.
    #[test]
    fn reopen_recovers_lesson() {
        let path = tmp_path("reopen");
        let key = LessonKey::new("browser.search", ["network_unavailable"]);
        {
            let store = RedbLessonStore::open(&path, PruneConfig::default()).unwrap();
            store.admit(lesson(key.clone(), 1, 1000, 1000)).unwrap();
        }
        let store = RedbLessonStore::open(&path, PruneConfig::default()).unwrap();
        let hits = store.find_similar(&key, MatchPolicy::Exact).unwrap();
        assert_eq!(hits.len(), 1);
    }

    // 3. subset retrieval through the persisted index.
    #[test]
    fn subset_retrieval_uses_persisted_index() {
        let path = tmp_path("subset");
        let general = LessonKey::new("browser.search", ["network_unavailable"]);
        {
            let store = RedbLessonStore::open(&path, PruneConfig::default()).unwrap();
            store.admit(lesson(general.clone(), 1, 1000, 1000)).unwrap();
        }
        let store = RedbLessonStore::open(&path, PruneConfig::default()).unwrap();
        let specific = LessonKey::new("browser.search", ["network_unavailable", "browser_restart"]);
        let hits = store.find_similar(&specific, MatchPolicy::Subset).unwrap();
        assert_eq!(hits.len(), 1, "general lesson matches specific query");

        // And the reverse must NOT match (specific stored ⊄ general query).
        let too_specific =
            LessonKey::new("browser.search", ["network_unavailable", "browser_restart"]);
        store
            .admit(lesson(too_specific.clone(), 1, 1000, 1000))
            .unwrap();
        let general_q = LessonKey::new("browser.search", ["network_unavailable"]);
        let hits = store.find_similar(&general_q, MatchPolicy::Subset).unwrap();
        assert_eq!(hits.len(), 1, "only the general lesson matches");
    }

    // 4. exact matching.
    #[test]
    fn exact_matching() {
        let path = tmp_path("exact");
        let key = LessonKey::new("a", ["b"]);
        let store = RedbLessonStore::open(&path, PruneConfig::default()).unwrap();
        store.admit(lesson(key.clone(), 1, 1000, 1000)).unwrap();
        assert_eq!(
            store.find_similar(&key, MatchPolicy::Exact).unwrap().len(),
            1
        );
        assert_eq!(
            store
                .find_similar(&LessonKey::new("a", ["b", "c"]), MatchPolicy::Exact)
                .unwrap()
                .len(),
            0
        );
    }

    // 5. jaccard matching.
    #[test]
    fn jaccard_matching() {
        let path = tmp_path("jaccard");
        let key = LessonKey::new("a", ["b", "c", "d"]);
        let store = RedbLessonStore::open(&path, PruneConfig::default()).unwrap();
        store.admit(lesson(key.clone(), 1, 1000, 1000)).unwrap();
        assert_eq!(
            store
                .find_similar(
                    &LessonKey::new("a", ["b", "e", "f"]),
                    MatchPolicy::Jaccard(0.5)
                )
                .unwrap()
                .len(),
            0
        );
        assert_eq!(
            store
                .find_similar(
                    &LessonKey::new("a", ["b", "c", "e"]),
                    MatchPolicy::Jaccard(0.5)
                )
                .unwrap()
                .len(),
            1
        );
    }

    // 6. strengthen persists across reopen.
    #[test]
    fn strengthen_persists() {
        let path = tmp_path("strengthen");
        let key = LessonKey::new("git.commit", ["dirty"]);
        {
            let store = RedbLessonStore::open(&path, PruneConfig::default()).unwrap();
            store.admit(lesson(key.clone(), 1, 1000, 1000)).unwrap();
            store.strengthen(&key, 2000).unwrap();
            store.strengthen(&key, 3000).unwrap();
        }
        let store = RedbLessonStore::open(&path, PruneConfig::default()).unwrap();
        let l = store.get(&key).unwrap().unwrap();
        assert_eq!(l.support_count, 3);
        assert_eq!(l.last_observed_ms, 3000);
    }

    // 7. contradiction persists across reopen.
    #[test]
    fn contradiction_persists() {
        let path = tmp_path("contradict");
        let k1 = LessonKey::new("a", ["b", "c"]);
        let k2 = LessonKey::new("a", ["b", "d"]);
        {
            let store = RedbLessonStore::open(&path, PruneConfig::default()).unwrap();
            store.admit(lesson(k1.clone(), 1, 1000, 1000)).unwrap();
            store.admit(lesson(k2.clone(), 1, 1000, 1000)).unwrap();
            store.mark_contradicted(&k1, &k2).unwrap();
        }
        let store = RedbLessonStore::open(&path, PruneConfig::default()).unwrap();
        assert_eq!(
            store.get(&k1).unwrap().unwrap().status,
            LessonStatus::Contradicted
        );
        assert_eq!(
            store.get(&k2).unwrap().unwrap().status,
            LessonStatus::Contradicted
        );
    }

    // 8. stale/archive pruning persists across reopen.
    #[test]
    fn prune_persists() {
        let path = tmp_path("prune");
        let cfg = PruneConfig {
            ttl_ms: 1000,
            confidence_floor: 0.5,
            ..PruneConfig::default()
        };
        {
            let store = RedbLessonStore::open(&path, cfg.clone()).unwrap();
            let archived = LessonKey::new("old", ["x"]);
            store.admit(lesson(archived.clone(), 1, 0, 0)).unwrap();
            let stale = LessonKey::new("stale", ["y"]);
            store.admit(lesson(stale.clone(), 5, 0, 0)).unwrap();
            for _ in 0..10 {
                store.strengthen(&stale, 0).unwrap();
            }
            store.prune_stale(864_000_000, &cfg).unwrap();
        }
        let store = RedbLessonStore::open(&path, cfg.clone()).unwrap();
        assert_eq!(
            store
                .get(&LessonKey::new("old", ["x"]))
                .unwrap()
                .unwrap()
                .status,
            LessonStatus::Archived
        );
        assert_eq!(
            store
                .get(&LessonKey::new("stale", ["y"]))
                .unwrap()
                .unwrap()
                .status,
            LessonStatus::Stale
        );
    }

    // 9. find_similar is read-only (no mutation across repeated reads).
    #[test]
    fn find_similar_is_read_only() {
        let path = tmp_path("readonly");
        let key = LessonKey::new("a", ["b"]);
        let store = RedbLessonStore::open(&path, PruneConfig::default()).unwrap();
        store.admit(lesson(key.clone(), 2, 1000, 1000)).unwrap();
        for _ in 0..5 {
            let hits = store.find_similar(&key, MatchPolicy::Exact).unwrap();
            assert_eq!(hits.len(), 1);
        }
        assert_eq!(store.get(&key).unwrap().unwrap().support_count, 2);
    }

    // 10. duplicate admission is rejected.
    #[test]
    fn duplicate_admission_rejected() {
        let path = tmp_path("dup");
        let key = LessonKey::new("a", ["b"]);
        let store = RedbLessonStore::open(&path, PruneConfig::default()).unwrap();
        store.admit(lesson(key.clone(), 1, 1000, 1000)).unwrap();
        assert!(store.admit(lesson(key.clone(), 1, 1000, 1000)).is_err());
    }

    // 11. unknown lesson returns None / empty.
    #[test]
    fn unknown_returns_none() {
        let path = tmp_path("unknown");
        let store = RedbLessonStore::open(&path, PruneConfig::default()).unwrap();
        let key = LessonKey::new("nope", ["z"]);
        assert!(store.get(&key).unwrap().is_none());
        assert_eq!(
            store.find_similar(&key, MatchPolicy::Subset).unwrap().len(),
            0
        );
    }

    // 12. index consistency after updates: subset query returns correct, bounded
    // set and never a lesson whose signals are not a subset.
    #[test]
    fn index_consistency_after_updates() {
        let path = tmp_path("consistency");
        let broad = LessonKey::new("a", ["b", "c", "d"]);
        let narrow = LessonKey::new("a", ["b"]);
        let store = RedbLessonStore::open(&path, PruneConfig::default()).unwrap();
        store.admit(lesson(broad.clone(), 1, 1000, 1000)).unwrap();
        store.admit(lesson(narrow.clone(), 1, 1000, 1000)).unwrap();
        store.strengthen(&narrow, 2000).unwrap();

        // Dropping store and reopening must preserve index + updates.
        drop(store);
        let store = RedbLessonStore::open(&path, PruneConfig::default()).unwrap();

        // Query {b}: both broad (b⊆{b}? no, broad={b,c,d} not subset of {b}) and
        // narrow ({b} ⊆ {b}) — only narrow matches.
        let hits = store
            .find_similar(&LessonKey::new("a", ["b"]), MatchPolicy::Subset)
            .unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].key, narrow);

        // Query {b,c,d}: both match (narrow ⊆, broad ⊆).
        let hits = store
            .find_similar(&LessonKey::new("a", ["b", "c", "d"]), MatchPolicy::Subset)
            .unwrap();
        assert_eq!(hits.len(), 2);

        // Index did not corrupt: narrow support persisted through reopen.
        assert_eq!(store.get(&narrow).unwrap().unwrap().support_count, 2);
    }
}
