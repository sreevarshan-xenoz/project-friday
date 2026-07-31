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
    Capability, CapabilityRequest, CorrelationId, CostEstimate, DecisionLedger, DecisionRecord,
    EvaluatorKind, ExecutionPlan, Experience, ExperienceKey, PlanOutcome, PlannerFeedback,
    Reputation,
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

    /// True when no calibration has been learned for this pair.
    pub fn is_zero(&self) -> bool {
        self.latency_offset == 0.0
            && self.cost_offset == 0.0
            && self.success_offset == 0.0
            && self.confidence_offset == 0.0
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

// ── Explainability (Sprint 6A) ─────────────────────────────────────────────────

/// Why a candidate was or wasn't selected.
#[derive(Debug, Clone)]
pub struct CandidateExplanation {
    pub operator_id: String,
    pub composite_score: f64,
    pub policy_bonus: f64,
    pub profile_match_score: f64,
    pub context_fit_score: f64,
    pub selected: bool,
    pub rejection_reason: Option<String>,
}

/// What per-capability history contributed to the decision.
#[derive(Debug, Clone)]
pub struct ExperienceFactor {
    pub sample_size: u64,
    pub success_rate: f64,
    pub average_latency_ms: f64,
    pub last_failure_reason: Option<String>,
}

/// What cross-capability trust contributed to the decision.
#[derive(Debug, Clone)]
pub struct ReputationFactor {
    pub reliability: f64,
    pub confidence: f64,
    pub total_observations: u64,
}

/// What calibration changed in the estimate.
#[derive(Debug, Clone)]
pub struct CalibrationFactor {
    pub latency_offset: f64,
    pub cost_offset: f64,
    pub success_offset: f64,
    pub confidence_offset: f64,
}

/// Structured, machine-readable reason a plan was selected.
///
/// Kept separate from the human-readable narrative so explanations
/// can be localized, filtered, and analyzed without parsing text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExplanationReason {
    /// Chosen because it scored highest on the composite score.
    HighestCompositeScore,
    /// A policy rule constrained or favored this selection.
    PolicyConstraint,
    /// Historical per-capability reliability favored this operator.
    HigherReliability,
    /// Historical per-capability latency favored this operator.
    BetterHistoricalLatency,
    /// Calibration offsets adjusted the estimate for this operator.
    CalibrationAdjustment,
    /// This operator was chosen via a fallback chain.
    FallbackSelection,
}

/// A structured explanation of why a plan was selected.
///
/// Answers: which operator, why, why not the others, which policy
/// rules fired, and what experience/reputation/calibration contributed.
#[derive(Debug, Clone)]
pub struct DecisionExplanation {
    pub plan_id: CorrelationId,
    pub capability: Capability,
    pub selected_operator_id: String,
    pub reasons: Vec<ExplanationReason>,
    pub candidates: Vec<CandidateExplanation>,
    pub policy_factors: Vec<String>,
    pub experience_factor: Option<ExperienceFactor>,
    pub reputation_factor: Option<ReputationFactor>,
    pub calibration_factor: Option<CalibrationFactor>,
    pub narrative: String,
}

/// Produces structured explanations for planning decisions.
///
/// Reads the plan's rationale (evaluator evidence + scored candidates)
/// and the current knowledge stores. Never mutates anything.
pub struct ExplanationEngine;

impl ExplanationEngine {
    /// Explain why a plan selected its operator.
    ///
    /// Pure read — no mutation of any store.
    pub fn explain(
        plan: &ExecutionPlan,
        experience_store: &ExperienceStore,
        reputation_store: &ReputationStore,
        calibration: &CalibrationStore,
    ) -> DecisionExplanation {
        let candidates: Vec<CandidateExplanation> = plan
            .rationale
            .candidates
            .iter()
            .map(|c| {
                let selected = c.plugin_id == plan.operator_id;
                let rejection_reason = if selected {
                    None
                } else {
                    Some(format!(
                        "composite score {:.3} vs selected {:.3}",
                        c.weighted_score, plan.rationale.composite_score
                    ))
                };
                CandidateExplanation {
                    operator_id: c.plugin_id.clone(),
                    composite_score: c.weighted_score,
                    policy_bonus: c.policy_bonus,
                    profile_match_score: c.profile_match_score,
                    context_fit_score: c.context_fit_score,
                    selected,
                    rejection_reason,
                }
            })
            .collect();

        let policy_factors: Vec<String> = plan
            .rationale
            .evaluator_evidence
            .iter()
            .filter(|e| e.source == EvaluatorKind::Policy)
            .map(|e| e.explanation.clone())
            .collect();

        let exp_key = ExperienceKey {
            capability: plan.capability.clone(),
            operator_id: plan.operator_id.clone(),
        };
        let experience_factor = experience_store.get(&exp_key).map(|e| ExperienceFactor {
            sample_size: e.sample_size,
            success_rate: e.success_rate,
            average_latency_ms: e.average_latency_ms,
            last_failure_reason: e.last_failure_reason.clone(),
        });

        let reputation_factor = reputation_store.get(&plan.operator_id).map(|r| {
            ReputationFactor {
                reliability: r.reliability,
                confidence: r.confidence,
                total_observations: r.total_observations,
            }
        });

        let calibration_factor = {
            let off = calibration.get(&exp_key);
            if off.latency_offset != 0.0
                || off.cost_offset != 0.0
                || off.success_offset != 0.0
                || off.confidence_offset != 0.0
            {
                Some(CalibrationFactor {
                    latency_offset: off.latency_offset,
                    cost_offset: off.cost_offset,
                    success_offset: off.success_offset,
                    confidence_offset: off.confidence_offset,
                })
            } else {
                None
            }
        };

        let mut reasons = vec![ExplanationReason::HighestCompositeScore];
        if !policy_factors.is_empty() {
            reasons.push(ExplanationReason::PolicyConstraint);
        }
        if let Some(ef) = &experience_factor {
            if ef.success_rate >= 0.5 {
                reasons.push(ExplanationReason::HigherReliability);
            }
            if ef.average_latency_ms < 100.0 {
                reasons.push(ExplanationReason::BetterHistoricalLatency);
            }
        }
        if calibration_factor.is_some() {
            reasons.push(ExplanationReason::CalibrationAdjustment);
        }

        let mut narrative_parts = vec![format!(
            "Selected {} for {} (composite {:.3}).",
            plan.operator_id,
            plan.capability.as_str(),
            plan.rationale.composite_score
        )];
        let rejected: Vec<&str> = candidates
            .iter()
            .filter(|c| !c.selected)
            .map(|c| c.operator_id.as_str())
            .collect();
        if !rejected.is_empty() {
            narrative_parts.push(format!("Rejected: {}.", rejected.join(", ")));
        }
        if !policy_factors.is_empty() {
            narrative_parts.push(format!("Policy: {}.", policy_factors.join("; ")));
        }
        if let Some(ef) = &experience_factor {
            narrative_parts.push(format!(
                "Experience: {} samples, {:.0}% success, {:.0}ms avg.",
                ef.sample_size, ef.success_rate * 100.0, ef.average_latency_ms
            ));
        }
        if let Some(rf) = &reputation_factor {
            narrative_parts.push(format!(
                "Reputation: {:.0}% reliable, {:.0}% confidence.",
                rf.reliability * 100.0, rf.confidence * 100.0
            ));
        }
        if let Some(cf) = &calibration_factor {
            narrative_parts.push(format!(
                "Calibration: latency {:.1}ms, success {:.3}.",
                cf.latency_offset, cf.success_offset
            ));
        }

        DecisionExplanation {
            plan_id: plan.plan_id.clone(),
            capability: plan.capability.clone(),
            selected_operator_id: plan.operator_id.clone(),
            reasons,
            candidates,
            policy_factors,
            experience_factor,
            reputation_factor,
            calibration_factor,
            narrative: narrative_parts.join(" "),
        }
    }
}

