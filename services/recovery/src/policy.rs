//! Recovery policies and the action-level recovery graph.

use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};

use crate::confidence::ConfidenceModel;
use crate::taxonomy::{FailureCategory, RecoveryAction};

/// Below this confidence, the chosen recovery is escalated to the user via the
/// existing approval policy.
pub const APPROVAL_THRESHOLD: f64 = 0.5;

/// An ordered recovery plan keyed by [`FailureCategory`].
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RecoveryPolicy {
    /// category -> steps tried in order until one resolves.
    pub by_category: HashMap<FailureCategory, Vec<RecoveryAction>>,
}

impl RecoveryPolicy {
    pub fn actions_for(&self, c: FailureCategory) -> &[RecoveryAction] {
        self.by_category
            .get(&c)
            .map(|v| v.as_slice())
            .unwrap_or(&[])
    }

    /// Convenience builder: add a step list for a category.
    pub fn with(mut self, c: FailureCategory, steps: Vec<RecoveryAction>) -> Self {
        self.by_category.insert(c, steps);
        self
    }
}

/// The outcome of attempting recovery for one failed action.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecoveryResult {
    pub action: String,
    pub category: FailureCategory,
    /// The full candidate plan (for audit / dashboards).
    pub steps: Vec<RecoveryAction>,
    /// The step actually taken (first one that resolves).
    pub chosen: RecoveryAction,
    /// Recovery Confidence for the chosen step.
    pub confidence: f64,
    /// True when confidence is low or the step is human-in-the-loop.
    pub needs_approval: bool,
    /// False when no step in the plan resolves with current availability.
    pub recovered: bool,
}

/// The recovery graph: `action id -> RecoveryPolicy`. Pure, deterministic,
/// data-driven (authored as Rust defaults or loaded from JSON).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RecoveryGraph {
    policies: HashMap<String, RecoveryPolicy>,
}

impl RecoveryGraph {
    pub fn new() -> Self {
        Self {
            policies: HashMap::new(),
        }
    }

    /// Graph with the project's default recovery policies (see `defaults`).
    pub fn with_defaults() -> Self {
        crate::defaults::default_graph()
    }

    pub fn register(&mut self, action: &str, policy: RecoveryPolicy) {
        self.policies.insert(action.to_string(), policy);
    }

    pub fn policy_for(&self, action: &str) -> Option<&RecoveryPolicy> {
        self.policies.get(action)
    }

    pub fn actions(&self) -> Vec<&String> {
        self.policies.keys().collect()
    }

    /// Attempt recovery for `action` failing under `category`, given the set of
    /// capability ids that currently have a live provider (`available_caps`).
    ///
    /// Returns the first resolving step (so the plan is followed in order) with
    /// its Recovery Confidence, or an unrecoverable result.
    pub fn recover(
        &self,
        action: &str,
        category: FailureCategory,
        available_caps: &HashSet<String>,
    ) -> RecoveryResult {
        self.recover_with(
            action,
            category,
            available_caps,
            &ConfidenceModel::default(),
        )
    }

    /// Like [`Self::recover`] but blends each step's base confidence with the
    /// learned [`ConfidenceModel`] (so approval decisions track observed
    /// outcomes across runs).
    pub fn recover_with(
        &self,
        action: &str,
        category: FailureCategory,
        available_caps: &HashSet<String>,
        model: &ConfidenceModel,
    ) -> RecoveryResult {
        let steps: Vec<RecoveryAction> = self
            .policies
            .get(action)
            .map(|p| p.actions_for(category).to_vec())
            .unwrap_or_default();

        let chosen_idx = steps
            .iter()
            .position(|a| a.resolves(category, available_caps));

        match chosen_idx {
            Some(i) => {
                let chosen = steps[i].clone();
                let confidence = model.effective(chosen.confidence(), action, category, &chosen);
                let needs_approval =
                    confidence < APPROVAL_THRESHOLD || matches!(chosen, RecoveryAction::AskUser);
                RecoveryResult {
                    action: action.to_string(),
                    category,
                    steps,
                    chosen,
                    confidence,
                    needs_approval,
                    recovered: true,
                }
            }
            None => RecoveryResult {
                action: action.to_string(),
                category,
                steps,
                chosen: RecoveryAction::Escalate,
                confidence: 0.0,
                needs_approval: true,
                recovered: false,
            },
        }
    }

    /// **Recovery Replay**: every step in the policy that would resolve this
    /// failure *now* (not just the first chosen one), with its confidence. Lets
    /// us ask "would another policy have worked better?" — invaluable for
    /// improving recovery after the fact.
    pub fn recover_alternatives(
        &self,
        action: &str,
        category: FailureCategory,
        available_caps: &HashSet<String>,
    ) -> Vec<(RecoveryAction, f64)> {
        self.policies
            .get(action)
            .map(|p| p.actions_for(category).to_vec())
            .unwrap_or_default()
            .into_iter()
            .filter(|a| a.resolves(category, available_caps))
            .map(|a| {
                let c = a.confidence();
                (a, c)
            })
            .collect()
    }

    /// Like [`Self::recover_alternatives`] but reports learned confidence.
    pub fn recover_alternatives_with(
        &self,
        action: &str,
        category: FailureCategory,
        available_caps: &HashSet<String>,
        model: &ConfidenceModel,
    ) -> Vec<(RecoveryAction, f64)> {
        self.policies
            .get(action)
            .map(|p| p.actions_for(category).to_vec())
            .unwrap_or_default()
            .into_iter()
            .filter(|a| a.resolves(category, available_caps))
            .map(|a| {
                let c = model.effective(a.confidence(), action, category, &a);
                (a, c)
            })
            .collect()
    }
}
