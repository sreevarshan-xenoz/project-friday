//! # FRIDAY Experience Engine
//!
//! Aggregates `DecisionRecord`s from the plugin runtime's `DecisionLedger`
//! into `Experience` — statistical summaries per (capability, operator).
//!
//! The experience crate is a **consumer** of the runtime. The runtime does
//! not depend on this crate. Experience is derived from history, not from
//! in-flight telemetry.
//!
//! ## Pipeline
//!
//! ```text
//! DecisionLedger
//!        │
//!        ▼
//! ExperienceEngine.update()
//!        │
//!        ▼
//! ExperienceStore
//!        │
//!        ▼
//! ReputationEngine.update()
//!        │
//!        ▼
//! ReputationStore
//!        │
//!        ▼
//! CostModel.estimate() / .rank()
//! ```
//!
//! ## Invariants
//!
//! - Experience is derived exclusively from DecisionRecords.
//! - Reputation is a projection of Experience, never of DecisionRecords.
//! - The ExperienceEngine uses a cursor (`last_processed`) so each record
//!   is processed exactly once.
//! - Reputation is derived exclusively from Experience.
//! - No scheduling, no background threads, no async.

use std::collections::HashMap;

use friday_plugin_runtime::{
    Capability, CostEstimate, DecisionLedger, DecisionRecord, Experience, ExperienceKey,
    PlannerFeedback, Reputation,
};

// ── Calibration ─────────────────────────────────────────────────────────────────

/// Per (Capability, Operator) calibration offsets.
///
/// These are the only mutable state the CostModel maintains.
/// All offsets are bounded to prevent oscillation.
#[derive(Debug, Clone)]
pub struct CalibrationOffsets {
    pub latency_offset: f64,
    pub cost_offset: f64,
    pub success_offset: f64,
    pub confidence_offset: f64,
}

impl CalibrationOffsets {
    pub fn new() -> Self {
        Self {
            latency_offset: 0.0,
            cost_offset: 0.0,
            success_offset: 0.0,
            confidence_offset: 0.0,
        }
    }
}

impl Default for CalibrationOffsets {
    fn default() -> Self {
        Self::new()
    }
}

/// Maintains calibration state per (Capability, Operator).
///
/// The CostModel uses this to adjust future predictions based on
/// observed PlannerFeedback. Calibration is the **learning** layer
/// — it mutates offsets, never history.
pub struct CalibrationStore {
    offsets: HashMap<ExperienceKey, CalibrationOffsets>,
    learning_rate: f64,
}

impl CalibrationStore {
    pub fn new() -> Self {
        Self {
            offsets: HashMap::new(),
            learning_rate: 0.3,
        }
    }

    /// Get calibration offsets for a (capability, operator) pair.
    pub fn get(&self, key: &ExperienceKey) -> CalibrationOffsets {
        self.offsets.get(key).cloned().unwrap_or_default()
    }

    /// Apply feedback to adjust calibration offsets.
    ///
    /// Uses bounded learning rate and clamps to prevent oscillation.
    pub fn apply_feedback(&mut self, feedback: &PlannerFeedback) {
        let key = ExperienceKey {
            capability: feedback.capability.clone(),
            operator_id: feedback.operator_id.clone(),
        };

        let mut offsets = self.get(&key);
        let lr = self.learning_rate;

        // Update with bounded learning rate, clamp to prevent oscillation
        offsets.latency_offset = (offsets.latency_offset
            + lr * feedback.latency_adjustment_ms.clamp(-100.0, 100.0))
        .clamp(-500.0, 500.0);

        offsets.cost_offset = (offsets.cost_offset
            + lr * feedback.cost_adjustment.clamp(-0.5, 0.5))
        .clamp(-1.0, 1.0);

        offsets.success_offset = (offsets.success_offset
            + lr * feedback.success_adjustment.clamp(-0.5, 0.5))
        .clamp(-0.3, 0.3);

        offsets.confidence_offset = (offsets.confidence_offset
            + lr * feedback.confidence_adjustment.clamp(-0.5, 0.5))
        .clamp(-0.5, 0.5);

        self.offsets.insert(key, offsets);
    }

    /// Number of (capability, operator) pairs being calibrated.
    pub fn len(&self) -> usize {
        self.offsets.len()
    }

    pub fn is_empty(&self) -> bool {
        self.offsets.is_empty()
    }

    /// Clear all calibration offsets.
    pub fn clear(&mut self) {
        self.offsets.clear();
    }
}

impl Default for CalibrationStore {
    fn default() -> Self {
        Self::new()
    }
}

/// Storage for Experience aggregates.
///
/// Owns history-derived knowledge. Insert-only from the engine's
/// perspective — external consumers read via `get()` or `iter()`.
pub struct ExperienceStore {
    experiences: HashMap<ExperienceKey, Experience>,
}

impl ExperienceStore {
    pub fn new() -> Self {
        Self {
            experiences: HashMap::new(),
        }
    }

