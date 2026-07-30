use std::collections::HashMap;

use crate::contract::Capability;

/// Policy Engine — declares which providers are allowed and assigns
/// score bonuses or penalties based on policy rules.
///
/// The resolver never encodes policy directly. It queries the policy
/// engine to filter candidates and adjust scores.
pub trait PolicyEngine: Send + Sync {
    /// Filter a list of candidate provider IDs, returning only the
    /// ones that are allowed by policy.
    fn filter_providers(&self, capability: &Capability, candidate_ids: &[String]) -> Vec<String>;

    /// Return a score bonus (or penalty) for a specific provider.
    /// Range: [-1.0, 1.0] where positive = preferred, negative = discouraged.
    fn score_bonus(&self, plugin_id: &str, capability: &Capability) -> f64;
}

/// Default policy engine — allows all providers and assigns no bonuses.
pub struct DefaultPolicyEngine;

impl PolicyEngine for DefaultPolicyEngine {
    fn filter_providers(&self, _capability: &Capability, candidate_ids: &[String]) -> Vec<String> {
        candidate_ids.to_vec()
    }

    fn score_bonus(&self, _plugin_id: &str, _capability: &Capability) -> f64 {
        0.0
    }
}

// ── Composable Policy Layers ────────────────────────────────────────────────

/// Security-gated policy: blocks providers that don't meet security requirements.
///
/// If the context requires High or Maximum security, providers without
/// the "security.high" permission are filtered out if safer alternatives exist.
pub struct SecurityPolicy {
    /// Maps provider_id → permission set
    provider_permissions: HashMap<String, Vec<String>>,
}

impl SecurityPolicy {
    pub fn new() -> Self {
        Self {
            provider_permissions: HashMap::new(),
        }
    }

    pub fn with_provider_permissions(mut self, provider_id: &str, permissions: Vec<&str>) -> Self {
        self.provider_permissions.insert(
            provider_id.to_string(),
            permissions.into_iter().map(|s| s.to_string()).collect(),
        );
        self
    }
}

impl Default for SecurityPolicy {
    fn default() -> Self {
        Self::new()
    }
}

impl PolicyEngine for SecurityPolicy {
    fn filter_providers(&self, _capability: &Capability, candidate_ids: &[String]) -> Vec<String> {
        // Security policy alone doesn't filter; it works via score_bonus
        candidate_ids.to_vec()
    }

    fn score_bonus(&self, plugin_id: &str, _capability: &Capability) -> f64 {
        // Could check permissions here; for now base score
        if let Some(perms) = self.provider_permissions.get(plugin_id) {
            if perms.contains(&"security.high".to_string()) {
                return 0.1;
            }
        }
        0.0
    }
}

/// Organizational policy: enforces org-wide allow/block rules.
pub struct OrganizationPolicy {
    allowed_providers: Option<Vec<String>>,
    blocked_providers: Vec<String>,
}

impl OrganizationPolicy {
    pub fn new() -> Self {
        Self {
            allowed_providers: None,
            blocked_providers: Vec::new(),
        }
    }

    pub fn allow_only(mut self, providers: Vec<String>) -> Self {
        self.allowed_providers = Some(providers);
        self
    }

    pub fn block(mut self, provider_id: &str) -> Self {
        self.blocked_providers.push(provider_id.to_string());
        self
    }
}

impl Default for OrganizationPolicy {
    fn default() -> Self {
        Self::new()
    }
}

impl PolicyEngine for OrganizationPolicy {
    fn filter_providers(&self, _capability: &Capability, candidate_ids: &[String]) -> Vec<String> {
        candidate_ids
            .iter()
            .filter(|id| {
                let blocked = self.blocked_providers.contains(id);
                let allowed = self
                    .allowed_providers
                    .as_ref()
                    .map(|allowed| allowed.contains(id))
                    .unwrap_or(true);
                !blocked && allowed
            })
            .cloned()
            .collect()
    }

    fn score_bonus(&self, _plugin_id: &str, _capability: &Capability) -> f64 {
        0.0
    }
}

