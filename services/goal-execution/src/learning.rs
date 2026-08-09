use crate::policy::Policy;
use crate::Capability;
use friday_reflection::{Insight, InsightKind};

// ── Learning Engine ─────────────────────────────────────────────────

/// Learning — consumes Insights, produces Policies.
pub struct LearningEngine {
    pub(crate) policies: Vec<Policy>,
}

impl LearningEngine {
    pub fn new() -> Self {
        Self {
            policies: Vec::new(),
        }
    }

    pub fn learn(&mut self, insights: &[Insight]) -> Vec<Policy> {
        let mut new_policies = Vec::new();

        for insight in insights {
            if let Some(policy) = self.derive_policy(insight) {
                new_policies.push(policy);
            }
        }

        self.policies.extend(new_policies.clone());
        new_policies
    }

    fn derive_policy(&self, insight: &Insight) -> Option<Policy> {
        match insight.kind {
            InsightKind::Lesson => Some(
                Policy::retry(Capability::ExecuteCommand, 1, insight.confidence, 1)
                    .with_created_from(vec![insight.id.clone()]),
            ),
            InsightKind::Reinforcement => None,
            InsightKind::Warning => None,
            InsightKind::Optimization => None,
        }
    }

    pub fn policies(&self) -> &[Policy] {
        &self.policies
    }

    pub fn policy_count(&self) -> usize {
        self.policies.len()
    }

    pub fn policies_for(&self, capability: &Capability) -> Vec<&Policy> {
        self.policies
            .iter()
            .filter(|p| p.capability.as_ref() == Some(capability))
            .collect()
    }

    pub fn prune_expired(&mut self, now_ms: u64) {
        self.policies.retain(|p| !p.is_expired(now_ms));
    }
}

impl Default for LearningEngine {
    fn default() -> Self {
        Self::new()
    }
}