    /// Insert or update an experience entry.
    pub fn insert(&mut self, experience: Experience) {
        self.experiences.insert(experience.key.clone(), experience);
    }

    /// Get experience for a specific key.
    pub fn get(&self, key: &ExperienceKey) -> Option<&Experience> {
        self.experiences.get(key)
    }

    /// Iterate over all stored experiences.
    pub fn iter(&self) -> impl Iterator<Item = &Experience> {
        self.experiences.values()
    }

    /// Number of distinct (capability, operator) entries.
    pub fn len(&self) -> usize {
        self.experiences.len()
    }

    /// Returns true if the store is empty.
    pub fn is_empty(&self) -> bool {
        self.experiences.is_empty()
    }

    /// Clear all experiences. Intended for testing.
    pub fn clear(&mut self) {
        self.experiences.clear();
    }
}

impl Default for ExperienceStore {
    fn default() -> Self {
        Self::new()
    }
}

/// Aggregates DecisionRecords into Experience.
///
/// Uses a cursor (`last_processed`) so each record is processed
/// exactly once. Call `update()` periodically with the ledger to
/// incorporate new records.
pub struct ExperienceEngine {
    last_processed: usize,
}

impl ExperienceEngine {
    pub fn new() -> Self {
        Self { last_processed: 0 }
    }

    /// Process new records from the ledger since the last update.
    ///
    /// Each call advances the cursor to the ledger's current length.
    /// Records before the cursor are never re-processed.
    pub fn update(&mut self, ledger: &DecisionLedger, store: &mut ExperienceStore) {
        let current_len = ledger.len();
        for i in self.last_processed..current_len {
            if let Some(record) = ledger.get(i) {
                apply_record(record, store);
            }
        }
        self.last_processed = current_len;
    }

    /// Reset the cursor. Next `update()` will re-process all records.
    pub fn reset(&mut self) {
        self.last_processed = 0;
    }

    /// Get the current cursor position.
    pub fn last_processed(&self) -> usize {
        self.last_processed
    }
}

impl Default for ExperienceEngine {
    fn default() -> Self {
        Self::new()
    }
}

/// Apply a single DecisionRecord to the experience store.
///
/// Updates four aggregates:
///   - sample_size (incremented)
///   - success_rate (running proportion)
///   - average_latency_ms (running average)
///   - last_failure (if the record is a failure)
fn apply_record(record: &DecisionRecord, store: &mut ExperienceStore) {
    let key = ExperienceKey {
        capability: record.capability.clone(),
        operator_id: record.plan.operator_id.clone(),
    };

    let existing = store.get(&key).cloned();
    let exp = existing.unwrap_or_else(|| Experience {
        key: key.clone(),
        sample_size: 0,
        success_rate: 0.0,
        success_count: 0,
        failure_count: 0,
        average_latency_ms: 0.0,
        p50_latency_ms: 0,
        p95_latency_ms: 0,
        p99_latency_ms: 0,
        last_execution_ms: Some(record.timestamp_ms),
        last_failure_ms: None,
        last_failure_reason: None,
        consecutive_successes: 0,
        consecutive_failures: 0,
        since_ms: record.timestamp_ms,
    });

    let n = exp.sample_size;
    let new_n = n + 1;
    let success = record.outcome.success;

    // Running averages
    let new_success_count = exp.success_count + if success { 1 } else { 0 };
    let new_failure_count = exp.failure_count + if success { 0 } else { 1 };
    let new_success_rate = new_success_count as f64 / new_n as f64;
    let new_avg_latency = if n == 0 {
        record.outcome.latency_ms as f64
    } else {
        (exp.average_latency_ms * n as f64 + record.outcome.latency_ms as f64) / new_n as f64
    };

    let new_last_failure_ms = if success {
        exp.last_failure_ms
    } else {
        Some(record.timestamp_ms)
    };
    let new_last_failure_reason = if success {
        exp.last_failure_reason
    } else {
        record.outcome.error.clone()
    };

    let new_consecutive_successes = if success {
        exp.consecutive_successes + 1
    } else {
        0
    };
    let new_consecutive_failures = if success {
        0
    } else {
        exp.consecutive_failures + 1
    };

    store.insert(Experience {
        key,
        sample_size: new_n,
        success_rate: new_success_rate,
        success_count: new_success_count,
        failure_count: new_failure_count,
        average_latency_ms: new_avg_latency,
        p50_latency_ms: 0,
        p95_latency_ms: 0,
        p99_latency_ms: 0,
        last_execution_ms: Some(record.timestamp_ms),
        last_failure_ms: new_last_failure_ms,
        last_failure_reason: new_last_failure_reason,
        consecutive_successes: new_consecutive_successes,
        consecutive_failures: new_consecutive_failures,
        since_ms: exp.since_ms,
    });
}