/// Resource policy: enforces cost and concurrency constraints.
pub struct ResourcePolicy {
    max_budget: f64,
    provider_costs: HashMap<String, f64>,
}

impl ResourcePolicy {
    pub fn new() -> Self {
        Self {
            max_budget: f64::MAX,
            provider_costs: HashMap::new(),
        }
    }

    pub fn with_max_budget(mut self, budget: f64) -> Self {
        self.max_budget = budget;
        self
    }

    pub fn with_provider_cost(mut self, provider_id: &str, cost: f64) -> Self {
        self.provider_costs
            .insert(provider_id.to_string(), cost);
        self
    }
}

impl Default for ResourcePolicy {
    fn default() -> Self {
        Self::new()
    }
}

impl PolicyEngine for ResourcePolicy {
    fn filter_providers(&self, _capability: &Capability, candidate_ids: &[String]) -> Vec<String> {
        candidate_ids
            .iter()
            .filter(|id| {
                let cost = self.provider_costs.get(*id).copied().unwrap_or(0.0);
                cost <= self.max_budget
            })
            .cloned()
            .collect()
    }

    fn score_bonus(&self, plugin_id: &str, _capability: &Capability) -> f64 {
        let cost = self.provider_costs.get(plugin_id).copied().unwrap_or(0.0);
        if self.max_budget < f64::MAX && cost > 0.0 {
            -cost.min(0.5)
        } else {
            0.0
        }
    }
}

/// User preference policy: enforces user-specified preferences.
pub struct UserPolicy {
    preferred_providers: Vec<String>,
    dispreferred_providers: Vec<String>,
}

impl UserPolicy {
    pub fn new() -> Self {
        Self {
            preferred_providers: Vec::new(),
            dispreferred_providers: Vec::new(),
        }
    }

    pub fn prefer(mut self, provider_id: &str) -> Self {
        self.preferred_providers.push(provider_id.to_string());
        self
    }

    pub fn disprefer(mut self, provider_id: &str) -> Self {
        self.dispreferred_providers.push(provider_id.to_string());
        self
    }
}

impl Default for UserPolicy {
    fn default() -> Self {
        Self::new()
    }
}

impl PolicyEngine for UserPolicy {
    fn filter_providers(&self, _capability: &Capability, candidate_ids: &[String]) -> Vec<String> {
        candidate_ids.to_vec()
    }

    fn score_bonus(&self, plugin_id: &str, _capability: &Capability) -> f64 {
        let mut bonus = 0.0;
        if self.preferred_providers.contains(&plugin_id.to_string()) {
            bonus += 0.3;
        }
        if self.dispreferred_providers.contains(&plugin_id.to_string()) {
            bonus -= 0.3;
        }
        bonus
    }
}

/// Composes multiple policy layers into a single engine.
///
/// Layers are evaluated in order:
///   1. SecurityPolicy  — security gating (score bonuses)
///   2. OrganizationPolicy — org-wide allow/block rules
///   3. ResourcePolicy  — cost & concurrency constraints
///   4. UserPolicy      — user-specified preferences
///
/// Filtering is cumulative: each layer receives the output of the
/// previous layer's filter. Score bonuses are summed across layers.
pub struct LayeredPolicyEngine {
    layers: Vec<(String, Box<dyn PolicyEngine>)>,
}

impl LayeredPolicyEngine {
    pub fn new() -> Self {
        Self { layers: Vec::new() }
    }

    pub fn push(mut self, name: &str, engine: Box<dyn PolicyEngine>) -> Self {
        self.layers.push((name.to_string(), engine));
        self
    }

    pub fn layer_count(&self) -> usize {
        self.layers.len()
    }
}

impl Default for LayeredPolicyEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl PolicyEngine for LayeredPolicyEngine {
    fn filter_providers(&self, capability: &Capability, candidate_ids: &[String]) -> Vec<String> {
        let mut ids = candidate_ids.to_vec();
        for (_name, layer) in &self.layers {
            ids = layer.filter_providers(capability, &ids);
            if ids.is_empty() {
                return ids;
            }
        }
        ids
    }