// ---------------------------------------------------------------------------
// Sprint 6B: Decision Timeline
// ---------------------------------------------------------------------------

/// A point-in-time snapshot of one full decision: request, plan, prediction,
/// outcome, and every derived read model.
///
/// The timeline owns its data — it stores snapshots, not references — so it
/// remains valid even if the live stores change after reconstruction.
#[derive(Debug, Clone)]
pub struct DecisionTimeline {
    /// The request that started the decision.
    pub correlation_id: CorrelationId,
    /// Reconstructed request that produced the plan.
    pub request: CapabilityRequest,
    /// The immutable plan that was executed.
    pub plan: ExecutionPlan,
    /// The prediction embedded in the plan.
    pub prediction: CostEstimate,
    /// The recorded outcome.
    pub outcome: PlanOutcome,
    /// The immutable ledger record for this decision.
    pub record: DecisionRecord,
    /// Experience snapshot for the (capability, operator) at build time.
    pub experience: Option<Experience>,
    /// Reputation snapshot for the operator at build time.
    pub reputation: Option<Reputation>,
    /// Feedback derived by comparing plan against outcome.
    pub feedback: PlannerFeedback,
    /// Calibration offsets for this capability/operator, if any.
    pub calibration: Option<CalibrationOffsets>,
    /// Structured explanation of why the plan was chosen.
    pub explanation: DecisionExplanation,
}

/// Reconstructs `DecisionTimeline` snapshots from existing state.
///
/// Reads: the ledger, the three derived stores, and the `ExplanationEngine`.
/// Owns no state of its own; reconstruction is a pure read model.
pub struct TimelineBuilder<'a> {
    ledger: &'a DecisionLedger,
    experience: &'a ExperienceStore,
    reputation: &'a ReputationStore,
    calibration: &'a CalibrationStore,
}

impl<'a> TimelineBuilder<'a> {
    pub fn new(
        ledger: &'a DecisionLedger,
        experience: &'a ExperienceStore,
        reputation: &'a ReputationStore,
        calibration: &'a CalibrationStore,
    ) -> Self {
        Self {
            ledger,
            experience,
            reputation,
            calibration,
        }
    }

    /// Reconstruct the timeline for a request `CorrelationId`.
    ///
    /// Matches the ledger record whose plan carries that correlation id
    /// (the request id flows into `plan.rationale.correlation_id` at plan
    /// time). Returns `None` when no record exists for the id.
    pub fn build(&self, correlation_id: &CorrelationId) -> Option<DecisionTimeline> {
        let record = self.ledger.iter().find(|r| {
            r.plan.rationale.correlation_id == *correlation_id
                || r.plan.plan_id == *correlation_id
        })?;

        let request = CapabilityRequest {
            capability: record.capability.clone(),
            parameters: record.parameters.clone(),
            permissions: Vec::new(),
            correlation_id: correlation_id.clone(),
            context: record.context.clone(),
        };

        let prediction = CostEstimate {
            operator_id: record.plan.operator_id.clone(),
            estimated_latency_ms: record.plan.estimated_latency_ms,
            estimated_cost: record.plan.estimated_cost,
            estimated_success_probability: record.plan.estimated_success_probability,
            estimated_risk: record.plan.estimated_risk,
            confidence: record.plan.confidence,
        };

        let key = ExperienceKey {
            capability: record.capability.clone(),
            operator_id: record.plan.operator_id.clone(),
        };
        let experience = self.experience.get(&key);
        let reputation = self.reputation.get(record.plan.operator_id.as_str());
        let offsets = self.calibration.get(&key);
        let calibration = if offsets.is_zero() {
            None
        } else {
            Some(offsets.clone())
        };

        let explanation = ExplanationEngine::explain(
            &record.plan,
            self.experience,
            self.reputation,
            self.calibration,
        );

        Some(DecisionTimeline {
            correlation_id: correlation_id.clone(),
            request,
            plan: record.plan.clone(),
            prediction,
            outcome: record.outcome.clone(),
            record: record.clone(),
            experience: experience.cloned(),
            reputation: reputation.cloned(),
            feedback: PlannerFeedback::compare(&record.plan, &record.outcome),
            calibration,
            explanation,
        })
    }

    /// Reconstruct timelines for every decision in the ledger, in order.
    pub fn build_all(&self) -> Vec<DecisionTimeline> {
        let ids: Vec<CorrelationId> = self
            .ledger
            .iter()
            .map(|r| r.plan.rationale.correlation_id.clone())
            .collect();
        ids.iter()
            .filter_map(|cid| self.build(cid))
            .collect()
    }
}

// ---------------------------------------------------------------------------
// Sprint 6C: Metrics & Observability
// ---------------------------------------------------------------------------

/// How well the CostModel predicted what actually happened.
#[derive(Debug, Clone, Default)]
pub struct PredictionAccuracy {
    /// Number of decisions evaluated.
    pub decisions: usize,
    /// Decisions where predicted success matched observed success.
    pub correct_predictions: usize,
    /// `correct_predictions / decisions` (0.0 when no decisions).
    pub accuracy: f64,
    /// Mean |estimated_latency − observed_latency| in ms.
    pub mean_latency_error_ms: f64,
    /// Mean |estimated_cost − observed_cost|.
    pub mean_cost_error: f64,
    /// Mean (estimated_success − observed_success). Positive = overconfident.
    pub mean_success_bias: f64,
}

/// Whether calibration is converging toward stability or oscillating.
#[derive(Debug, Clone, Default)]
pub struct CalibrationMetrics {
    /// Decisions whose plan carried learned (nonzero) calibration offsets.
    pub decisions_with_calibration: usize,
    /// Mean absolute latency offset in ms.
    pub mean_latency_offset: f64,
    /// Mean absolute cost offset.
    pub mean_cost_offset: f64,
    /// Mean absolute success offset.
    pub mean_success_offset: f64,
    /// Mean absolute confidence offset.
    pub mean_confidence_offset: f64,
    /// True when calibration magnitude shrank between the first and second
    /// half of the decisions (needs ≥ 2 calibrated decisions).
    pub converging: bool,
}

/// Planner effectiveness measured from plan fallback chains and outcomes.
#[derive(Debug, Clone, Default)]
pub struct FallbackMetrics {
    /// Number of decisions evaluated.
    pub decisions: usize,
    /// Decisions whose plan carried at least one fallback plan.
    pub decisions_with_fallback_chain: usize,
    /// Fraction of decisions that had a fallback safety net.
    pub fallback_rate: f64,
    /// Mean number of fallback plans attached per decision.
    pub avg_fallback_depth: f64,
    /// Decisions whose outcome failed.
    pub failures: usize,
    /// `failures / decisions` (0.0 when no decisions).
    pub failure_rate: f64,
}

/// Long-term trust coverage across operators.
#[derive(Debug, Clone, Default)]
pub struct ReputationMetrics {
    /// Number of decisions evaluated.
    pub decisions: usize,
    /// Decisions with a reputation snapshot at build time.
    pub decisions_with_reputation: usize,
    /// Fraction of decisions that had reputation data.
    pub coverage: f64,
    /// Mean reliability across decisions that had reputation.
    pub avg_reliability: f64,
    /// Mean confidence across decisions that had reputation.
    pub avg_confidence: f64,
    /// Total observations aggregated across all reputation snapshots.
    pub total_observations: u64,
    /// Distinct operators with reputation data.
    pub distinct_operators: usize,
}

/// How much historical experience exists for the (capability, operator) space.
#[derive(Debug, Clone, Default)]
pub struct ExperienceMetrics {
    /// Number of decisions evaluated.
    pub decisions: usize,
    /// Decisions with an experience snapshot at build time.
    pub decisions_with_experience: usize,
    /// Fraction of decisions that had historical experience.
    pub coverage: f64,
    /// Distinct (capability, operator) pairs with experience.
    pub distinct_pairs: usize,
    /// Total samples aggregated across all experience snapshots.
    pub total_samples: u64,
    /// Mean sample size across decisions that had experience.
    pub avg_sample_size: f64,
    /// Mean success rate across decisions that had experience.
    pub avg_success_rate: f64,
    /// Mean average latency (ms) across decisions that had experience.
    pub avg_latency_ms: f64,
}

