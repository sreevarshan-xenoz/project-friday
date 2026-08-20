//! 7B world-model feedback — the smallest adapter around the existing world-model
//! concepts.
//!
//! The authoritative world model already lives in `friday-goal-execution`
//! (`WorldState`, `WorldStateDiff`, `CausalSimulation.projected_world` = the
//! prediction, `VerifiedObservation` = the actual re-observation, `StateTransition`
//! = the recorded causal transition). This module reuses that *shape* in a
//! dependency-light, hermetic form so the controlled 7A tick can compare its
//! prediction against reality without pulling the heavy goal-execution crate into
//! the experiment.
//!
//! The real `friday-goal-execution::WorldModel` can implement [`WorldModel`] later
//! without changing the bounded tick. Crucially, **no world-model state is ever
//! written into `CognitiveTrace`** — the trace records the world-model *operation*
//! and its *assessment* (Correct / Mismatch / Unknown), never the domain state.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Mutex;

/// A minimal world-state snapshot — mirrors `friday-goal-execution::WorldState`
/// (variables + boolean flags such as file/network presence).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct WorldSnapshot {
    pub variables: HashMap<String, serde_json::Value>,
    pub flags: HashMap<String, bool>,
}

impl WorldSnapshot {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_variable(mut self, key: impl Into<String>, value: serde_json::Value) -> Self {
        self.variables.insert(key.into(), value);
        self
    }

    pub fn with_flag(mut self, key: impl Into<String>, value: bool) -> Self {
        self.flags.insert(key.into(), value);
        self
    }

    /// Difference between two snapshots — mirrors `WorldState::diff` /
    /// `WorldStateDiff`. Empty means "nothing changed".
    pub fn diff(&self, other: &WorldSnapshot) -> WorldStateDiff {
        let mut added = Vec::new();
        let mut removed = Vec::new();
        let mut changed = Vec::new();

        for (k, v) in &other.variables {
            match self.variables.get(k) {
                None => added.push(k.clone()),
                Some(old) if old != v => changed.push(k.clone()),
                _ => {}
            }
        }
        for k in self.variables.keys() {
            if !other.variables.contains_key(k) {
                removed.push(k.clone());
            }
        }

        for (k, v) in &other.flags {
            match self.flags.get(k) {
                None => added.push(k.clone()),
                Some(old) if old != v => changed.push(k.clone()),
                _ => {}
            }
        }
        for k in self.flags.keys() {
            if !other.flags.contains_key(k) {
                removed.push(k.clone());
            }
        }

        WorldStateDiff {
            added,
            removed,
            changed,
        }
    }
}

/// What changed between two world snapshots — mirrors `WorldStateDiff`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct WorldStateDiff {
    pub added: Vec<String>,
    pub removed: Vec<String>,
    pub changed: Vec<String>,
}

impl WorldStateDiff {
    pub fn is_empty(&self) -> bool {
        self.added.is_empty() && self.removed.is_empty() && self.changed.is_empty()
    }
}

/// The assessment of a prediction against reality.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum PredictionOutcome {
    /// Predicted state equals observed state — model was right.
    Correct,
    /// Predicted state diverged from reality — model records a correction.
    Mismatch { diff: WorldStateDiff },
    /// Observed state is unknown — the model must NOT invent an outcome; the
    /// update is deferred and the model's belief is left unchanged.
    Unknown,
}

impl PredictionOutcome {
    /// Maps the prediction assessment onto the frozen `VerificationStatus` used by
    /// `CognitiveTrace`. `Unknown` deliberately has no verdict — the trace must
    /// not synthesize one.
    pub fn verification(&self) -> Option<friday_core::trace::VerificationStatus> {
        match self {
            PredictionOutcome::Correct => Some(friday_core::trace::VerificationStatus::Passed),
            PredictionOutcome::Mismatch { .. } => Some(friday_core::trace::VerificationStatus::Failed {
                reason: "prediction mismatch".into(),
            }),
            PredictionOutcome::Unknown => None,
        }
    }

    pub fn result(&self) -> friday_core::trace::ResultType {
        match self {
            PredictionOutcome::Correct => friday_core::trace::ResultType::Success,
            PredictionOutcome::Mismatch { .. } => friday_core::trace::ResultType::Failure,
            PredictionOutcome::Unknown => friday_core::trace::ResultType::Pending,
        }
    }
}

/// A recorded prediction correction — mirrors the audit-trail concept of
/// `StateTransition` (before/after) without touching the frozen trace.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorldModelCorrection {
    pub predicted: WorldSnapshot,
    pub actual: Option<WorldSnapshot>,
    pub outcome: PredictionOutcome,
}

/// The world-model collaborator the bounded tick talks to. Mirrors the existing
/// `WorldModel` surface: `simulate` → [`WorldModel::predict`], `VerifiedObservation`
/// → [`WorldModel::observe`], `apply_verified_observation`/`record_transition` →
/// [`WorldModel::update`].
///
/// All methods are `&self` (interior mutability) so the tick can stay `&self` and
/// bounded — no recursion, no scheduling.
pub trait WorldModel: Send + Sync {
    /// The predicted post-action world state (mirrors `CausalSimulation.projected_world`).
    fn predict(&self, action: &str) -> WorldSnapshot;

    /// The actual post-action re-observation. `None` ⇒ unknown/uncertain.
    fn observe(&self) -> Option<WorldSnapshot>;

    /// Compare `predicted` against `actual`, update the model's belief, and return
    /// the assessment. Unknown `actual` ⇒ `Unknown`, belief unchanged (deferred).
    fn update(&self, predicted: &WorldSnapshot, actual: Option<WorldSnapshot>) -> PredictionOutcome;
}