    fn score_bonus(&self, plugin_id: &str, capability: &Capability) -> f64 {
        self.layers
            .iter()
            .map(|(_name, layer)| layer.score_bonus(plugin_id, capability))
            .sum()
    }
}

// ── Legacy RuleBasedPolicyEngine (wraps LayeredPolicyEngine) ─────────────────

/// A policy engine with explicit allow/block rules and preference bonuses.
///
/// Maintained for backward compatibility. Internally delegates to
/// OrganizationPolicy + UserPolicy.
pub struct RuleBasedPolicyEngine {
    org_policy: OrganizationPolicy,
    user_policy: UserPolicy,
}

impl Default for RuleBasedPolicyEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl RuleBasedPolicyEngine {
    pub fn new() -> Self {
        Self {
            org_policy: OrganizationPolicy::new(),
            user_policy: UserPolicy::new(),
        }
    }

    pub fn allow_only(mut self, providers: Vec<String>) -> Self {
        self.org_policy = self.org_policy.allow_only(providers);
        self
    }

    pub fn block(mut self, provider_id: &str) -> Self {
        self.org_policy = self.org_policy.block(provider_id);
        self
    }

    pub fn prefer(mut self, provider_id: &str, bonus: f64) -> Self {
        if bonus > 0.0 {
            self.user_policy = self.user_policy.prefer(provider_id);
        }
        self
    }

    pub fn prefer_for(mut self, _capability: &Capability, provider_id: &str, _bonus: f64) -> Self {
        self.user_policy = self.user_policy.prefer(provider_id);
        self
    }
}

impl PolicyEngine for RuleBasedPolicyEngine {
    fn filter_providers(&self, capability: &Capability, candidate_ids: &[String]) -> Vec<String> {
        self.org_policy.filter_providers(capability, candidate_ids)
    }