/// Aggregated metrics for a set of reconstructed decisions.
#[derive(Debug, Clone)]
pub struct MetricsReport {
    pub decisions: usize,
    pub prediction: PredictionAccuracy,
    pub calibration: CalibrationMetrics,
    pub fallback: FallbackMetrics,
    pub reputation: ReputationMetrics,
    pub experience: ExperienceMetrics,
}

/// Stateless metric derivation.
///
/// Owns no state and mutates nothing — it is a pure read model over
/// `DecisionTimeline` snapshots. Metrics are derived, never stored.
pub struct MetricsEngine;

impl MetricsEngine {
    pub fn prediction_accuracy(timelines: &[DecisionTimeline]) -> PredictionAccuracy {
        let mut m = PredictionAccuracy {
            decisions: timelines.len(),
            ..Default::default()
        };
        if timelines.is_empty() {
            return m;
        }
        let mut latency_error_total = 0.0;
        let mut cost_error_total = 0.0;
        let mut bias_total = 0.0;
        for t in timelines {
            let predicted_success = t.plan.estimated_success_probability >= 0.5;
            if predicted_success == t.outcome.success {
                m.correct_predictions += 1;
            }
            let latency_error =
                (t.plan.estimated_latency_ms as i64 - t.outcome.latency_ms as i64).abs() as f64;
            latency_error_total += latency_error;
            cost_error_total += (t.plan.estimated_cost - t.outcome.cost).abs();
            bias_total += t.plan.estimated_success_probability - (t.outcome.success as u8 as f64);
        }
        let n = timelines.len() as f64;
        m.accuracy = m.correct_predictions as f64 / n;
        m.mean_latency_error_ms = latency_error_total / n;
        m.mean_cost_error = cost_error_total / n;
        m.mean_success_bias = bias_total / n;
        m
    }

    pub fn calibration(timelines: &[DecisionTimeline]) -> CalibrationMetrics {
        let mut m = CalibrationMetrics::default();
        let calibrated: Vec<&CalibrationOffsets> = timelines
            .iter()
            .filter_map(|t| t.calibration.as_ref())
            .collect();
        m.decisions_with_calibration = calibrated.len();
        if calibrated.is_empty() {
            return m;
        }
        let mut lat = 0.0;
        let mut cost = 0.0;
        let mut success = 0.0;
        let mut conf = 0.0;
        for o in &calibrated {
            lat += o.latency_offset.abs();
            cost += o.cost_offset.abs();
            success += o.success_offset.abs();
            conf += o.confidence_offset.abs();
        }
        let n = calibrated.len() as f64;
        m.mean_latency_offset = lat / n;
        m.mean_cost_offset = cost / n;
        m.mean_success_offset = success / n;
        m.mean_confidence_offset = conf / n;
        if calibrated.len() >= 2 {
            let mid = calibrated.len() / 2;
            let first: f64 = calibrated[..mid]
                .iter()
                .map(|o| o.latency_offset.abs())
                .sum::<f64>()
                / mid as f64;
            let second: f64 = calibrated[mid..]
                .iter()
                .map(|o| o.latency_offset.abs())
                .sum::<f64>()
                / (calibrated.len() - mid) as f64;
            m.converging = second < first;
        }
        m
    }

    pub fn fallback(timelines: &[DecisionTimeline]) -> FallbackMetrics {
        let mut m = FallbackMetrics {
            decisions: timelines.len(),
            ..Default::default()
        };
        if timelines.is_empty() {
            return m;
        }
        let mut depth_total = 0.0;
        for t in timelines {
            if !t.plan.fallback_chain.is_empty() {
                m.decisions_with_fallback_chain += 1;
                depth_total += t.plan.fallback_chain.len() as f64;
            }
            if !t.outcome.success {
                m.failures += 1;
            }
        }
        let n = timelines.len() as f64;
        m.fallback_rate = m.decisions_with_fallback_chain as f64 / n;
        m.avg_fallback_depth = depth_total / n;
        m.failure_rate = m.failures as f64 / n;
        m
    }

    pub fn reputation(timelines: &[DecisionTimeline]) -> ReputationMetrics {
        let mut m = ReputationMetrics {
            decisions: timelines.len(),
            ..Default::default()
        };
        let mut operators = std::collections::HashSet::new();
        let mut reliability_total = 0.0;
        let mut confidence_total = 0.0;
        for t in timelines {
            if let Some(r) = &t.reputation {
                m.decisions_with_reputation += 1;
                reliability_total += r.reliability;
                confidence_total += r.confidence;
                m.total_observations += r.total_observations;
                operators.insert(r.operator_id.clone());
            }
        }
        m.distinct_operators = operators.len();
        if m.decisions_with_reputation > 0 {
            let n = m.decisions_with_reputation as f64;
            m.avg_reliability = reliability_total / n;
            m.avg_confidence = confidence_total / n;
            m.coverage = m.decisions_with_reputation as f64 / m.decisions as f64;
        }
        m
    }

    pub fn experience(timelines: &[DecisionTimeline]) -> ExperienceMetrics {
        let mut m = ExperienceMetrics {
            decisions: timelines.len(),
            ..Default::default()
        };
        let mut keys = std::collections::HashSet::new();
        let mut samples_total = 0.0;
        let mut success_total = 0.0;
        let mut latency_total = 0.0;
        for t in timelines {
            if let Some(e) = &t.experience {
                m.decisions_with_experience += 1;
                keys.insert(e.key.clone());
                m.total_samples += e.sample_size;
                samples_total += e.sample_size as f64;
                success_total += e.success_rate;
                latency_total += e.average_latency_ms;
            }
        }
        m.distinct_pairs = keys.len();
        if m.decisions_with_experience > 0 {
            let n = m.decisions_with_experience as f64;
            m.coverage = m.decisions_with_experience as f64 / m.decisions as f64;
            m.avg_sample_size = samples_total / n;
            m.avg_success_rate = success_total / n;
            m.avg_latency_ms = latency_total / n;
        }
        m
    }

    pub fn summarize(timelines: &[DecisionTimeline]) -> MetricsReport {
        MetricsReport {
            decisions: timelines.len(),
            prediction: Self::prediction_accuracy(timelines),
            calibration: Self::calibration(timelines),
            fallback: Self::fallback(timelines),
            reputation: Self::reputation(timelines),
            experience: Self::experience(timelines),
        }
    }
}

// ---------------------------------------------------------------------------
// Sprint 6D: Replay Engine
// ---------------------------------------------------------------------------

/// How today's predictions differ from what was predicted historically.
#[derive(Debug, Clone)]
pub struct PredictionDelta {
    /// Replayed latency minus historical estimate, in ms.
    pub latency_delta_ms: i64,
    /// Replayed cost minus historical estimate.
    pub cost_delta: f64,
    /// Replayed success probability minus historical estimate.
    pub success_probability_delta: f64,
    /// Replayed confidence minus historical confidence.
    pub confidence_delta: f64,
}

/// The immutable record of what was decided and what actually happened.
#[derive(Debug, Clone)]
pub struct HistoricalSnapshot {
    /// The plan that was executed (immutable).
    pub plan: ExecutionPlan,
    /// The prediction embedded in that plan.
    pub prediction: CostEstimate,
    /// The explanation produced at decision time.
    pub explanation: DecisionExplanation,
    /// What actually happened.
    pub outcome: PlanOutcome,
}

