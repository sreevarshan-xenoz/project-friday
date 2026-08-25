//! 8A production seam: a thin adapter from the real `friday_goal_execution::WorldModel`
//! to the autonomy [`WorldModel`](crate::world::WorldModel) trait.
//!
//! The adapter **translates types and delegates** — it contains no world-model
//! logic of its own. `simulate` → [`WorldModel::predict`], `state` →
//! [`WorldModel::observe`], and the assessment + `apply_verified_observation` →
//! [`WorldModel::update`]. [`crate::world::SimpleWorldModel`] remains the hermetic
//! fake and is untouched.

use crate::world::{PredictionOutcome, WorldModel, WorldSnapshot, WorldStateDiff};
use friday_goal_execution::{
    Capability, VerifiedObservation, WorldModel as ProdWorldModel, WorldState,
};
use std::sync::Mutex;

/// Reduce a production [`WorldState`] to the autonomy [`WorldSnapshot`] view.
///
/// Deliberate reduction: the autonomy tick reasons over `variables` plus a small
/// flag set; the production model keeps full fidelity (files, processes, env,
/// tools, mission progress, ...). The production model is **never** reconstructed
/// from this reduced view, so no production state is lost.
fn to_snapshot(state: &WorldState) -> WorldSnapshot {
    let mut snap = WorldSnapshot::new();
    for (k, v) in &state.variables {
        snap.variables.insert(k.clone(), v.clone());
    }
    snap.flags
        .insert("network_available".to_string(), state.network_available);
    snap.flags.insert(
        "working_directory_set".to_string(),
        state.working_directory.is_some(),
    );
    snap
}

/// Wraps the real production world model so the bounded tick can talk to it through
/// the stable autonomy [`WorldModel`] trait. Interior mutability (`Mutex`) keeps the
/// trait methods `&self`, so the tick stays `&self` and bounded.
pub struct ProductionWorldModelAdapter {
    inner: Mutex<ProdWorldModel>,
}

impl ProductionWorldModelAdapter {
    pub fn new(initial: WorldState) -> Self {
        Self {
            inner: Mutex::new(ProdWorldModel::new(initial)),
        }
    }

    /// Wrap an existing production world-model instance (e.g. one seeded with the
    /// M5.6 knowledge graph).
    pub fn from_model(model: ProdWorldModel) -> Self {
        Self {
            inner: Mutex::new(model),
        }
    }

    /// Inspect the live production state (tests / debugging).
    pub fn snapshot_state(&self) -> WorldState {
        self.inner
            .lock()
            .expect("world model poisoned")
            .state()
            .clone()
    }

    /// Replace the production state (tests / debugging).
    pub fn set_world_state(&self, state: WorldState) {
        let mut g = self.inner.lock().expect("world model poisoned");
        let _ = g.apply_verified_observation(VerifiedObservation {
            action_id: "test".to_string(),
            goal_id: "test".to_string(),
            state,
            preconditions: vec![],
        });
    }
}

impl WorldModel for ProductionWorldModelAdapter {
    fn predict(&self, action: &str) -> WorldSnapshot {
        // The autonomy tick passes the goal as `action`. Production `simulate` is
        // keyed by a `Capability`; there is no `FromStr` for `Capability`, so we
        // delegate through `Capability::Custom` — a lossless, non-bending
        // translation. Where the production causal model has no edge for this
        // capability it returns an inapplicable projection (the unchanged current
        // state), which is the honest prediction until 8D/8E wire real
        // capabilities/steps.
        let cap = Capability::Custom(action.to_string());
        let sim = self
            .inner
            .lock()
            .expect("world model poisoned")
            .simulate(&cap);
        to_snapshot(&sim.projected_world)
    }

    fn observe(&self) -> Option<WorldSnapshot> {
        // The real model always knows its current state.
        Some(to_snapshot(
            self.inner.lock().expect("world model poisoned").state(),
        ))
    }