/// A hermetic in-memory `WorldModel` used by 7B tests and as the reference
/// adapter. The real `friday-goal-execution::WorldModel` can implement the trait
/// with the same shape.
pub struct SimpleWorldModel {
    predicted: WorldSnapshot,
    observed: Option<WorldSnapshot>,
    current: Mutex<WorldSnapshot>,
    corrections: Mutex<Vec<WorldModelCorrection>>,
}

impl SimpleWorldModel {
    pub fn new(predicted: WorldSnapshot, observed: Option<WorldSnapshot>) -> Self {
        let current = predicted.clone();
        Self {
            predicted,
            observed,
            current: Mutex::new(current),
            corrections: Mutex::new(Vec::new()),
        }
    }

    /// Inspect the model's current belief (for assertions in tests).
    pub fn current(&self) -> WorldSnapshot {
        self.current.lock().expect("world model poisoned").clone()
    }

    /// Inspect the correction audit trail (for assertions in tests).
    pub fn corrections(&self) -> Vec<WorldModelCorrection> {
        self.corrections.lock().expect("world model poisoned").clone()
    }
}

impl WorldModel for SimpleWorldModel {
    fn predict(&self, _action: &str) -> WorldSnapshot {
        self.predicted.clone()
    }

    fn observe(&self) -> Option<WorldSnapshot> {
        self.observed.clone()
    }

    fn update(&self, predicted: &WorldSnapshot, actual: Option<WorldSnapshot>) -> PredictionOutcome {
        let outcome = match &actual {
            None => PredictionOutcome::Unknown,
            Some(a) => {
                let diff = predicted.diff(a);
                if diff.is_empty() {
                    PredictionOutcome::Correct
                } else {
                    PredictionOutcome::Mismatch { diff }
                }
            }
        };

        // Reality wins: when actual is known, the belief adopts it. When unknown,
        // the belief is deliberately left unchanged (deferred — never invented).
        if let Some(a) = &actual {
            *self.current.lock().expect("world model poisoned") = a.clone();
        }

        self.corrections
            .lock()
            .expect("world model poisoned")
            .push(WorldModelCorrection {
                predicted: predicted.clone(),
                actual,
                outcome: outcome.clone(),
            });

        outcome
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── 7B.1: correct prediction ────────────────────────────────────────
    // predicted == actual → verification Passed → model update accepted.
    #[test]
    fn correct_prediction_accepts_update() {
        let predicted = WorldSnapshot::new().with_variable("version", serde_json::json!(1));
        let actual = Some(WorldSnapshot::new().with_variable("version", serde_json::json!(1)));
        let wm = SimpleWorldModel::new(predicted.clone(), actual);

        let outcome = wm.update(&predicted, wm.observe());

        assert_eq!(outcome, PredictionOutcome::Correct);
        // Belief accepted the (identical) actual state.
        assert_eq!(wm.current(), predicted);
        // Verification mapping is Passed.
        assert!(matches!(
            outcome.verification(),
            Some(friday_core::trace::VerificationStatus::Passed)
        ));

        let corr = wm.corrections();
        assert_eq!(corr.len(), 1);
        assert_eq!(corr[0].outcome, PredictionOutcome::Correct);
        assert!(corr[0].actual.is_some());
    }

    // ── 7B.2: prediction error ──────────────────────────────────────────
    // predicted != actual → verification Failed → model records correction,
    // failure remains failure (reality still adopted as the new belief).
    #[test]
    fn prediction_error_records_correction_and_failure() {
        let predicted = WorldSnapshot::new().with_variable("version", serde_json::json!(1));
        let actual = WorldSnapshot::new().with_variable("version", serde_json::json!(2));
        let wm = SimpleWorldModel::new(predicted.clone(), Some(actual.clone()));

        let outcome = wm.update(&predicted, wm.observe());

        match &outcome {
            PredictionOutcome::Mismatch { diff } => {
                assert_eq!(diff.changed, vec!["version".to_string()]);
            }
            other => panic!("expected Mismatch, got {:?}", other),
        }
        // Reality wins: belief becomes the actual (version 2), not the prediction.
        assert_eq!(wm.current(), actual);
        // Failure remains a failure.
        assert!(matches!(
            outcome.verification(),
            Some(friday_core::trace::VerificationStatus::Failed { .. })
        ));

        let corr = wm.corrections();
        assert_eq!(corr.len(), 1);
        assert!(matches!(corr[0].outcome, PredictionOutcome::Mismatch { .. }));
    }

    // ── 7B.3: unknown outcome ───────────────────────────────────────────
    // actual = unknown → model does NOT invent an outcome → update deferred,
    // belief unchanged.
    #[test]
    fn unknown_outcome_defers_update() {
        let predicted = WorldSnapshot::new().with_variable("version", serde_json::json!(1));
        let wm = SimpleWorldModel::new(predicted.clone(), None);
        let before = wm.current();

        let outcome = wm.update(&predicted, wm.observe());

        assert_eq!(outcome, PredictionOutcome::Unknown);
        // No verdict is synthesized.
        assert!(outcome.verification().is_none());
        // Belief is left exactly as it was — deferred, not invented.
        assert_eq!(wm.current(), before);

        let corr = wm.corrections();
        assert_eq!(corr.len(), 1);
        assert_eq!(corr[0].outcome, PredictionOutcome::Unknown);
        assert!(corr[0].actual.is_none());
    }
}
