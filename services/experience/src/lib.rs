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
//! ```
//!
//! ## Invariants
//!
//! - Experience is derived exclusively from DecisionRecords.
//! - The ExperienceEngine uses a cursor (`last_processed`) so each record
//!   is processed exactly once.
//! - Reputation is derived exclusively from Experience.
//! - No scheduling, no background threads, no async.

use std::collections::HashMap;

use friday_plugin_runtime::{
    DecisionLedger, DecisionRecord, Experience, ExperienceKey, Reputation,
};

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
}