/// Today's evaluation of the same decision against current read models.
#[derive(Debug, Clone)]
pub struct ReplayEvaluation {
    /// The same immutable plan, re-evaluated.
    pub plan: ExecutionPlan,
    /// Today's estimate for the historically selected operator.
    pub prediction: CostEstimate,
    /// Today's explanation (same plan, current experience/reputation/calibration).
    pub explanation: DecisionExplanation,
    /// Today's full ranking of the historical candidates, best first.
    pub ranked_candidates: Vec<CostEstimate>,
}

/// What changed between the historical decision and today's evaluation.
///
/// History itself never changes; these are the differences between two
/// evaluations of the same decision.
#[derive(Debug, Clone)]
pub struct ReplayDifferences {
    /// True when today's top-ranked candidate differs from the historical choice.
    pub operator_changed: bool,
    /// Regression signal: would the current model still make the same
    /// decision? Equal to `!operator_changed`. Tracks whether learning is
    /// improving decisions or destabilizing them.
    pub decision_still_optimal: bool,
    /// `replay.prediction − historical.prediction`, per dimension.
    pub prediction_delta: PredictionDelta,
    /// Current reputation reliability minus the historical snapshot
    /// (None unless both exist).
    pub reputation_delta: Option<f64>,
    /// Current calibration magnitude minus the historical snapshot
    /// (None only when neither has any calibration).
    pub calibration_delta: Option<f64>,
    /// Added (`+`) and removed (`−`) explanation reasons.
    pub explanation_delta: Vec<String>,
}

/// Result of evaluating one historical decision against current read models.
///
/// A self-contained comparison: immutable `historical` side, current `replay`
/// side, and a `differences` section emphasizing how the system has evolved.
/// History is never rewritten — replay is a read-only projection.
#[derive(Debug, Clone)]
pub struct ReplayReport {
    pub correlation_id: CorrelationId,
    pub capability: Capability,
    pub historical: HistoricalSnapshot,
    pub replay: ReplayEvaluation,
    pub differences: ReplayDifferences,
    pub replay_timestamp_ms: u64,
}

/// Evaluates historical decisions against the current state.
///
/// Builds on the existing read models (Timeline, ExplanationEngine, CostModel)
/// rather than recomputing pieces independently. Owns no state.
pub struct ReplayEngine;