/// Storage for operator reputation.
///
/// Keyed by operator_id. Only the ReputationEngine may mutate entries.
/// External consumers read via `get()` or `iter()`.
pub struct ReputationStore {
    reputations: HashMap<String, Reputation>,
}

impl ReputationStore {
    pub fn new() -> Self {
        Self {
            reputations: HashMap::new(),
        }
    }

    /// Insert or update a reputation entry.
    pub fn insert(&mut self, reputation: Reputation) {
        self.reputations
            .insert(reputation.operator_id.clone(), reputation);
    }

    /// Get reputation for a specific operator.
    pub fn get(&self, operator_id: &str) -> Option<&Reputation> {
        self.reputations.get(operator_id)
    }

    /// Iterate over all stored reputations.
    pub fn iter(&self) -> impl Iterator<Item = &Reputation> {
        self.reputations.values()
    }

    /// Number of operators with reputation data.
    pub fn len(&self) -> usize {
        self.reputations.len()
    }

    /// Returns true if the store is empty.
    pub fn is_empty(&self) -> bool {
        self.reputations.is_empty()
    }

    /// Clear all reputations. Intended for testing.
    pub fn clear(&mut self) {
        self.reputations.clear();
    }
}

impl Default for ReputationStore {
    fn default() -> Self {
        Self::new()
    }
}

/// Derives long-term trust from Experience.
///
/// Reputation is operator-level (cross-capability). It consumes
/// Experience, which is (capability, operator) level, and produces
/// a single trust score per operator.
///
/// Initially uses simple formulas with constant decay. Future
/// iterations can add time-decay, confidence intervals, and
/// trend detection without changing the pipeline.
pub struct ReputationEngine;

impl ReputationEngine {
    /// Derive reputation for all operators from the experience store.
    ///
    /// Each operator's reputation is computed across all capabilities
    /// they provide. The engine reads from ExperienceStore and writes
    /// to ReputationStore — it does not consume the input store.
    pub fn update(experience_store: &ExperienceStore, reputation_store: &mut ReputationStore) {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;

        // Group experiences by operator_id
        let mut by_operator: HashMap<&str, Vec<&Experience>> = HashMap::new();
        for exp in experience_store.iter() {
            by_operator
                .entry(exp.key.operator_id.as_str())
                .or_default()
                .push(exp);
        }

        for (operator_id, experiences) in &by_operator {
            let total_observations: u64 = experiences.iter().map(|e| e.sample_size).sum();
            let weighted_reliability: f64 = experiences
                .iter()
                .map(|e| e.success_rate * e.sample_size as f64)
                .sum::<f64>()
                / total_observations.max(1) as f64;
            let since_ms = experiences
                .iter()
                .map(|e| e.since_ms)
                .min()
                .unwrap_or(now);

            let reputation = Reputation {
                operator_id: operator_id.to_string(),
                reliability: weighted_reliability,
                confidence: (total_observations as f64 / 100.0).min(1.0),
                total_observations,
                since_ms,
                last_updated_ms: now,
                decay_factor: 0.95,
            };

            reputation_store.insert(reputation);
        }
    }
}

/// Predicts execution outcomes from Experience and Reputation.
///
/// The CostModel answers: "What is likely to happen next?"
///
/// Unlike Experience (which records what happened) and Reputation (which
/// tracks operator trust), the CostModel **projects** past knowledge onto
/// future executions.
///
/// Calibration offsets (from `CalibrationStore`) are applied on top
/// of the base estimate to correct systematic bias over time.
///
/// ## Invariants
///
/// - The CostModel may change estimates, but it may never rewrite history.
///   `DecisionRecord` is immutable. Experience is a deterministic projection
///   of the ledger. Reputation is a deterministic projection of experience.
///   Only future predictions change.
/// - Calibration is per (Capability, Operator) and bounded to prevent
///   oscillation.
pub struct CostModel;

