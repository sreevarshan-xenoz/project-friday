//! Learning Recovery Confidence.
//!
//! Base confidence (`RecoveryAction::confidence`) is a prior. The
//! [`ConfidenceModel`] records actual outcomes of each recovery step and blends
//! them with the prior, so confidence *adjusts automatically* as evidence
//! accumulates — exactly the "Outcome → Experience → Updated Confidence" loop:
//!
//! ```text
//! RecoveryGraph → Policy → Outcome → ConfidenceModel → Updated Confidence
//! ```

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::taxonomy::{FailureCategory, RecoveryAction};

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
struct Sample {
    trials: u32,
    successes: u32,
}

/// Stable string key for a (action, category, step) outcome. JSON object keys
/// must be strings, so we compose one rather than serializing the enum tuple.
fn outcome_key(action: &str, category: FailureCategory, step: &RecoveryAction) -> String {
    let step_json = serde_json::to_string(step).unwrap_or_default();
    format!("{}|{}|{}", action, category.as_str(), step_json)
}

/// Accumulates observed recovery outcomes and yields a learned confidence per
/// (action, category, step). Persisted across runs inside `BenchmarkDay` so the
/// benchmark compounds evidence over time.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ConfidenceModel {
    outcomes: HashMap<String, Sample>,
    /// Trials below this count fall back to the base prior (with shrinkage).
    min_samples: u32,
    /// Weight of the base prior relative to observed trials (Laplace-style).
    prior_weight: f64,
}

impl ConfidenceModel {
    pub fn new() -> Self {
        Self {
            outcomes: HashMap::new(),
            min_samples: 20,
            prior_weight: 5.0,
        }
    }

    /// Record whether a recovery step ultimately delivered a successful outcome.
    pub fn record(
        &mut self,
        action: &str,
        category: FailureCategory,
        step: &RecoveryAction,
        success: bool,
    ) {
        let key = outcome_key(action, category, step);
        let s = self.outcomes.entry(key).or_default();
        s.trials += 1;
        if success {
            s.successes += 1;
        }
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
        let key = outcome_key(action, category, step);
        match self.outcomes.get(&key) {
            Some(s) if s.trials >= self.min_samples => {
                let observed = s.successes as f64 / s.trials as f64;
                (observed * s.trials as f64 + base * self.prior_weight)
                    / (s.trials as f64 + self.prior_weight)
            }
            _ => base,
        }
    }

    /// (trials, successes) observed for a step, if any.
    pub fn stats(
        &self,
        action: &str,
        category: FailureCategory,
        step: &RecoveryAction,
    ) -> Option<(u32, u32)> {
        self.outcomes
            .get(&outcome_key(action, category, step))
            .map(|s| (s.trials, s.successes))
    }
}
