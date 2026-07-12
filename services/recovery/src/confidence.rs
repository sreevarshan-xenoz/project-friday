//! Learning Recovery Confidence.
//!
//! Base confidence (`RecoveryAction::confidence`) is a prior. The
//! [`ConfidenceModel`] records actual outcomes of each recovery step — as rich
//! [`RecoveryObservation`]s, not just booleans — and blends them with the prior,
//! so confidence *adjusts automatically* as evidence accumulates (the
//! "Outcome → Experience → Updated Confidence" loop). Storing latency and
//! interruption lets us later ask *which recoveries are slow?* and *which
//! recoveries annoy users?*:

use serde::{Deserialize, Serialize};

use crate::taxonomy::{FailureCategory, RecoveryAction};

/// One observed recovery attempt. Stored (not aggregated away) so future
/// analysis can slice by latency, interruption, subsystem, etc.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecoveryObservation {
    pub subsystem: String,
    pub action: String,
    pub category: FailureCategory,
    pub chosen_step: RecoveryAction,
    /// Confidence predicted *before* the outcome was known.
    pub predicted_confidence: f64,
    /// Whether the task ultimately succeeded.
    pub outcome: bool,
    /// Wall-clock cost of the recovery, milliseconds.
    pub latency_ms: f64,
    /// True if a human interrupted / cancelled the recovery.
    pub user_interrupted: bool,
}

/// Accumulates [`RecoveryObservation`]s and yields a learned confidence per
/// (subsystem, action, category, step). Persisted across runs inside
/// `BenchmarkDay` so the benchmark compounds evidence over time.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfidenceModel {
    observations: Vec<RecoveryObservation>,
    /// Trials below this count fall back to the base prior (with shrinkage).
    min_samples: u32,
    /// Weight of the base prior relative to observed trials (Laplace-style).
    prior_weight: f64,
}

impl Default for ConfidenceModel {
    fn default() -> Self {
        Self::new()
    }
}

impl ConfidenceModel {
    pub fn new() -> Self {
        Self {
            observations: Vec::new(),
            min_samples: 20,
            prior_weight: 5.0,
        }
    }

    /// Convenience: record a simple outcome (latency 0, no interruption, the
    /// subsystem inferred from the action prefix).
    pub fn record(
        &mut self,
        action: &str,
        category: FailureCategory,
        step: &RecoveryAction,
        success: bool,
    ) {
        let subsystem = action.split('.').next().unwrap_or("unknown").to_string();
        self.record_observation(RecoveryObservation {
            subsystem,
            action: action.to_string(),
            category,
            chosen_step: step.clone(),
            predicted_confidence: step.confidence(),
            outcome: success,
            latency_ms: 0.0,
            user_interrupted: false,
        });
    }

    /// Record a fully-specified observation.
    pub fn record_observation(&mut self, obs: RecoveryObservation) {
        self.observations.push(obs);
    }

    fn matching(
        &self,
        action: &str,
        category: FailureCategory,
        step: &RecoveryAction,
    ) -> Vec<&RecoveryObservation> {
        self.observations
            .iter()
            .filter(|o| o.action == action && o.category == category && &o.chosen_step == step)
            .collect()
    }

    /// Learned confidence for a step: a shrinkage estimator that equals the base
    /// prior with no data and converges to the observed success rate as trials
    /// accumulate.
    pub fn effective(
        &self,
        base: f64,
        action: &str,
        category: FailureCategory,
        step: &RecoveryAction,
    ) -> f64 {
        let matches = self.matching(action, category, step);
        if matches.len() >= self.min_samples as usize {
            let trials = matches.len() as f64;
            let successes = matches.iter().filter(|o| o.outcome).count() as f64;
            let observed = successes / trials;
            (observed * trials + base * self.prior_weight) / (trials + self.prior_weight)
        } else {
            base
        }
    }

    /// (trials, successes) observed for a step, if any.
    pub fn stats(
        &self,
        action: &str,
        category: FailureCategory,
        step: &RecoveryAction,
    ) -> Option<(u32, u32)> {
        let m = self.matching(action, category, step);
        if m.is_empty() {
            None
        } else {
            let trials = m.len() as u32;
            let successes = m.iter().filter(|o| o.outcome).count() as u32;
            Some((trials, successes))
        }
    }

    /// Mean recovery latency (ms) for a step, if observed.
    pub fn mean_latency_ms(
        &self,
        action: &str,
        category: FailureCategory,
        step: &RecoveryAction,
    ) -> Option<f64> {
        let m = self.matching(action, category, step);
        if m.is_empty() {
            None
        } else {
            let sum: f64 = m.iter().map(|o| o.latency_ms).sum();
            Some(sum / m.len() as f64)
        }
    }

    /// Fraction of recoveries for a step that a user interrupted, if observed.
    pub fn user_interrupt_rate(
        &self,
        action: &str,
        category: FailureCategory,
        step: &RecoveryAction,
    ) -> Option<f64> {
        let m = self.matching(action, category, step);
        if m.is_empty() {
            None
        } else {
            let interrupted = m.iter().filter(|o| o.user_interrupted).count() as f64;
            Some(interrupted / m.len() as f64)
        }
    }
}