    fn score_bonus(&self, plugin_id: &str, capability: &Capability) -> f64 {
        self.org_policy.score_bonus(plugin_id, capability)
            + self.user_policy.score_bonus(plugin_id, capability)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── DefaultPolicyEngine ─────────────────────────────────────────────

    #[test]
    fn default_policy_allows_all() {
        let policy = DefaultPolicyEngine;
        let ids = vec!["fs".into(), "browser".into()];
        let filtered = policy.filter_providers(&Capability::ReadFile, &ids);
        assert_eq!(filtered.len(), 2);
        assert_eq!(policy.score_bonus("fs", &Capability::ReadFile), 0.0);
    }

    // ── OrganizationPolicy ──────────────────────────────────────────────

    #[test]
    fn org_policy_blocks_specific_provider() {
        let policy = OrganizationPolicy::new().block("untrusted-fs");
        let ids = vec!["trusted-fs".into(), "untrusted-fs".into()];
        let filtered = policy.filter_providers(&Capability::ReadFile, &ids);
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0], "trusted-fs");
    }

    #[test]
    fn org_policy_allow_only() {
        let policy = OrganizationPolicy::new().allow_only(vec!["preferred-fs".into()]);
        let ids = vec!["preferred-fs".into(), "other-fs".into()];
        let filtered = policy.filter_providers(&Capability::ReadFile, &ids);
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0], "preferred-fs");
    }

    #[test]
    fn org_policy_block_overrides_allow() {
        let policy = OrganizationPolicy::new()
            .allow_only(vec!["a".into(), "b".into()])
            .block("b");
        let ids = vec!["a".into(), "b".into(), "c".into()];
        let filtered = policy.filter_providers(&Capability::ReadFile, &ids);
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0], "a");
    }

    // ── UserPolicy ──────────────────────────────────────────────────────

    #[test]
    fn user_policy_preference_bonus() {
        let policy = UserPolicy::new().prefer("fast-fs");
        assert!((policy.score_bonus("fast-fs", &Capability::ReadFile) - 0.3).abs() < 0.01);
        assert!((policy.score_bonus("slow-fs", &Capability::ReadFile) - 0.0).abs() < 0.01);
    }

    #[test]
    fn user_policy_dispreference_penalty() {
        let policy = UserPolicy::new().disprefer("slow-fs");
        assert!((policy.score_bonus("slow-fs", &Capability::ReadFile) + 0.3).abs() < 0.01);
    }

    // ── ResourcePolicy ──────────────────────────────────────────────────

    #[test]
    fn resource_policy_filters_by_cost() {
        let policy = ResourcePolicy::new()
            .with_max_budget(0.5)
            .with_provider_cost("cheap", 0.1)
            .with_provider_cost("expensive", 1.0);
        let ids = vec!["cheap".into(), "expensive".into()];
        let filtered = policy.filter_providers(&Capability::ReadFile, &ids);
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0], "cheap");
    }

    // ── SecurityPolicy ──────────────────────────────────────────────────

    #[test]
    fn security_policy_bonus_for_secure_providers() {
        let policy = SecurityPolicy::new()
            .with_provider_permissions("secure-fs", vec!["security.high"]);
        assert!((policy.score_bonus("secure-fs", &Capability::ReadFile) - 0.1).abs() < 0.01);
        assert!((policy.score_bonus("normal-fs", &Capability::ReadFile) - 0.0).abs() < 0.01);
    }

    // ── LayeredPolicyEngine ─────────────────────────────────────────────

    #[test]
    fn layered_policy_combines_multiple_engines() {
        let org = OrganizationPolicy::new().block("blocked-fs");
        let user = UserPolicy::new().prefer("favored-fs");
        let layered = LayeredPolicyEngine::new()
            .push("org", Box::new(org))
            .push("user", Box::new(user));

        let ids = vec!["favored-fs".into(), "blocked-fs".into(), "normal-fs".into()];
        let filtered = layered.filter_providers(&Capability::ReadFile, &ids);
        assert_eq!(filtered.len(), 2);
        assert!(!filtered.contains(&"blocked-fs".to_string()));

        // Sum of bonuses: org gives 0.0, user gives 0.3 for favored
        let bonus = layered.score_bonus("favored-fs", &Capability::ReadFile);
        assert!((bonus - 0.3).abs() < 0.01);
    }

    #[test]
    fn layered_policy_stops_when_empty() {
        let org = OrganizationPolicy::new().allow_only(vec!["a".into()]);
        let user = UserPolicy::new().prefer("b");
        let layered = LayeredPolicyEngine::new()
            .push("org", Box::new(org))
            .push("user", Box::new(user));

        let ids = vec!["b".into(), "c".into()];
        let filtered = layered.filter_providers(&Capability::ReadFile, &ids);
        assert!(filtered.is_empty());
    }

    // ── Legacy RuleBasedPolicyEngine ────────────────────────────────────

    #[test]
    fn rule_based_blocks_specific_provider() {
        let policy = RuleBasedPolicyEngine::new().block("untrusted-fs");
        let ids = vec!["trusted-fs".into(), "untrusted-fs".into()];
        let filtered = policy.filter_providers(&Capability::ReadFile, &ids);
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0], "trusted-fs");
    }

    #[test]
    fn rule_based_allow_only() {
        let policy = RuleBasedPolicyEngine::new()
            .allow_only(vec!["preferred-fs".into()]);
        let ids = vec!["preferred-fs".into(), "other-fs".into()];
        let filtered = policy.filter_providers(&Capability::ReadFile, &ids);
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0], "preferred-fs");
    }

    #[test]
    fn rule_based_preference_bonus() {
        let policy = RuleBasedPolicyEngine::new()
            .prefer("fast-fs", 0.2);
        assert!((policy.score_bonus("fast-fs", &Capability::ReadFile) - 0.3).abs() < 0.01);
        assert!((policy.score_bonus("slow-fs", &Capability::ReadFile) - 0.0).abs() < 0.01);
    }

    #[test]
    fn rule_based_block_overrides_allow() {
        let policy = RuleBasedPolicyEngine::new()
            .allow_only(vec!["a".into(), "b".into()])
            .block("b");
        let ids = vec!["a".into(), "b".into(), "c".into()];
        let filtered = policy.filter_providers(&Capability::ReadFile, &ids);
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0], "a");
    }
}