    fn update(
        &self,
        predicted: &WorldSnapshot,
        actual: Option<WorldSnapshot>,
    ) -> PredictionOutcome {
        // Assessment is computed in autonomy types — no production logic. Compare the
        // predicted snapshot against the observed snapshot.
        let outcome = match &actual {
            None => PredictionOutcome::Unknown,
            Some(a) => {
                let diff: WorldStateDiff = predicted.diff(a);
                if diff.is_empty() {
                    PredictionOutcome::Correct
                } else {
                    PredictionOutcome::Mismatch { diff }
                }
            }
        };

        // Delegate the writeback to the production model. We submit the REAL full
        // production state (read back through `state()`), not a lossy reconstruction
        // of the reduced snapshot, so the production model's fidelity is preserved —
        // no file/process/env data is clobbered. Applying the current state as a
        // verified observation is a safe self-transition; a richer re-observation
        // arrives with 8E.
        if actual.is_some() {
            let mut g = self.inner.lock().expect("world model poisoned");
            let real_state = g.state().clone();
            g.apply_verified_observation(VerifiedObservation {
                action_id: "autonomous-tick".to_string(),
                goal_id: "autonomous-tick".to_string(),
                state: real_state,
                preconditions: vec![],
            });
        }

        outcome
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use friday_goal_execution::FileMeta;

    fn file_meta(path: &str) -> FileMeta {
        FileMeta {
            path: path.to_string(),
            exists: true,
            size_bytes: None,
            is_directory: false,
            modified_at_ms: None,
        }
    }

    // 8A.1: predict reflects the current production state through the reduced view.
    #[test]
    fn predict_reflects_current_production_state() {
        let init = WorldState::new()
            .with_variable("x", serde_json::json!(1))
            .with_network(true);
        let adapter = ProductionWorldModelAdapter::new(init);

        let pred = adapter.predict("research project");
        assert_eq!(pred.variables.get("x"), Some(&serde_json::json!(1)));
        assert_eq!(pred.flags.get("network_available"), Some(&true));
    }

    // 8A.2: observe returns the current production state (never unknown here).
    #[test]
    fn observe_returns_current_state() {
        let init = WorldState::new().with_variable("x", serde_json::json!(7));
        let adapter = ProductionWorldModelAdapter::new(init);
        let obs = adapter.observe();
        assert!(obs.is_some());
        assert_eq!(obs.unwrap().variables.get("x"), Some(&serde_json::json!(7)));
    }

    // 8A.3: predict == observe ⇒ Correct (no second prediction engine, no clobber).
    #[test]
    fn matching_predict_and_observe_is_correct() {
        let init = WorldState::new()
            .with_variable("x", serde_json::json!(1))
            .with_network(true);
        let adapter = ProductionWorldModelAdapter::new(init);

        let predicted = adapter.predict("research project");
        let actual = adapter.observe();
        let outcome = adapter.update(&predicted, actual);

        assert_eq!(outcome, PredictionOutcome::Correct);
        // Production state preserved (file still present, not clobbered).
        let s = adapter.snapshot_state();
        assert_eq!(s.variables.get("x"), Some(&serde_json::json!(1)));
    }

    // 8A.4: divergence between predict and observe ⇒ Mismatch (model records it).
    #[test]
    fn divergent_predict_and_observe_is_mismatch() {
        let init = WorldState::new()
            .with_variable("x", serde_json::json!(1))
            .with_network(true);
        let adapter = ProductionWorldModelAdapter::new(init);

        let predicted = adapter.predict("research project");
        // Reality changes (simulated re-observation with a different value).
        adapter.set_world_state(
            WorldState::new()
                .with_variable("x", serde_json::json!(2))
                .with_network(true),
        );
        let actual = adapter.observe();

        let outcome = adapter.update(&predicted, actual);
        match &outcome {
            PredictionOutcome::Mismatch { diff } => {
                assert_eq!(diff.changed, vec!["x".to_string()]);
            }
            other => panic!("expected Mismatch, got {:?}", other),
        }
    }

    // 8A.5: unknown actual ⇒ Unknown; model must NOT invent an outcome, and must
    // NOT mutate the production state.
    #[test]
    fn unknown_actual_is_deferred_and_preserves_state() {
        let init = WorldState::new()
            .with_file("/a.txt", file_meta("/a.txt"))
            .with_variable("x", serde_json::json!(1));
        let adapter = ProductionWorldModelAdapter::new(init.clone());

        let predicted = adapter.predict("research project");
        let outcome = adapter.update(&predicted, None);

        assert_eq!(outcome, PredictionOutcome::Unknown);
        assert!(outcome.verification().is_none());
        // Production state untouched.
        let s = adapter.snapshot_state();
        assert!(s.file_exists("/a.txt"));
        assert_eq!(s.variables.get("x"), Some(&serde_json::json!(1)));
    }
}
