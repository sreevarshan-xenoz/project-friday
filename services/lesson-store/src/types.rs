use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

/// Normalized equivalence/dedup key for a lesson. Exact `(action_class, signal set)`
/// equality ⇒ same lesson. `BTreeSet` gives canonical ordering so the key is
/// deterministic and hashable.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct LessonKey {
    pub action_class: String,
    pub signals: BTreeSet<String>,
}

impl LessonKey {
    pub fn new(
        action_class: impl Into<String>,
        signals: impl IntoIterator<Item = impl Into<String>>,
    ) -> Self {
        Self {
            action_class: action_class.into(),
            signals: signals.into_iter().map(|s| s.into()).collect(),
        }
    }
}

/// How `find_similar` matches a stored lesson against a query key.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum MatchPolicy {
    /// Stored key equals the query key exactly.
    Exact,
    /// Stored lesson is *more general*: `stored.signals ⊆ query.signals` ⇒ it
    /// applies to the observation. (The reverse is intentionally **not** true — a
    /// too-specific stored lesson does not apply to a narrower observation.)
    Subset,
    /// Jaccard similarity between the two signal sets must meet the threshold.
    Jaccard(f64),
}

impl Default for MatchPolicy {
    fn default() -> Self {
        MatchPolicy::Subset
    }
}

/// Lifecycle status of a lesson.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LessonStatus {
    /// Normal, usable by the planner consultation seam.
    Active,
    /// Last observed longer ago than the TTL (not contradicted).
    Stale,
    /// Overlaps an opposite/contradicting lesson; excluded from consultation.
    Contradicted,
    /// Confidence fell below the floor; retained for audit only.
    Archived,
}

/// Audit trail for a lesson. The raw action is kept verbatim; the canonical
/// `action_class` lives in [`LessonKey`]. `trace_id` is stored by value only —
/// lessons never copy `CognitiveTrace` data (frozen contract untouched).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LessonProvenance {
    pub action_raw: String,
    pub trace_id: Option<String>,
    pub sample_signals: Vec<String>,
}

/// A production lesson: the prediction-error generalization (7C semantics).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProductionLesson {
    pub key: LessonKey,
    /// Human-readable generalizable claim, e.g. "prediction for 'browser.search'
    /// unreliable when signals [network_unavailable] diverge".
    pub claim: String,
    /// Belief strength in `[0,1]`; rises with support, falls with age/contradiction.
    pub confidence: f32,
    /// Number of corroborating experiences backing this lesson.
    pub support_count: u64,
    pub first_observed_ms: u64,
    pub last_observed_ms: u64,
    pub status: LessonStatus,
    pub provenance: LessonProvenance,
}

impl ProductionLesson {
    /// Usable by the planner consultation seam.
    pub fn is_usable(&self) -> bool {
        self.status == LessonStatus::Active
    }
}

/// Tunable parameters for pruning/decay. All configurable seams — no hardcoded
/// production numbers baked into logic.
#[derive(Debug, Clone)]
pub struct PruneConfig {
    /// Age beyond which a lesson becomes `Stale` (unless `Contradicted`).
    pub ttl_ms: u64,
    /// Confidence beneath which a lesson is `Archived`.
    pub confidence_floor: f32,
    /// Confidence lost per day of age (applied in recomputation).
    pub decay_per_day: f32,
}

impl Default for PruneConfig {
    fn default() -> Self {
        Self {
            ttl_ms: 30 * 24 * 60 * 60 * 1000, // 30 days
            confidence_floor: 0.2,
            decay_per_day: 0.01,
        }
    }
}

const MS_PER_DAY: f64 = 86_400_000.0;

/// Recompute confidence: 7C heritage for the support term (diminishing returns),
/// minus a configurable age decay. Bounded to `[0, 0.99]`.
pub fn recompute_confidence(support: u64, age_days: f64, config: &PruneConfig) -> f32 {
    let base = (0.5 + (support as f32).min(10.0) * 0.05).min(0.99);
    let decay = config.decay_per_day * age_days as f32;
    (base - decay).max(0.0).min(0.99)
}

/// Build a `ProductionLesson` with confidence normalized via [`recompute_confidence`].
pub fn build_lesson(
    key: LessonKey,
    claim: impl Into<String>,
    support_count: u64,
    first_observed_ms: u64,
    last_observed_ms: u64,
    provenance: LessonProvenance,
    config: &PruneConfig,
) -> ProductionLesson {
    let age_days = (last_observed_ms.saturating_sub(first_observed_ms)) as f64 / MS_PER_DAY;
    let confidence = recompute_confidence(support_count, age_days, config);
    ProductionLesson {
        key,
        claim: claim.into(),
        confidence,
        support_count,
        first_observed_ms,
        last_observed_ms,
        status: LessonStatus::Active,
        provenance,
    }
}