impl CostModel {
    /// Produce a cost estimate for a specific (capability, operator).
    ///
    /// Blends Experience (per-capability statistics) with Reputation
    /// (cross-capability trust) to produce a dimensional estimate.
    ///
    /// When `calibration` is provided, offsets are applied to correct
    /// systematic bias learned from prior feedback.
    ///
    /// When no data is available, returns conservative defaults with
    /// confidence = 0.0.
    pub fn estimate(
        capability: &Capability,
        operator_id: &str,
        experience_store: &ExperienceStore,
        reputation_store: &ReputationStore,
        calibration: Option<&CalibrationStore>,
    ) -> CostEstimate {
        let exp_key = ExperienceKey {
            capability: capability.clone(),
            operator_id: operator_id.to_string(),
        };
        let experience = experience_store.get(&exp_key);
        let reputation = reputation_store.get(operator_id);

        let sample_size = experience.map(|e| e.sample_size).unwrap_or(0);
        let has_any_data = sample_size > 0 || reputation.is_some();

        // Base latency: prefer experience data, fall back to default
        let base_latency_ms = experience
            .map(|e| e.average_latency_ms)
            .unwrap_or(100.0);

        // Base cost: no cost tracking yet, use default
        let base_cost = 0.1;

        // Success probability: blend experience + reputation
        let exp_weight = if sample_size > 0 {
            (sample_size as f64 / (sample_size as f64 + 20.0)).min(0.8)
        } else {
            0.0
        };
        let rep_weight = reputation
            .map(|r| r.confidence * 0.5)
            .unwrap_or(0.0);
        let total_weight = exp_weight + rep_weight;

        let base_success_probability = if total_weight > 0.0 {
            let exp_contrib = experience
                .map(|e| e.success_rate * exp_weight)
                .unwrap_or(0.0);
            let rep_contrib = reputation
                .map(|r| r.reliability * rep_weight)
                .unwrap_or(0.0);
            ((exp_contrib + rep_contrib) / total_weight).clamp(0.01, 0.99)
        } else {
            0.5 // no data — maximally uncertain
        };

        // Base confidence: how much data supports this estimate
        let base_confidence = if has_any_data {
            let exp_conf = experience
                .map(|e| (e.sample_size as f64 / 100.0).min(1.0) * 0.6)
                .unwrap_or(0.0);
            let rep_conf = reputation
                .map(|r| r.confidence * 0.4)
                .unwrap_or(0.0);
            (exp_conf + rep_conf).min(1.0)
        } else {
            0.0
        };

        // Apply calibration offsets if available
        let (estimated_latency_ms, estimated_cost, estimated_success_probability, confidence) =
            if let Some(cal) = calibration {
                let off = cal.get(&exp_key);
                (
                    (base_latency_ms + off.latency_offset).max(1.0).round() as u64,
                    (base_cost + off.cost_offset).max(0.01),
                    (base_success_probability + off.success_offset).clamp(0.01, 0.99),
                    (base_confidence + off.confidence_offset).clamp(0.0, 1.0),
                )
            } else {
                (
                    base_latency_ms.round() as u64,
                    base_cost,
                    base_success_probability,
                    base_confidence,
                )
            };

        let estimated_risk = 1.0 - estimated_success_probability;

        CostEstimate {
            operator_id: operator_id.to_string(),
            estimated_latency_ms,
            estimated_cost,
            estimated_success_probability,
            estimated_risk,
            confidence,
        }
    }

    /// Rank candidates by expected utility for a capability.
    ///
    /// Calls `estimate()` for each candidate and sorts by
    /// expected success probability (descending), then by
    /// confidence (descending) as a tiebreaker.
    pub fn rank(
        capability: &Capability,
        candidates: &[String],
        experience_store: &ExperienceStore,
        reputation_store: &ReputationStore,
        calibration: Option<&CalibrationStore>,
    ) -> Vec<CostEstimate> {
        let mut estimates: Vec<CostEstimate> = candidates
            .iter()
            .map(|op| {
                Self::estimate(
                    capability,
                    op,
                    experience_store,
                    reputation_store,
                    calibration,
                )
            })
            .collect();

        estimates.sort_by(|a, b| {
            b.estimated_success_probability
                .partial_cmp(&a.estimated_success_probability)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then(
                    b.confidence
                        .partial_cmp(&a.confidence)
                        .unwrap_or(std::cmp::Ordering::Equal),
                )
        });

        estimates
    }

    /// Incorporate feedback from execution to improve future estimates.
    ///
    /// Updates per (Capability, Operator) calibration offsets using a
    /// bounded learning rate. Calibration is the **learning** layer —
    /// it mutates offsets in the CalibrationStore, never decision history.
    pub fn apply_feedback(
        feedback: &PlannerFeedback,
        calibration: &mut CalibrationStore,
    ) {
        calibration.apply_feedback(feedback);
    }