impl ReplayEngine {
    pub fn replay(
        timeline: &DecisionTimeline,
        experience_store: &ExperienceStore,
        reputation_store: &ReputationStore,
        calibration: &CalibrationStore,
    ) -> ReplayReport {
        let correlation_id = timeline.correlation_id.clone();
        let capability = timeline.plan.capability.clone();
        let original_operator_id = timeline.plan.operator_id.clone();

        let candidate_ops: Vec<String> = if timeline.plan.rationale.candidates.is_empty() {
            vec![original_operator_id.clone()]
        } else {
            timeline
                .plan
                .rationale
                .candidates
                .iter()
                .map(|c| c.plugin_id.clone())
                .collect()
        };

        let ranked = CostModel::rank(
            &capability,
            &candidate_ops,
            experience_store,
            reputation_store,
            Some(calibration),
        );
        let ranked_candidates = ranked.clone();
        let best_prediction = ranked
            .into_iter()
            .next()
            .unwrap_or_else(|| {
                CostModel::estimate(
                    &capability,
                    &original_operator_id,
                    experience_store,
                    reputation_store,
                    Some(calibration),
                )
            });
        let replayed_operator_id = best_prediction.operator_id.clone();

        let replayed_prediction = CostModel::estimate(
            &capability,
            &original_operator_id,
            experience_store,
            reputation_store,
            Some(calibration),
        );

        let historical_prediction = CostEstimate {
            operator_id: original_operator_id.clone(),
            estimated_latency_ms: timeline.plan.estimated_latency_ms,
            estimated_cost: timeline.plan.estimated_cost,
            estimated_success_probability: timeline.plan.estimated_success_probability,
            estimated_risk: timeline.plan.estimated_risk,
            confidence: timeline.plan.confidence,
        };

        let operator_changed = replayed_operator_id != original_operator_id;

        let prediction_delta = PredictionDelta {
            latency_delta_ms: replayed_prediction.estimated_latency_ms as i64
                - historical_prediction.estimated_latency_ms as i64,
            cost_delta: replayed_prediction.estimated_cost - historical_prediction.estimated_cost,
            success_probability_delta: replayed_prediction.estimated_success_probability
                - historical_prediction.estimated_success_probability,
            confidence_delta: replayed_prediction.confidence - historical_prediction.confidence,
        };

        let snapshot_reliability = timeline.reputation.as_ref().map(|r| r.reliability);
        let current_reliability = reputation_store
            .get(original_operator_id.as_str())
            .map(|r| r.reliability);
        let reputation_delta = match (snapshot_reliability, current_reliability) {
            (Some(old), Some(new)) => Some(new - old),
            _ => None,
        };

        let key = ExperienceKey {
            capability: capability.clone(),
            operator_id: original_operator_id.clone(),
        };
        let current_offsets = calibration.get(&key);
        let snapshot_magnitude = timeline.calibration.as_ref().map(|o| o.latency_offset.abs());
        let current_magnitude = current_offsets.latency_offset.abs();
        let calibration_delta = if current_offsets.is_zero() && timeline.calibration.is_none() {
            None
        } else {
            Some(current_magnitude - snapshot_magnitude.unwrap_or(0.0))
        };

        let current_explanation =
            ExplanationEngine::explain(&timeline.plan, experience_store, reputation_store, calibration);
        let original_reasons = &timeline.explanation.reasons;
        let current_reasons = &current_explanation.reasons;
        let mut explanation_delta: Vec<String> = current_reasons
            .iter()
            .filter(|r| !original_reasons.contains(r))
            .map(|r| format!("+ {:?}", r))
            .collect();
        explanation_delta.extend(
            original_reasons
                .iter()
                .filter(|r| !current_reasons.contains(r))
                .map(|r| format!("- {:?}", r)),
        );

        ReplayReport {
            correlation_id,
            capability,
            historical: HistoricalSnapshot {
                plan: timeline.plan.clone(),
                prediction: historical_prediction,
                explanation: timeline.explanation.clone(),
                outcome: timeline.outcome.clone(),
            },
            replay: ReplayEvaluation {
                plan: timeline.plan.clone(),
                prediction: replayed_prediction,
                explanation: current_explanation,
                ranked_candidates,
            },
            differences: ReplayDifferences {
                operator_changed,
                decision_still_optimal: !operator_changed,
                prediction_delta,
                reputation_delta,
                calibration_delta,
                explanation_delta,
            },
            replay_timestamp_ms: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use friday_plugin_runtime::{
        Capability, CorrelationId, EvaluatorKind, EvaluatorSignal, ExecutionPlan, PlanOutcome,
        PlanningRationale, ScoredCandidate, SelectionContext,
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

    // ── ExplanationEngine tests ───────────────────────────────────────────

    fn explain_plan() -> ExecutionPlan {
        let cid = CorrelationId::new();
        let candidates = vec![
            ScoredCandidate {
                plugin_id: "op_primary".into(),
                plugin_name: "primary".into(),
                reliability_score: 0.9,
                availability_score: 0.9,
                latency_penalty: 0.1,
                cost_penalty: 0.1,
                policy_bonus: 0.05,
                profile_match_score: 0.8,
                context_fit_score: 0.8,
                weighted_score: 0.95,
            },
            ScoredCandidate {
                plugin_id: "op_backup".into(),
                plugin_name: "backup".into(),
                reliability_score: 0.6,
                availability_score: 0.6,
                latency_penalty: 0.2,
                cost_penalty: 0.2,
                policy_bonus: 0.0,
                profile_match_score: 0.5,
                context_fit_score: 0.5,
                weighted_score: 0.6,
            },
        ];
        ExecutionPlan {
            plan_id: cid.clone(),
            version: 1,
            capability: Capability::ReadFile,
            operator_id: "op_primary".into(),
            estimated_latency_ms: 10,
            estimated_cost: 0.1,
            estimated_success_probability: 0.9,
            estimated_risk: 0.1,
            confidence: 0.9,
            fallback_chain: Vec::new(),
            rationale: PlanningRationale {
                candidates: candidates.clone(),
                selected_operator_id: "op_primary".into(),
                composite_score: 0.95,
                explanation: "healthiest plugin".into(),
                correlation_id: cid,
                evaluator_evidence: vec![EvaluatorSignal {
                    source: EvaluatorKind::Policy,
                    score: 0.05,
                    confidence: 1.0,
                    explanation: "prefer_for ReadFile -> op_primary".into(),
                }],
            },
        }
    }

    #[test]
    fn explainer_identifies_selected_and_rejected() {
        let plan = explain_plan();
        let e_store = ExperienceStore::new();
        let r_store = ReputationStore::new();
        let cal = CalibrationStore::new();

        let exp = ExplanationEngine::explain(&plan, &e_store, &r_store, &cal);

        assert_eq!(exp.selected_operator_id, "op_primary");
        assert_eq!(exp.capability, Capability::ReadFile);
        assert_eq!(exp.candidates.len(), 2);
        assert!(exp.candidates[0].selected);
        assert!(!exp.candidates[1].selected);
        assert!(exp.candidates[1].rejection_reason.is_some());
        assert!(exp.candidates[0].rejection_reason.is_none());
    }

    #[test]
    fn explainer_captures_policy_factors() {
        let plan = explain_plan();
        let e_store = ExperienceStore::new();
        let r_store = ReputationStore::new();
        let cal = CalibrationStore::new();

        let exp = ExplanationEngine::explain(&plan, &e_store, &r_store, &cal);

        assert_eq!(exp.policy_factors.len(), 1);
        assert!(exp.policy_factors[0].contains("prefer_for"));
    }

    #[test]
    fn explainer_no_experience_or_reputation_when_absent() {
        let plan = explain_plan();
        let e_store = ExperienceStore::new();
        let r_store = ReputationStore::new();
        let cal = CalibrationStore::new();

        let exp = ExplanationEngine::explain(&plan, &e_store, &r_store, &cal);

        assert!(exp.experience_factor.is_none());
        assert!(exp.reputation_factor.is_none());
        assert!(exp.calibration_factor.is_none());
        assert!(exp.narrative.contains("Selected op_primary"));
        assert!(exp.narrative.contains("Rejected: op_backup"));
    }

    #[test]
    fn explainer_attaches_experience_and_reputation() {
        let plan = explain_plan();
        let mut e_store = ExperienceStore::new();
        let mut r_store = ReputationStore::new();

        e_store.insert(Experience {
            key: ExperienceKey {
                capability: Capability::ReadFile,
                operator_id: "op_primary".into(),
            },
            sample_size: 50,
            success_rate: 0.9,
            success_count: 45,
            failure_count: 5,
            average_latency_ms: 12.0,
            p50_latency_ms: 10,
            p95_latency_ms: 20,
            p99_latency_ms: 30,
            last_execution_ms: Some(500),
            last_failure_ms: None,
            last_failure_reason: None,
            consecutive_successes: 5,
            consecutive_failures: 0,
            since_ms: 100,
        });
        r_store.insert(Reputation {
            operator_id: "op_primary".into(),
            reliability: 0.85,
            confidence: 0.5,
            total_observations: 50,
            since_ms: 100,
            last_updated_ms: 500,
            decay_factor: 0.95,
        });

        let cal = CalibrationStore::new();
        let exp = ExplanationEngine::explain(&plan, &e_store, &r_store, &cal);

        let ef = exp.experience_factor.unwrap();
        assert_eq!(ef.sample_size, 50);
        assert_eq!(ef.success_rate, 0.9);
        assert_eq!(ef.average_latency_ms, 12.0);

        let rf = exp.reputation_factor.unwrap();
        assert_eq!(rf.reliability, 0.85);
        assert_eq!(rf.confidence, 0.5);

        assert!(exp.narrative.contains("50 samples"));
        assert!(exp.narrative.contains("Reputation: 85%"));
    }

    #[test]
    fn explainer_attaches_calibration_when_nonzero() {
        let plan = explain_plan();
        let e_store = ExperienceStore::new();
        let r_store = ReputationStore::new();
        let mut cal = CalibrationStore::new();

        let feedback = PlannerFeedback {
            capability: Capability::ReadFile,
            operator_id: "op_primary".into(),
            latency_error_ms: 0,
            cost_error: 0.0,
            success_bias: 0.0,
            latency_adjustment_ms: 50.0,
            cost_adjustment: 0.0,
            success_adjustment: 0.0,
            confidence_adjustment: 0.0,
            trigger_model_update: true,
            timestamp_ms: 1000,
        };
        CostModel::apply_feedback(&feedback, &mut cal);

        let exp = ExplanationEngine::explain(&plan, &e_store, &r_store, &cal);

        let cf = exp.calibration_factor.unwrap();
        assert!(cf.latency_offset > 0.0);
        assert!(exp.narrative.contains("Calibration"));
    }

    // -- 6B: Decision Timeline ---------------------------------------------

    fn timeline_feedback() -> PlannerFeedback {
        PlannerFeedback {
            capability: Capability::ReadFile,
            operator_id: "op_alpha".into(),
            latency_error_ms: 0,
            cost_error: 0.0,
            success_bias: 0.0,
            latency_adjustment_ms: 30.0,
            cost_adjustment: 0.0,
            success_adjustment: 0.0,
            confidence_adjustment: 0.0,
            trigger_model_update: true,
            timestamp_ms: 1000,
        }
    }

    fn timeline_experience() -> Experience {
        Experience {
            key: ExperienceKey {
                capability: Capability::ReadFile,
                operator_id: "op_alpha".into(),
            },
            sample_size: 30,
            success_rate: 0.9,
            success_count: 27,
            failure_count: 3,
            average_latency_ms: 11.0,
            p50_latency_ms: 10,
            p95_latency_ms: 18,
            p99_latency_ms: 22,
            last_execution_ms: Some(500),
            last_failure_ms: None,
            last_failure_reason: None,
            consecutive_successes: 9,
            consecutive_failures: 0,
            since_ms: 100,
        }
    }

    fn timeline_reputation() -> Reputation {
        Reputation {
            operator_id: "op_alpha".into(),
            reliability: 0.85,
            confidence: 0.5,
            total_observations: 30,
            since_ms: 100,
            last_updated_ms: 500,
            decay_factor: 0.95,
        }
    }

    #[test]
    fn timeline_reconstructs_full_decision() {
        let record = make_record(Capability::ReadFile, "op_alpha", true);
        let cid = record.plan.rationale.correlation_id.clone();
        let mut ledger = DecisionLedger::new();
        ledger.append(record.clone());

        let e = ExperienceStore::new();
        let r = ReputationStore::new();
        let c = CalibrationStore::new();
        let builder = TimelineBuilder::new(&ledger, &e, &r, &c);
        let timeline = builder.build(&cid).unwrap();

        assert_eq!(timeline.correlation_id, cid);
        assert_eq!(timeline.request.capability, record.capability);
        assert_eq!(timeline.request.correlation_id, cid);
        assert_eq!(timeline.plan.plan_id, record.plan.plan_id);
        assert_eq!(timeline.prediction.operator_id, "op_alpha");
        assert_eq!(timeline.prediction.estimated_latency_ms, 10);
        assert_eq!(timeline.prediction.estimated_success_probability, 0.9);
        assert!(timeline.outcome.success);
        assert_eq!(timeline.feedback.operator_id, "op_alpha");
        assert!(timeline.experience.is_none());
        assert!(timeline.reputation.is_none());
        assert!(timeline.calibration.is_none());
        assert_eq!(timeline.explanation.selected_operator_id, "op_alpha");
    }

    #[test]
    fn timeline_returns_none_for_unknown_correlation() {
        let ledger = DecisionLedger::new();
        let e = ExperienceStore::new();
        let r = ReputationStore::new();
        let c = CalibrationStore::new();
        let builder = TimelineBuilder::new(&ledger, &e, &r, &c);
        assert!(builder.build(&CorrelationId::new()).is_none());
    }

    #[test]
    fn timeline_build_all_reconstructs_every_record() {
        let mut ledger = DecisionLedger::new();
        ledger.append(make_record(Capability::ReadFile, "op_a", true));
        ledger.append(make_record(Capability::ReadFile, "op_b", false));
        ledger.append(make_record(Capability::ListDirectory, "op_a", true));

        let e = ExperienceStore::new();
        let r = ReputationStore::new();
        let c = CalibrationStore::new();
        let builder = TimelineBuilder::new(&ledger, &e, &r, &c);
        let timelines = builder.build_all();

        assert_eq!(timelines.len(), 3);
        assert_eq!(timelines[0].plan.operator_id, "op_a");
        assert_eq!(timelines[1].plan.operator_id, "op_b");
        assert_eq!(timelines[2].plan.operator_id, "op_a");
        assert!(timelines[1].feedback.success_bias > 0.0);
    }

    #[test]
    fn timeline_is_a_snapshot_that_survives_store_changes() {
        let record = make_record(Capability::ReadFile, "op_alpha", false);
        let cid = record.plan.rationale.correlation_id.clone();
        let mut ledger = DecisionLedger::new();
        ledger.append(record);

        let mut e_store = ExperienceStore::new();
        e_store.insert(timeline_experience());
        let mut r_store = ReputationStore::new();
        r_store.insert(timeline_reputation());
        let mut cal = CalibrationStore::new();
        CostModel::apply_feedback(&timeline_feedback(), &mut cal);

        let (exp_snapshot, rep_snapshot, cal_snapshot) = {
            let builder = TimelineBuilder::new(&ledger, &e_store, &r_store, &cal);
            let timeline = builder.build(&cid).unwrap();
            (
                timeline.experience.clone().unwrap(),
                timeline.reputation.clone().unwrap(),
                timeline.calibration.clone().unwrap(),
            )
        };

        e_store.insert(Experience {
            key: ExperienceKey {
                capability: Capability::ReadFile,
                operator_id: "op_alpha".into(),
            },
            sample_size: 1,
            success_rate: 0.0,
            success_count: 0,
            failure_count: 1,
            average_latency_ms: 99.0,
            p50_latency_ms: 99,
            p95_latency_ms: 99,
            p99_latency_ms: 99,
            last_execution_ms: Some(600),
            last_failure_ms: Some(600),
            last_failure_reason: Some("timeout".into()),
            consecutive_successes: 0,
            consecutive_failures: 1,
            since_ms: 100,
        });
        r_store.insert(Reputation {
            operator_id: "op_alpha".into(),
            reliability: 0.1,
            confidence: 0.5,
            total_observations: 1,
            since_ms: 100,
            last_updated_ms: 600,
            decay_factor: 0.95,
        });
        CostModel::apply_feedback(&timeline_feedback(), &mut cal);

        assert_eq!(exp_snapshot.success_rate, 0.9);
        assert_eq!(exp_snapshot.average_latency_ms, 11.0);
        assert_eq!(rep_snapshot.reliability, 0.85);
        let stored_cal = cal_snapshot.latency_offset;

        let builder2 = TimelineBuilder::new(&ledger, &e_store, &r_store, &cal);
        let timeline2 = builder2.build(&cid).unwrap();
        assert!(timeline2.experience.unwrap().success_rate < 0.5);
        assert!(timeline2.reputation.unwrap().reliability < 0.5);
        assert!(timeline2.calibration.unwrap().latency_offset > stored_cal);
    }

    #[test]
    fn timeline_attaches_calibration_when_learned() {
        let record = make_record(Capability::ReadFile, "op_alpha", true);
        let cid = record.plan.rationale.correlation_id.clone();
        let mut ledger = DecisionLedger::new();
        ledger.append(record);

        let mut cal = CalibrationStore::new();
        CostModel::apply_feedback(&timeline_feedback(), &mut cal);

        let e = ExperienceStore::new();
        let r = ReputationStore::new();
        let builder = TimelineBuilder::new(&ledger, &e, &r, &cal);
        let timeline = builder.build(&cid).unwrap();

        let cf = timeline.calibration.unwrap();
        assert!(cf.latency_offset > 0.0);
        assert!(timeline.explanation.calibration_factor.is_some());
        assert!(timeline
            .explanation
            .reasons
            .contains(&ExplanationReason::CalibrationAdjustment));
    }

    #[test]
    fn timeline_explanation_contains_structured_reasons() {
        let plan = explain_plan();
        let cid = plan.rationale.correlation_id.clone();
        let record = DecisionRecord {
            id: plan.plan_id.clone(),
            timestamp_ms: 1000,
            capability: Capability::ReadFile,
            context: SelectionContext::default(),
            parameters: HashMap::new(),
            plan,
            outcome: PlanOutcome {
                latency_ms: 10,
                cost: 0.1,
                success: true,
                error: None,
                completed_steps: 1,
                cancelled: false,
                timeout: false,
                retries: 0,
            },
            reflection: None,
        };
        let mut ledger = DecisionLedger::new();
        ledger.append(record);

        let e = ExperienceStore::new();
        let r = ReputationStore::new();
        let c = CalibrationStore::new();
        let builder = TimelineBuilder::new(&ledger, &e, &r, &c);
        let timeline = builder.build(&cid).unwrap();

        assert_eq!(timeline.explanation.candidates.len(), 2);
        assert!(timeline
            .explanation
            .reasons
            .contains(&ExplanationReason::HighestCompositeScore));
        assert!(timeline
            .explanation
            .reasons
            .contains(&ExplanationReason::PolicyConstraint));
    }

    // -- 6C: Metrics --------------------------------------------------------

    fn metrics_record(
        op: &str,
        estimated_success: f64,
        actual_success: bool,
        est_latency: u64,
        actual_latency: u64,
        fallbacks: usize,
    ) -> DecisionRecord {
        let cid = CorrelationId::new();
        let mut fallback_chain = Vec::new();
        for i in 0..fallbacks {
            let fcid = CorrelationId::new();
            fallback_chain.push(ExecutionPlan {
                plan_id: fcid.clone(),
                version: 1,
                capability: Capability::ReadFile,
                operator_id: format!("fb_{}", i),
                estimated_latency_ms: est_latency,
                estimated_cost: 0.1,
                estimated_success_probability: estimated_success,
                estimated_risk: 0.1,
                confidence: 0.5,
                fallback_chain: Vec::new(),
                rationale: PlanningRationale {
                    candidates: Vec::new(),
                    selected_operator_id: format!("fb_{}", i),
                    composite_score: 0.5,
                    explanation: "fallback".into(),
                    correlation_id: fcid,
                    evaluator_evidence: Vec::new(),
                },
            });
        }
        DecisionRecord {
            id: cid.clone(),
            timestamp_ms: 1000,
            capability: Capability::ReadFile,
            context: SelectionContext::default(),
            parameters: HashMap::new(),
            plan: ExecutionPlan {
                plan_id: cid.clone(),
                version: 1,
                capability: Capability::ReadFile,
                operator_id: op.into(),
                estimated_latency_ms: est_latency,
                estimated_cost: 0.1,
                estimated_success_probability: estimated_success,
                estimated_risk: 0.1,
                confidence: 0.9,
                fallback_chain,
                rationale: PlanningRationale {
                    candidates: Vec::new(),
                    selected_operator_id: op.into(),
                    composite_score: 0.9,
                    explanation: "selected".into(),
                    correlation_id: cid,
                    evaluator_evidence: Vec::new(),
                },
            },
            outcome: PlanOutcome {
                latency_ms: actual_latency,
                cost: 0.1,
                success: actual_success,
                error: if actual_success {
                    None
                } else {
                    Some("timeout".into())
                },
                completed_steps: if actual_success { 1 } else { 0 },
                cancelled: false,
                timeout: !actual_success,
                retries: 0,
            },
            reflection: None,
        }
    }

    fn metrics_timelines(records: Vec<DecisionRecord>) -> Vec<DecisionTimeline> {
        let mut ledger = DecisionLedger::new();
        for r in records {
            ledger.append(r);
        }
        let e = ExperienceStore::new();
        let r = ReputationStore::new();
        let c = CalibrationStore::new();
        TimelineBuilder::new(&ledger, &e, &r, &c).build_all()
    }

    #[test]
    fn metrics_prediction_accuracy_matches_outcomes() {
        let timelines = metrics_timelines(vec![
            metrics_record("op_a", 0.9, true, 10, 10, 0),
            metrics_record("op_a", 0.9, false, 10, 50, 0),
            metrics_record("op_a", 0.2, false, 10, 10, 0),
        ]);
        let m = MetricsEngine::prediction_accuracy(&timelines);

        assert_eq!(m.decisions, 3);
        assert_eq!(m.correct_predictions, 2);
        assert!((m.accuracy - 2.0 / 3.0).abs() < 1e-9);
        assert!((m.mean_latency_error_ms - (0.0 + 40.0 + 0.0) / 3.0).abs() < 1e-9);
        assert!(m.mean_success_bias > 0.0);
    }

    #[test]
    fn metrics_empty_timelines_produce_zero_metrics() {
        let timelines: Vec<DecisionTimeline> = Vec::new();
        let report = MetricsEngine::summarize(&timelines);

        assert_eq!(report.decisions, 0);
        assert_eq!(report.prediction.accuracy, 0.0);
        assert_eq!(report.calibration.decisions_with_calibration, 0);
        assert!(!report.calibration.converging);
        assert_eq!(report.fallback.fallback_rate, 0.0);
        assert_eq!(report.reputation.coverage, 0.0);
        assert_eq!(report.experience.coverage, 0.0);
    }

    #[test]
    fn metrics_calibration_magnitude_and_convergence() {
        let mut ledger = DecisionLedger::new();
        let first = metrics_record("op_alpha", 0.9, true, 10, 10, 0);
        let second = metrics_record("op_alpha", 0.9, true, 10, 10, 0);
        let first_cid = first.plan.rationale.correlation_id.clone();
        let second_cid = second.plan.rationale.correlation_id.clone();
        ledger.append(first);
        ledger.append(second);

        let mut cal = CalibrationStore::new();
        let feedback = |adj: f64| PlannerFeedback {
            capability: Capability::ReadFile,
            operator_id: "op_alpha".into(),
            latency_error_ms: 0,
            cost_error: 0.0,
            success_bias: 0.0,
            latency_adjustment_ms: adj,
            cost_adjustment: 0.0,
            success_adjustment: 0.0,
            confidence_adjustment: 0.0,
            trigger_model_update: true,
            timestamp_ms: 1000,
        };

        CostModel::apply_feedback(&feedback(100.0), &mut cal);
        let e = ExperienceStore::new();
        let r = ReputationStore::new();
        let builder = TimelineBuilder::new(&ledger, &e, &r, &cal);
        let tl0 = builder.build(&first_cid).unwrap();

        CostModel::apply_feedback(&feedback(-50.0), &mut cal);
        let builder2 = TimelineBuilder::new(&ledger, &e, &r, &cal);
        let tl1 = builder2.build(&second_cid).unwrap();

        let m = MetricsEngine::calibration(&[tl0, tl1]);
        assert_eq!(m.decisions_with_calibration, 2);
        assert!(m.mean_latency_offset > 0.0);
        assert!(m.converging);
    }

    #[test]
    fn metrics_fallback_rate_counts_chains_and_failures() {
        let timelines = metrics_timelines(vec![
            metrics_record("op_a", 0.9, true, 10, 10, 2),
            metrics_record("op_a", 0.9, false, 10, 10, 0),
            metrics_record("op_a", 0.2, false, 10, 10, 1),
        ]);
        let m = MetricsEngine::fallback(&timelines);

        assert_eq!(m.decisions, 3);
        assert_eq!(m.decisions_with_fallback_chain, 2);
        assert!((m.fallback_rate - 2.0 / 3.0).abs() < 1e-9);
        assert!((m.avg_fallback_depth - 1.0).abs() < 1e-9);
        assert_eq!(m.failures, 2);
        assert!((m.failure_rate - 2.0 / 3.0).abs() < 1e-9);
    }

    #[test]
    fn metrics_reputation_and_experience_coverage() {
        let mut ledger = DecisionLedger::new();
        ledger.append(metrics_record("op_alpha", 0.9, true, 10, 10, 0));
        ledger.append(metrics_record("op_beta", 0.9, true, 10, 10, 0));

        let mut e_store = ExperienceStore::new();
        e_store.insert(Experience {
            key: ExperienceKey {
                capability: Capability::ReadFile,
                operator_id: "op_alpha".into(),
            },
            sample_size: 40,
            success_rate: 0.8,
            success_count: 32,
            failure_count: 8,
            average_latency_ms: 12.0,
            p50_latency_ms: 11,
            p95_latency_ms: 20,
            p99_latency_ms: 25,
            last_execution_ms: Some(500),
            last_failure_ms: Some(300),
            last_failure_reason: Some("timeout".into()),
            consecutive_successes: 4,
            consecutive_failures: 1,
            since_ms: 100,
        });
        let mut r_store = ReputationStore::new();
        r_store.insert(Reputation {
            operator_id: "op_alpha".into(),
            reliability: 0.7,
            confidence: 0.6,
            total_observations: 40,
            since_ms: 100,
            last_updated_ms: 500,
            decay_factor: 0.95,
        });
        let cal = CalibrationStore::new();
        let timelines = TimelineBuilder::new(&ledger, &e_store, &r_store, &cal).build_all();

        let exp = MetricsEngine::experience(&timelines);
        assert_eq!(exp.decisions, 2);
        assert_eq!(exp.decisions_with_experience, 1);
        assert!((exp.coverage - 0.5).abs() < 1e-9);
        assert_eq!(exp.distinct_pairs, 1);
        assert_eq!(exp.total_samples, 40);
        assert!((exp.avg_success_rate - 0.8).abs() < 1e-9);
        assert!((exp.avg_latency_ms - 12.0).abs() < 1e-9);

        let rep = MetricsEngine::reputation(&timelines);
        assert_eq!(rep.decisions_with_reputation, 1);
        assert!((rep.coverage - 0.5).abs() < 1e-9);
        assert_eq!(rep.distinct_operators, 1);
        assert_eq!(rep.total_observations, 40);
        assert!((rep.avg_reliability - 0.7).abs() < 1e-9);
        assert!((rep.avg_confidence - 0.6).abs() < 1e-9);
    }

    #[test]
    fn metrics_summarize_builds_full_report() {
        let timelines =
            metrics_timelines(vec![metrics_record("op_a", 0.9, true, 10, 10, 1)]);
        let report = MetricsEngine::summarize(&timelines);

        assert_eq!(report.decisions, 1);
        assert_eq!(report.prediction.accuracy, 1.0);
        assert_eq!(report.fallback.decisions, 1);
        assert_eq!(report.fallback.decisions_with_fallback_chain, 1);
        assert_eq!(report.reputation.decisions, 1);
        assert_eq!(report.experience.decisions, 1);
    }

    // -- 6D: Replay Engine --------------------------------------------------

    fn replay_setup() -> (DecisionLedger, CorrelationId) {
        let plan = explain_plan();
        let cid = plan.rationale.correlation_id.clone();
        let record = DecisionRecord {
            id: plan.plan_id.clone(),
            timestamp_ms: 1000,
            capability: Capability::ReadFile,
            context: SelectionContext::default(),
            parameters: HashMap::new(),
            plan,
            outcome: PlanOutcome {
                latency_ms: 10,
                cost: 0.1,
                success: true,
                error: None,
                completed_steps: 1,
                cancelled: false,
                timeout: false,
                retries: 0,
            },
            reflection: None,
        };
        let mut ledger = DecisionLedger::new();
        ledger.append(record);
        (ledger, cid)
    }

    fn replay_experience(op: &str, success_rate: f64, latency: f64) -> Experience {
        Experience {
            key: ExperienceKey {
                capability: Capability::ReadFile,
                operator_id: op.into(),
            },
            sample_size: 100,
            success_rate,
            success_count: (success_rate * 100.0) as u64,
            failure_count: 100 - (success_rate * 100.0) as u64,
            average_latency_ms: latency,
            p50_latency_ms: latency as u64,
            p95_latency_ms: latency as u64 + 10,
            p99_latency_ms: latency as u64 + 20,
            last_execution_ms: Some(500),
            last_failure_ms: None,
            last_failure_reason: None,
            consecutive_successes: 5,
            consecutive_failures: 0,
            since_ms: 100,
        }
    }

    fn replay_feedback(adjustment: f64) -> PlannerFeedback {
        PlannerFeedback {
            capability: Capability::ReadFile,
            operator_id: "op_primary".into(),
            latency_error_ms: 0,
            cost_error: 0.0,
            success_bias: 0.0,
            latency_adjustment_ms: adjustment,
            cost_adjustment: 0.0,
            success_adjustment: 0.0,
            confidence_adjustment: 0.0,
            trigger_model_update: true,
            timestamp_ms: 1000,
        }
    }

    #[test]
    fn replay_unchanged_when_model_identical() {
        let (ledger, cid) = replay_setup();
        let e = ExperienceStore::new();
        let r = ReputationStore::new();
        let c = CalibrationStore::new();
        let timeline = TimelineBuilder::new(&ledger, &e, &r, &c).build(&cid).unwrap();
        let report = ReplayEngine::replay(&timeline, &e, &r, &c);

        assert_eq!(report.historical.plan.operator_id, "op_primary");
        assert_eq!(report.replay.ranked_candidates[0].operator_id, "op_primary");
        assert!(!report.differences.operator_changed);
        assert!(report.differences.decision_still_optimal);
        assert!(report.differences.explanation_delta.is_empty());
        assert!(report.differences.reputation_delta.is_none());
        assert!(report.differences.calibration_delta.is_none());
        assert!(report.historical.outcome.success);
    }

    #[test]
    fn replay_detects_operator_change() {
        let (ledger, cid) = replay_setup();
        let mut e = ExperienceStore::new();
        e.insert(replay_experience("op_backup", 0.95, 8.0));
        e.insert(replay_experience("op_primary", 0.2, 90.0));
        let r = ReputationStore::new();
        let c = CalibrationStore::new();
        let timeline = TimelineBuilder::new(&ledger, &e, &r, &c).build(&cid).unwrap();
        let report = ReplayEngine::replay(&timeline, &e, &r, &c);

        assert!(report.differences.operator_changed);
        assert!(!report.differences.decision_still_optimal);
        assert_eq!(report.replay.ranked_candidates[0].operator_id, "op_backup");
        assert!(report.replay.ranked_candidates[0].estimated_success_probability > 0.5);
    }

    #[test]
    fn decision_still_optimal_reflects_operator_change() {
        let (ledger, cid) = replay_setup();
        let r = ReputationStore::new();
        let c = CalibrationStore::new();

        let mut unchanged = ExperienceStore::new();
        unchanged.insert(replay_experience("op_primary", 0.95, 8.0));
        let unchanged_timeline =
            TimelineBuilder::new(&ledger, &unchanged, &r, &c).build(&cid).unwrap();
        let report = ReplayEngine::replay(&unchanged_timeline, &unchanged, &r, &c);
        assert!(!report.differences.operator_changed);
        assert!(report.differences.decision_still_optimal);

        let mut changed = ExperienceStore::new();
        changed.insert(replay_experience("op_backup", 0.95, 8.0));
        changed.insert(replay_experience("op_primary", 0.2, 90.0));
        let changed_timeline = TimelineBuilder::new(&ledger, &changed, &r, &c).build(&cid).unwrap();
        let report = ReplayEngine::replay(&changed_timeline, &changed, &r, &c);
        assert!(report.differences.operator_changed);
        assert!(!report.differences.decision_still_optimal);
        assert_eq!(
            report.differences.decision_still_optimal,
            !report.differences.operator_changed
        );
    }

    #[test]
    fn replay_reports_prediction_deltas() {
        let (ledger, cid) = replay_setup();
        let mut e = ExperienceStore::new();
        e.insert(replay_experience("op_primary", 0.2, 50.0));
        let r = ReputationStore::new();
        let c = CalibrationStore::new();
        let timeline = TimelineBuilder::new(&ledger, &e, &r, &c).build(&cid).unwrap();
        let report = ReplayEngine::replay(&timeline, &e, &r, &c);

        assert_eq!(report.differences.prediction_delta.latency_delta_ms, 40);
        assert!(report.differences.prediction_delta.success_probability_delta < 0.0);
        assert!(report.differences.prediction_delta.cost_delta.abs() < 1e-9);
        assert_eq!(report.historical.prediction.estimated_latency_ms, 10);
        assert_eq!(report.replay.prediction.estimated_latency_ms, 50);
    }

    #[test]
    fn replay_reports_reputation_delta() {
        let (ledger, cid) = replay_setup();
        let e = ExperienceStore::new();
        let mut r = ReputationStore::new();
        r.insert(Reputation {
            operator_id: "op_primary".into(),
            reliability: 0.5,
            confidence: 0.4,
            total_observations: 20,
            since_ms: 100,
            last_updated_ms: 500,
            decay_factor: 0.95,
        });
        let c = CalibrationStore::new();
        let timeline = TimelineBuilder::new(&ledger, &e, &r, &c).build(&cid).unwrap();
        r.insert(Reputation {
            operator_id: "op_primary".into(),
            reliability: 0.8,
            confidence: 0.6,
            total_observations: 30,
            since_ms: 100,
            last_updated_ms: 900,
            decay_factor: 0.95,
        });
        let report = ReplayEngine::replay(&timeline, &e, &r, &c);

        let delta = report
            .differences
            .reputation_delta
            .expect("reputation delta present");
        assert!((delta - 0.3).abs() < 1e-9);
    }

    #[test]
    fn replay_reports_calibration_delta() {
        let (ledger, cid) = replay_setup();
        let e = ExperienceStore::new();
        let r = ReputationStore::new();
        let mut c = CalibrationStore::new();
        CostModel::apply_feedback(&replay_feedback(100.0), &mut c);
        let timeline = TimelineBuilder::new(&ledger, &e, &r, &c).build(&cid).unwrap();
        CostModel::apply_feedback(&replay_feedback(-50.0), &mut c);
        let report = ReplayEngine::replay(&timeline, &e, &r, &c);

        let delta = report
            .differences
            .calibration_delta
            .expect("calibration delta present");
        assert!((delta + 15.0).abs() < 1e-6);
    }

    #[test]
    fn replay_detects_explanation_change() {
        let (ledger, cid) = replay_setup();
        let e = ExperienceStore::new();
        let r = ReputationStore::new();
        let mut c = CalibrationStore::new();
        let timeline = TimelineBuilder::new(&ledger, &e, &r, &c).build(&cid).unwrap();
        CostModel::apply_feedback(&replay_feedback(100.0), &mut c);
        let report = ReplayEngine::replay(&timeline, &e, &r, &c);

        assert!(!report.differences.explanation_delta.is_empty());
        assert!(report
            .differences
            .explanation_delta
            .iter()
            .any(|d| d.contains("CalibrationAdjustment")));
        assert!(report.differences.calibration_delta.is_some());
    }
}