    /// Invalidate cached estimates for a (capability, operator) pair.
    ///
    /// Not yet implemented — reserved for future use.
    #[allow(unused_variables)]
    pub fn invalidate(
        capability: &Capability,
        operator_id: &str,
        calibration: &mut CalibrationStore,
    ) {
        todo!("Phase 5H: invalidation")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use friday_plugin_runtime::{
        Capability, CorrelationId, ExecutionPlan, PlanOutcome, PlanningRationale,
        SelectionContext,
    };
    use std::collections::HashMap;

    fn make_record(cap: Capability, operator: &str, success: bool) -> DecisionRecord {
        let cid = CorrelationId::new();
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        DecisionRecord {
            id: cid.clone(),
            timestamp_ms: now,
            capability: cap,
            context: SelectionContext::default(),
            parameters: HashMap::new(),
            plan: ExecutionPlan {
                plan_id: cid.clone(),
                version: 1,
                capability: Capability::ReadFile,
                operator_id: operator.into(),
                estimated_latency_ms: 10,
                estimated_cost: 0.1,
                estimated_success_probability: 0.9,
                estimated_risk: 0.1,
                confidence: 0.9,
                fallback_chain: Vec::new(),
                rationale: PlanningRationale {
                    candidates: Vec::new(),
                    selected_operator_id: operator.into(),
                    composite_score: 0.9,
                    explanation: "selected".into(),
                    correlation_id: cid,
                    evaluator_evidence: Vec::new(),
                },
            },
            outcome: PlanOutcome {
                latency_ms: 10,
                cost: 0.1,
                success,
                error: if success { None } else { Some("timeout".into()) },
                completed_steps: if success { 1 } else { 0 },
                cancelled: false,
                timeout: !success,
                retries: 0,
            },
            reflection: None,
        }
    }

    fn experience_for(operator: &str, sample_size: u64, success_rate: f64, since_ms: u64) -> Experience {
        Experience {
            key: ExperienceKey {
                capability: Capability::ReadFile,
                operator_id: operator.into(),
            },
            sample_size,
            success_rate,
            success_count: (sample_size as f64 * success_rate).round() as u64,
            failure_count: sample_size - (sample_size as f64 * success_rate).round() as u64,
            average_latency_ms: 10.0,
            p50_latency_ms: 0,
            p95_latency_ms: 0,
            p99_latency_ms: 0,
            last_execution_ms: None,
            last_failure_ms: None,
            last_failure_reason: None,
            consecutive_successes: 0,
            consecutive_failures: 0,
            since_ms,
        }
    }

    #[test]
    fn store_is_empty_on_creation() {
        let store = ExperienceStore::new();
        assert!(store.is_empty());
        assert_eq!(store.len(), 0);
    }

    #[test]
    fn engine_processes_records_incrementally() {
        let mut store = ExperienceStore::new();
        let mut engine = ExperienceEngine::new();

        let r1 = make_record(Capability::ReadFile, "fs-native", true);
        let r2 = make_record(Capability::ReadFile, "fs-native", true);
        let mut ledger = DecisionLedger::new();
        ledger.append(r1);
        ledger.append(r2);

        engine.update(&ledger, &mut store);

        let key = ExperienceKey {
            capability: Capability::ReadFile,
            operator_id: "fs-native".into(),
        };
        let exp = store.get(&key).unwrap();
        assert_eq!(exp.sample_size, 2);
        assert_eq!(exp.success_rate, 1.0);
        assert_eq!(exp.success_count, 2);
    }

    #[test]
    fn engine_processes_only_new_records() {
        let mut store = ExperienceStore::new();
        let mut engine = ExperienceEngine::new();

        let r1 = make_record(Capability::ReadFile, "fs-native", true);
        let mut ledger = DecisionLedger::new();
        ledger.append(r1);

        engine.update(&ledger, &mut store);
        assert_eq!(engine.last_processed(), 1);

        let r2 = make_record(Capability::ReadFile, "fs-native", false);
        ledger.append(r2);

        engine.update(&ledger, &mut store);

        let key = ExperienceKey {
            capability: Capability::ReadFile,
            operator_id: "fs-native".into(),
        };
        let exp = store.get(&key).unwrap();
        assert_eq!(exp.sample_size, 2);
        assert_eq!(exp.success_count, 1);
        assert_eq!(exp.failure_count, 1);
        assert!((exp.success_rate - 0.5).abs() < 0.001);
    }

    #[test]
    fn engine_tracks_last_failure() {
        let mut store = ExperienceStore::new();
        let mut engine = ExperienceEngine::new();

        let r1 = make_record(Capability::ReadFile, "fs-native", false);
        let r2 = make_record(Capability::ReadFile, "fs-native", true);
        let r3 = make_record(Capability::ReadFile, "fs-native", false);
        let mut ledger = DecisionLedger::new();
        ledger.append(r1);
        ledger.append(r2);
        ledger.append(r3);

        engine.update(&ledger, &mut store);

        let key = ExperienceKey {
            capability: Capability::ReadFile,
            operator_id: "fs-native".into(),
        };
        let exp = store.get(&key).unwrap();
        assert_eq!(exp.sample_size, 3);
        assert_eq!(exp.success_count, 1);
        assert_eq!(exp.failure_count, 2);
        assert!(exp.last_failure_ms.is_some());
        assert_eq!(exp.last_failure_reason.as_deref(), Some("timeout"));
        assert_eq!(exp.consecutive_failures, 1);
        assert_eq!(exp.consecutive_successes, 0);
    }

    #[test]
    fn engine_tracks_consecutive_successes() {
        let mut store = ExperienceStore::new();
        let mut engine = ExperienceEngine::new();
        let mut ledger = DecisionLedger::new();

        for _ in 0..5 {
            let r = make_record(Capability::ReadFile, "fs-native", true);
            ledger.append(r);
        }

        engine.update(&ledger, &mut store);

        let key = ExperienceKey {
            capability: Capability::ReadFile,
            operator_id: "fs-native".into(),
        };
        let exp = store.get(&key).unwrap();
        assert_eq!(exp.consecutive_successes, 5);
        assert_eq!(exp.consecutive_failures, 0);
    }

    #[test]
    fn engine_averages_latency() {
        let mut store = ExperienceStore::new();
        let mut engine = ExperienceEngine::new();

        let mut ledger = DecisionLedger::new();

        for i in 1..=4 {
            let mut r = make_record(Capability::ReadFile, "fs-native", true);
            r.outcome.latency_ms = i * 10;
            ledger.append(r);
        }

        engine.update(&ledger, &mut store);

        let key = ExperienceKey {
            capability: Capability::ReadFile,
            operator_id: "fs-native".into(),
        };
        let exp = store.get(&key).unwrap();
        assert!((exp.average_latency_ms - 25.0).abs() < 0.001);
    }

    #[test]
    fn engine_reset_reprocesses_all_records() {
        let mut store = ExperienceStore::new();
        let mut engine = ExperienceEngine::new();

        let r = make_record(Capability::ReadFile, "fs-native", true);
        let mut ledger = DecisionLedger::new();
        ledger.append(r);

        engine.update(&ledger, &mut store);

        // Reset and update again — should produce same result
        engine.reset();
        store.clear();

        engine.update(&ledger, &mut store);

        let key = ExperienceKey {
            capability: Capability::ReadFile,
            operator_id: "fs-native".into(),
        };
        let exp = store.get(&key).unwrap();
        assert_eq!(exp.sample_size, 1);
        assert_eq!(exp.success_count, 1);
    }

    #[test]
    fn engine_separates_operators() {
        let mut store = ExperienceStore::new();
        let mut engine = ExperienceEngine::new();

        let r1 = make_record(Capability::ReadFile, "fs-native", true);
        let r2 = make_record(Capability::ReadFile, "fs-remote", false);
        let mut ledger = DecisionLedger::new();
        ledger.append(r1);
        ledger.append(r2);

        engine.update(&ledger, &mut store);

        assert_eq!(store.len(), 2);

        let k1 = ExperienceKey {
            capability: Capability::ReadFile,
            operator_id: "fs-native".into(),
        };
        let exp1 = store.get(&k1).unwrap();
        assert_eq!(exp1.success_count, 1);
        assert_eq!(exp1.failure_count, 0);

        let k2 = ExperienceKey {
            capability: Capability::ReadFile,
            operator_id: "fs-remote".into(),
        };
        let exp2 = store.get(&k2).unwrap();
        assert_eq!(exp2.success_count, 0);
        assert_eq!(exp2.failure_count, 1);
    }

    #[test]
    fn reputation_store_is_empty_on_creation() {
        let store = ReputationStore::new();
        assert!(store.is_empty());
        assert_eq!(store.len(), 0);
    }

    #[test]
    fn reputation_engine_derives_from_experience() {
        let mut exp_store = ExperienceStore::new();
        let mut rep_store = ReputationStore::new();

        let exp = experience_for("op-a", 50, 0.96, 1000);
        exp_store.insert(exp);

        ReputationEngine::update(&exp_store, &mut rep_store);

        let rep = rep_store.get("op-a").unwrap();
        assert_eq!(rep.total_observations, 50);
        assert!((rep.reliability - 0.96).abs() < 0.001);
        assert!((rep.confidence - 0.5).abs() < 0.001);
        assert_eq!(rep.decay_factor, 0.95);
    }

    #[test]
    fn reputation_engine_aggregates_across_capabilities() {
        let mut exp_store = ExperienceStore::new();
        let mut rep_store = ReputationStore::new();

        let mut read_exp = experience_for("multi-op", 80, 0.95, 2000);
        read_exp.key.capability = Capability::ReadFile;
        exp_store.insert(read_exp);

        let mut write_exp = experience_for("multi-op", 20, 0.80, 1500);
        write_exp.key.capability = Capability::WriteFile;
        exp_store.insert(write_exp);

        ReputationEngine::update(&exp_store, &mut rep_store);

        let rep = rep_store.get("multi-op").unwrap();
        assert_eq!(rep.total_observations, 100);
        // weighted: (80 * 0.95 + 20 * 0.80) / 100 = (76 + 16) / 100 = 0.92
        assert!((rep.reliability - 0.92).abs() < 0.001);
        assert!((rep.confidence - 1.0).abs() < 0.001);
    }

    #[test]
    fn reputation_engine_separates_operators() {
        let mut exp_store = ExperienceStore::new();
        let mut rep_store = ReputationStore::new();

        exp_store.insert(experience_for("op-a", 10, 0.9, 1000));
        exp_store.insert(experience_for("op-b", 20, 0.8, 2000));

        ReputationEngine::update(&exp_store, &mut rep_store);

        assert_eq!(rep_store.len(), 2);

        let rep_a = rep_store.get("op-a").unwrap();
        assert_eq!(rep_a.total_observations, 10);

        let rep_b = rep_store.get("op-b").unwrap();
        assert_eq!(rep_b.total_observations, 20);
    }

    #[test]
    fn reputation_engine_empty_experience_produces_no_reputation() {
        let exp_store = ExperienceStore::new();
        let mut rep_store = ReputationStore::new();

        ReputationEngine::update(&exp_store, &mut rep_store);

        assert!(rep_store.is_empty());
    }

    #[test]
    fn reputation_engine_zero_observations_defaults() {
        let mut exp_store = ExperienceStore::new();
        let mut rep_store = ReputationStore::new();

        let exp = experience_for("zero-op", 0, 0.0, 3000);
        exp_store.insert(exp);

        ReputationEngine::update(&exp_store, &mut rep_store);

        let rep = rep_store.get("zero-op").unwrap();
        assert_eq!(rep.total_observations, 0);
        assert_eq!(rep.reliability, 0.0);
        assert_eq!(rep.confidence, 0.0);
    }

    #[test]
    fn engine_update_with_empty_ledger() {
        let mut store = ExperienceStore::new();
        let mut engine = ExperienceEngine::new();
        let ledger = DecisionLedger::new();

        engine.update(&ledger, &mut store);

        assert!(store.is_empty());
        assert_eq!(engine.last_processed(), 0);
    }

    // ── CostModel tests ────────────────────────────────────────────

    fn exp(
        key: ExperienceKey,
        sample_size: u64,
        success_rate: f64,
        average_latency_ms: f64,
    ) -> Experience {
        let success_count = (sample_size as f64 * success_rate).round() as u64;
        let failure_count = sample_size - success_count;
        Experience {
            key,
            sample_size,
            success_rate,
            success_count,
            failure_count,
            average_latency_ms,
            p50_latency_ms: average_latency_ms.round() as u64,
            p95_latency_ms: (average_latency_ms * 1.5).round() as u64,
            p99_latency_ms: (average_latency_ms * 2.0).round() as u64,
            last_execution_ms: Some(200),
            last_failure_ms: None,
            last_failure_reason: None,
            consecutive_successes: success_count,
            consecutive_failures: 0,
            since_ms: 100,
        }
    }

    fn rep(
        operator_id: &str,
        reliability: f64,
        confidence: f64,
        total_observations: u64,
    ) -> Reputation {
        Reputation {
            operator_id: operator_id.into(),
            reliability,
            confidence,
            total_observations,
            since_ms: 100,
            last_updated_ms: 200,
            decay_factor: 0.95,
        }
    }

    fn make_test_context() -> (ExperienceStore, ReputationStore) {
        let mut e_store = ExperienceStore::new();
        let mut r_store = ReputationStore::new();

        e_store.insert(exp(
            ExperienceKey {
                capability: Capability::ExecuteCommand,
                operator_id: "operator_a".into(),
            },
            80,
            0.9,
            45.0,
        ));

        r_store.insert(rep("operator_a", 0.85, 0.8, 80));

        (e_store, r_store)
    }

    #[test]
    fn estimate_with_no_data_returns_conservative_defaults() {
        let cap = Capability::Custom("unknown".into());
        let (e_store, r_store) = make_test_context();

        let est = CostModel::estimate(&cap, "unknown_op", &e_store, &r_store, None);

        assert_eq!(est.operator_id, "unknown_op");
        assert_eq!(est.estimated_latency_ms, 100);
        assert_eq!(est.estimated_cost, 0.1);
        assert_eq!(est.estimated_success_probability, 0.5);
        assert_eq!(est.estimated_risk, 0.5);
        assert_eq!(est.confidence, 0.0);
    }

    #[test]
    fn estimate_uses_experience_when_present() {
        let cap = Capability::ExecuteCommand;
        let (e_store, r_store) = make_test_context();

        let est = CostModel::estimate(&cap, "operator_a", &e_store, &r_store, None);

        assert_eq!(est.estimated_latency_ms, 45);
        assert!(est.estimated_success_probability > 0.5);
        assert!(est.confidence > 0.0);
    }

    #[test]
    fn estimate_uses_reputation_when_no_experience() {
        let cap = Capability::ReadFile;
        let (e_store, r_store) = make_test_context();

        let est = CostModel::estimate(&cap, "operator_a", &e_store, &r_store, None);

        assert_eq!(est.estimated_latency_ms, 100);
        assert_eq!(est.estimated_success_probability, 0.85);
        assert!(est.confidence > 0.0);
    }

    #[test]
    fn estimate_confidence_is_zero_without_any_data() {
        let cap = Capability::Custom("nope".into());
        let e_store = ExperienceStore::new();
        let r_store = ReputationStore::new();

        let est = CostModel::estimate(&cap, "nobody", &e_store, &r_store, None);

        assert_eq!(est.confidence, 0.0);
    }

    #[test]
    fn rank_orders_by_success_probability_descending() {
        let cap = Capability::SearchWeb;
        let (e_store, r_store) = {
            let mut e = ExperienceStore::new();
            let mut r = ReputationStore::new();

            e.insert(exp(
                ExperienceKey {
                    capability: cap.clone(),
                    operator_id: "operator_a".into(),
                },
                80,
                0.9,
                30.0,
            ));
            r.insert(rep("operator_a", 0.9, 0.8, 80));

            e.insert(exp(
                ExperienceKey {
                    capability: cap.clone(),
                    operator_id: "operator_b".into(),
                },
                10,
                0.6,
                50.0,
            ));
            r.insert(rep("operator_b", 0.6, 0.1, 10));

            (e, r)
        };

        let candidates = vec![
            "operator_a".into(),
            "operator_b".into(),
            "operator_c".into(),
        ];
        let ranked = CostModel::rank(&cap, &candidates, &e_store, &r_store, None);

        assert_eq!(ranked.len(), 3);
        assert_eq!(ranked[0].operator_id, "operator_a");
        assert_eq!(ranked[1].operator_id, "operator_b");
        assert_eq!(ranked[2].operator_id, "operator_c");
        assert!(ranked[0].estimated_success_probability > ranked[1].estimated_success_probability);
        assert!(ranked[0].confidence > ranked[1].confidence);
    }

    #[test]
    fn apply_feedback_updates_calibration_offsets() {
        let mut cal = CalibrationStore::new();
        let feedback = PlannerFeedback {
            capability: Capability::ExecuteCommand,
            operator_id: "op_test".into(),
            latency_error_ms: 50,
            cost_error: 0.3,
            success_bias: 0.2,
            latency_adjustment_ms: 50.0,
            cost_adjustment: 0.3,
            success_adjustment: 0.2,
            confidence_adjustment: 0.1,
            trigger_model_update: true,
            timestamp_ms: 1000,
        };

        CostModel::apply_feedback(&feedback, &mut cal);

        let key = ExperienceKey {
            capability: Capability::ExecuteCommand,
            operator_id: "op_test".into(),
        };
        let off = cal.get(&key);
        assert!(off.latency_offset > 0.0);
        assert!(off.cost_offset > 0.0);
        assert!(off.success_offset > 0.0);
        assert!(off.confidence_offset > 0.0);
    }

    #[test]
    fn estimate_applies_calibration_offsets() {
        let mut cal = CalibrationStore::new();
        let feedback = PlannerFeedback {
            capability: Capability::ExecuteCommand,
            operator_id: "op_test".into(),
            latency_error_ms: 0,
            cost_error: 0.0,
            success_bias: 0.0,
            latency_adjustment_ms: 100.0,
            cost_adjustment: -0.2,
            success_adjustment: 0.1,
            confidence_adjustment: 0.05,
            trigger_model_update: true,
            timestamp_ms: 1000,
        };
        CostModel::apply_feedback(&feedback, &mut cal);

        let (e_store, r_store) = make_test_context();
        let est = CostModel::estimate(
            &Capability::ExecuteCommand,
            "op_test",
            &e_store,
            &r_store,
            Some(&cal),
        );

        // Calibration offset exists but no matching experience for op_test
        // Base latency: 100 (no experience), offset: (0.3 * 100).clamp(-500, 500) = 30
        // So expected: 100 + 30 = 130, but latency is cast as u64 after offset
        // Let me compute: base_latency=100.0, offset = 0.3 * 100 = 30, total = 130, round = 130
        assert_eq!(est.estimated_latency_ms, 130);
    }

    #[test]
    fn calibration_offsets_bounded_to_prevent_oscillation() {
        let mut cal = CalibrationStore::new();

        // Repeated large overestimates should converge, not explode
        for _ in 0..100 {
            let feedback = PlannerFeedback {
                capability: Capability::ExecuteCommand,
                operator_id: "op_test".into(),
                latency_error_ms: 1000,
                cost_error: 5.0,
                success_bias: 0.0,
                latency_adjustment_ms: 1000.0,
                cost_adjustment: 5.0,
                success_adjustment: 0.0,
                confidence_adjustment: 0.0,
                trigger_model_update: true,
                timestamp_ms: 1000,
            };
            CostModel::apply_feedback(&feedback, &mut cal);
        }

        let key = ExperienceKey {
            capability: Capability::ExecuteCommand,
            operator_id: "op_test".into(),
        };
        let off = cal.get(&key);
        assert!(off.latency_offset <= 500.0, "latency offset clamped: {}", off.latency_offset);
        assert!(off.cost_offset <= 1.0, "cost offset clamped: {}", off.cost_offset);
    }

    #[test]
    fn calibration_store_starts_empty() {
        let cal = CalibrationStore::new();
        assert!(cal.is_empty());
        assert_eq!(cal.len(), 0);
    }

    #[test]
    fn invalidate_is_not_yet_implemented() {
        let cap = Capability::Custom("x".into());
        let mut cal = CalibrationStore::new();
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            CostModel::invalidate(&cap, "op", &mut cal);
        }));
        assert!(result.is_err());
    }
}
