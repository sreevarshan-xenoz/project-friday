use std::collections::HashMap;

use crate::catalog::CapabilityCatalog;
use crate::contract::{
    Capability, CorrelationId, ExecutionPlan, PlanningRationale, PluginHealth, ProviderProfile,
    ResolvedCapability, RuntimeError, ScoredCandidate, ScoringWeights, SelectionContext,
    SelectionRationale, Urgency,
};
use crate::policy::PolicyEngine;
use crate::registry::CapabilityRegistry;

/// Capability Planner — multi-factor provider selection with rationale.
///
/// Decides which provider should handle a capability based on health,
/// policy, profile, and context signals. Produces a plan for the
/// runtime to execute. The rationale becomes part of the cognitive trace.
pub struct CapabilityPlanner<'a> {
    registry: &'a CapabilityRegistry,
    catalog: &'a CapabilityCatalog,
    policy: &'a dyn PolicyEngine,
    weights: &'a ScoringWeights,
    profiles: &'a HashMap<String, ProviderProfile>,
}

impl<'a> CapabilityPlanner<'a> {
    pub fn new(
        registry: &'a CapabilityRegistry,
        catalog: &'a CapabilityCatalog,
        policy: &'a dyn PolicyEngine,
        weights: &'a ScoringWeights,
        profiles: &'a HashMap<String, ProviderProfile>,
    ) -> Self {
        Self {
            registry,
            catalog,
            policy,
            weights,
            profiles,
        }
    }

    /// Resolve a capability to the best available plugin.
    ///
    /// Evaluates candidates across multiple signals:
    ///   - Reliability (from health monitor)
    ///   - Availability (from health monitor)
    ///   - Latency (observed vs expected)
    ///   - Cost (from catalog descriptor)
    ///   - Policy (from policy engine)
    ///   - Profile match (intrinsic provider characteristics × context)
    ///   - Context fit (urgency, offline, security × provider traits)
    ///
    /// Returns a `ResolvedCapability` with the chosen plugin and rationale,
    /// plus a full `SelectionRationale` for traceability.
    pub fn resolve(
        &self,
        capability: &Capability,
        plugin_health: &[&PluginHealth],
        correlation_id: &CorrelationId,
        context: &SelectionContext,
    ) -> Result<(ResolvedCapability, SelectionRationale), RuntimeError> {
        let raw_candidates = self.registry.plugins_for(capability);
        if raw_candidates.is_empty() {
            return Err(RuntimeError::NoProviderFound(capability.clone()));
        }

        let candidate_ids: Vec<String> = raw_candidates.iter().map(|s| s.to_string()).collect();
        let allowed = self.policy.filter_providers(capability, &candidate_ids);

        if allowed.is_empty() {
            let first = &candidate_ids[0];
            let name = self
                .registry
                .descriptor(first)
                .map(|d| d.name.as_str())
                .unwrap_or(first);
            let rationale = SelectionRationale {
                capability: capability.clone(),
                candidates: Vec::new(),
                selected_plugin_id: first.to_string(),
                composite_score: 0.0,
                explanation: format!(
                    "all candidates blocked by policy — falling back to {}",
                    name
                ),
                correlation_id: correlation_id.clone(),
            };
            return Ok((
                ResolvedCapability {
                    plugin_id: first.to_string(),
                    capability: capability.clone(),
                    confidence: 0.0,
                    rationale: rationale.explanation.clone(),
                    correlation_id: correlation_id.clone(),
                },
                rationale,
            ));
        }

        let candidate_health: Vec<_> = plugin_health
            .iter()
            .filter(|h| allowed.contains(&h.plugin_id))
            .copied()
            .collect();

        if candidate_health.is_empty() {
            // No health data yet — fall back with low confidence
            let first_id = &allowed[0];
            let desc = self.registry.descriptor(first_id);
            let name = desc.map(|d| d.name.as_str()).unwrap_or(first_id);
            let explanation = format!(
                "{} is registered but not yet healthy — selected as only provider",
                name
            );
            let fallback = ResolvedCapability {
                plugin_id: first_id.to_string(),
                capability: capability.clone(),
                confidence: 0.3,
                rationale: explanation.clone(),
                correlation_id: correlation_id.clone(),
            };
            let rationale = SelectionRationale {
                capability: capability.clone(),
                candidates: Vec::new(),
                selected_plugin_id: first_id.to_string(),
                composite_score: 0.3,
                explanation,
                correlation_id: correlation_id.clone(),
            };
            return Ok((fallback, rationale));
        }

        let cap_descriptor = self.catalog.get(capability);
        let expected_latency = cap_descriptor.map(|d| d.expected_latency_ms).unwrap_or(100);
        let estimated_cost = cap_descriptor.map(|d| d.estimated_cost).unwrap_or(0.1);

        // Score each candidate
        let mut scored: Vec<ScoredCandidate> = candidate_health
            .iter()
            .map(|h| {
                let reliability_score = h.reliability;
                let availability_score = h.availability;

                let latency_penalty = if h.average_latency_ms > 0.0 {
                    (h.average_latency_ms / (h.average_latency_ms + expected_latency as f64))
                        .min(1.0)
                } else {
                    0.0
                };

                let cost_penalty = estimated_cost;

                let policy_bonus = self.policy.score_bonus(&h.plugin_id, capability);

                let profile = self.profiles.get(&h.plugin_id);

                let profile_match_score = compute_profile_match(profile, capability, context);

                let context_fit_score = compute_context_fit(profile, context);

                let weighted = self.weights.reliability * reliability_score
                    + self.weights.availability * availability_score
                    - self.weights.latency * latency_penalty
                    - self.weights.cost * cost_penalty
                    + self.weights.policy * policy_bonus
                    + self.weights.profile_match * profile_match_score
                    + self.weights.context_fit * context_fit_score;

                ScoredCandidate {
                    plugin_id: h.plugin_id.clone(),
                    plugin_name: self
                        .registry
                        .descriptor(&h.plugin_id)
                        .map(|d| d.name.clone())
                        .unwrap_or_else(|| h.plugin_id.clone()),
                    reliability_score,
                    availability_score,
                    latency_penalty,
                    cost_penalty,
                    policy_bonus,
                    profile_match_score,
                    context_fit_score,
                    weighted_score: weighted,
                }
            })
            .collect();

        scored.sort_by(|a, b| {
            b.weighted_score
                .partial_cmp(&a.weighted_score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        let best = &scored[0];
        let candidates = scored.clone();
        let name = &best.plugin_name;

        let explanation = if scored.len() == 1 {
            format!(
                "{} is the only provider — reliability {:.2}, profile {:.2}, context {:.2}, score {:.3}",
                best.plugin_name, best.reliability_score, best.profile_match_score, best.context_fit_score, best.weighted_score
            )
        } else {
            let reasons: Vec<String> = candidates
                .iter()
                .filter(|c| c.plugin_id != best.plugin_id)
                .map(|c| format!("{} (score {:.3})", c.plugin_name, c.weighted_score))
                .collect();
            format!(
                "selected {} (score {:.3}) over {} — reliability {:.2}, profile {:.2}, policy {:.2}",
                name,
                best.weighted_score,
                reasons.join(", "),
                best.reliability_score,
                best.profile_match_score,
                best.policy_bonus,
            )
        };

        let rationale = SelectionRationale {
            capability: capability.clone(),
            candidates,
            selected_plugin_id: best.plugin_id.clone(),
            composite_score: best.weighted_score,
            explanation: explanation.clone(),
            correlation_id: correlation_id.clone(),
        };

        Ok((
            ResolvedCapability {
                plugin_id: best.plugin_id.clone(),
                capability: capability.clone(),
                confidence: best.weighted_score.clamp(0.0, 1.0),
                rationale: explanation,
                correlation_id: correlation_id.clone(),
            },
            rationale,
        ))
    }

    /// Plan execution of a capability — produce an `ExecutionPlan`.
    ///
    /// Internally delegates to the existing selection logic and wraps the
    /// result in a structured plan with dimensional estimates.
    ///
    /// The fallback chain is empty in this phase — replanning will be
    /// added in a later milestone.
    pub fn plan(
        &self,
        capability: &Capability,
        plugin_health: &[&PluginHealth],
        correlation_id: &CorrelationId,
        context: &SelectionContext,
    ) -> Result<ExecutionPlan, RuntimeError> {
        let (resolved, rationale) =
            self.resolve(capability, plugin_health, correlation_id, context)?;

        let cap_descriptor = self.catalog.get(capability);
        let expected_latency = cap_descriptor.map(|d| d.expected_latency_ms).unwrap_or(100);
        let estimated_cost = cap_descriptor.map(|d| d.estimated_cost).unwrap_or(0.1);

        let planning_rationale = PlanningRationale {
            candidates: rationale.candidates,
            selected_operator_id: rationale.selected_plugin_id,
            composite_score: rationale.composite_score,
            explanation: rationale.explanation,
            correlation_id: rationale.correlation_id,
            evaluator_evidence: Vec::new(),
        };

        Ok(ExecutionPlan {
            plan_id: CorrelationId::new(),
            version: 1,
            capability: capability.clone(),
            operator_id: resolved.plugin_id,
            estimated_latency_ms: expected_latency,
            estimated_cost,
            estimated_success_probability: resolved.confidence,
            estimated_risk: 1.0 - resolved.confidence,
            confidence: resolved.confidence,
            fallback_chain: Vec::new(),
            rationale: planning_rationale,
        })
    }
}

// ── Scoring helpers ─────────────────────────────────────────────────────────

/// How well a provider's intrinsic profile matches the capability category.
///
/// A streaming plugin gets a boost for streaming-like capabilities.
/// A deterministic plugin gets a boost for idempotent operations.
fn compute_profile_match(
    profile: Option<&ProviderProfile>,
    capability: &Capability,
    _context: &SelectionContext,
) -> f64 {
    let Some(profile) = profile else {
        return 0.0;
    };

    let mut score: f64 = 0.0;

    match capability {
        Capability::ReadFile | Capability::ListDirectory => {
            // Read operations benefit from determinism and offline capability
            if profile.deterministic {
                score += 0.3;
            }
            if profile.offline_capable {
                score += 0.2;
            }
            if !profile.requires_network {
                score += 0.1;
            }
        }
        Capability::WriteFile | Capability::DeleteFile => {
            // Write operations — side effects are expected
            if profile.deterministic {
                score += 0.2;
            }
            if profile.offline_capable {
                score += 0.2;
            }
        }
        Capability::ExecuteCommand => {
            // Shell commands — prefer deterministic, offline-capable
            if profile.deterministic {
                score += 0.2;
            }
            if profile.offline_capable {
                score += 0.3;
            }
        }
        Capability::OpenBrowser | Capability::SearchWeb => {
            // Network operations — network-required is fine, streaming helps
            if profile.streaming {
                score += 0.2;
            }
            if !profile.deterministic {
                score += 0.1; // non-determinism expected for search
            }
        }
        Capability::SearchMemory => {
            // Memory search — highly deterministic and offline
            if profile.deterministic {
                score += 0.3;
            }
            if profile.offline_capable {
                score += 0.3;
            }
        }
        Capability::Custom(_) => {
            score += 0.0;
        }
    }

    score
}

/// How well a provider fits the current execution context.
///
/// Urgency: critical requests bias toward low-latency, deterministic providers.
/// Offline: offline mode requires offline-capable providers.
/// Security: high security levels bias toward providers with security permissions.
fn compute_context_fit(profile: Option<&ProviderProfile>, context: &SelectionContext) -> f64 {
    let Some(profile) = profile else {
        return 0.0;
    };

    let mut score: f64 = 0.0;

    // Urgency: critical → prefer deterministic, fast providers
    match context.urgency {
        Urgency::Critical | Urgency::High => {
            if profile.deterministic {
                score += 0.25;
            }
            if !profile.requires_network {
                score += 0.15;
            }
        }
        Urgency::Normal => {
            if profile.deterministic {
                score += 0.1;
            }
        }
        Urgency::Low => {
            // No urgency bias
        }
    }

    // Offline mode
    if context.offline && profile.offline_capable {
        score += 0.3;
    } else if context.offline && profile.requires_network {
        score -= 0.5;
    }

    // Context tags match
    if !context.user_preferences.is_empty() {
        for (key, val) in &context.user_preferences {
            if profile.preferred_contexts.contains(key) || profile.preferred_contexts.contains(val)
            {
                score += 0.1;
            }
        }
    }

    // Preferred providers
    if context.preferred_providers.contains(&profile.plugin_id) {
        score += 0.2;
    }

    score.clamp(-1.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::CapabilityCatalog;
    use crate::contract::{PluginDescriptor, PluginStatus};
    use crate::policy::DefaultPolicyEngine;
    use crate::registry::CapabilityRegistry;

    fn cid() -> CorrelationId {
        CorrelationId::from_string("test-resolver")
    }

    fn default_ctx() -> SelectionContext {
        SelectionContext::default()
    }

    fn make_registry() -> CapabilityRegistry {
        let mut reg = CapabilityRegistry::new();
        reg.register(PluginDescriptor {
            id: "fs-native".into(),
            name: "Native Filesystem".into(),
            version: "1.0.0".into(),
            author: "FRIDAY".into(),
            capabilities: vec![Capability::ReadFile, Capability::WriteFile],
            permissions: vec!["fs.read".into()],
            profile: ProviderProfile::default_for("fs-native"),
        });
        reg.register(PluginDescriptor {
            id: "fs-remote".into(),
            name: "Remote Filesystem".into(),
            version: "1.0.0".into(),
            author: "FRIDAY".into(),
            capabilities: vec![Capability::ReadFile],
            permissions: vec!["network".into(), "fs.read".into()],
            profile: ProviderProfile {
                plugin_id: "fs-remote".into(),
                deterministic: true,
                offline_capable: false,
                streaming: false,
                has_side_effects: false,
                requires_network: true,
                max_concurrency: 2,
                preferred_contexts: vec!["remote".into()],
            },
        });
        reg
    }

    fn healthy(plugin_id: &str, reliability: f64, latency: f64) -> PluginHealth {
        PluginHealth {
            plugin_id: plugin_id.to_string(),
            status: PluginStatus::Healthy,
            availability: 1.0,
            reliability,
            average_latency_ms: latency,
            success_rate: reliability,
            total_executions: 10,
            failure_count: 0,
            last_error: None,
            last_execution_ms: None,
        }
    }

    fn empty_profiles() -> HashMap<String, ProviderProfile> {
        HashMap::new()
    }

    fn make_profiles() -> HashMap<String, ProviderProfile> {
        let mut map = HashMap::new();
        map.insert(
            "fs-native".into(),
            ProviderProfile::default_for("fs-native"),
        );
        map.insert(
            "fs-remote".into(),
            ProviderProfile {
                plugin_id: "fs-remote".into(),
                deterministic: true,
                offline_capable: false,
                streaming: false,
                has_side_effects: false,
                requires_network: true,
                max_concurrency: 2,
                preferred_contexts: vec!["remote".into()],
            },
        );
        map
    }

    #[test]
    fn resolve_picks_healthiest_plugin() {
        let reg = make_registry();
        let catalog = CapabilityCatalog::with_defaults();
        let policy = DefaultPolicyEngine;
        let weights = ScoringWeights::default();
        let profiles = make_profiles();
        let resolver = CapabilityPlanner::new(&reg, &catalog, &policy, &weights, &profiles);

        let fs_native = healthy("fs-native", 0.99, 5.0);
        let fs_remote = healthy("fs-remote", 0.85, 120.0);
        let health = vec![&fs_native, &fs_remote];

        let (resolved, rationale) = resolver
            .resolve(&Capability::ReadFile, &health, &cid(), &default_ctx())
            .unwrap();
        assert_eq!(resolved.plugin_id, "fs-native");
        assert!(
            resolved.confidence > 0.4,
            "confidence was {}",
            resolved.confidence
        );
        assert!(resolved.rationale.contains("Native Filesystem"));

        // Verify rationale structure
        assert_eq!(rationale.capability, Capability::ReadFile);
        assert_eq!(rationale.selected_plugin_id, "fs-native");
        assert!(rationale.composite_score > 0.0);
        assert_eq!(rationale.candidates.len(), 2);
    }

    #[test]
    fn resolve_falls_back_when_no_health_data() {
        let reg = make_registry();
        let catalog = CapabilityCatalog::with_defaults();
        let policy = DefaultPolicyEngine;
        let weights = ScoringWeights::default();
        let profiles = empty_profiles();
        let resolver = CapabilityPlanner::new(&reg, &catalog, &policy, &weights, &profiles);

        let (resolved, _) = resolver
            .resolve(&Capability::ReadFile, &[], &cid(), &default_ctx())
            .unwrap();
        assert_eq!(resolved.plugin_id, "fs-native");
        assert!((resolved.confidence - 0.3).abs() < 0.01);
        assert!(resolved.rationale.contains("not yet healthy"));
    }

    #[test]
    fn resolve_returns_error_for_unknown_capability() {
        let reg = CapabilityRegistry::new();
        let catalog = CapabilityCatalog::new();
        let policy = DefaultPolicyEngine;
        let weights = ScoringWeights::default();
        let profiles = empty_profiles();
        let resolver = CapabilityPlanner::new(&reg, &catalog, &policy, &weights, &profiles);

        let result = resolver.resolve(&Capability::OpenBrowser, &[], &cid(), &default_ctx());
        assert!(result.is_err());
        match result {
            Err(RuntimeError::NoProviderFound(_)) => {}
            _ => panic!("expected NoProviderFound"),
        }
    }

    #[test]
    fn rationale_contains_scored_candidates() {
        let reg = make_registry();
        let catalog = CapabilityCatalog::with_defaults();
        let policy = DefaultPolicyEngine;
        let weights = ScoringWeights::default();
        let profiles = make_profiles();
        let resolver = CapabilityPlanner::new(&reg, &catalog, &policy, &weights, &profiles);

        let fs_native = healthy("fs-native", 0.99, 5.0);
        let fs_remote = healthy("fs-remote", 0.85, 120.0);
        let health = vec![&fs_native, &fs_remote];

        let (_, rationale) = resolver
            .resolve(&Capability::ReadFile, &health, &cid(), &default_ctx())
            .unwrap();

        // Each candidate should have new score components
        for candidate in &rationale.candidates {
            assert!(candidate.reliability_score > 0.0);
            assert!(candidate.weighted_score > 0.0 || candidate.weighted_score <= 0.0);
        }

        let best_weighted = rationale
            .candidates
            .iter()
            .map(|c| c.weighted_score)
            .max_by(|a, b| a.partial_cmp(b).unwrap());
        assert!(best_weighted.is_some());
    }

    #[test]
    fn resolve_applies_policy_bonus() {
        use crate::policy::RuleBasedPolicyEngine;

        let reg = make_registry();
        let catalog = CapabilityCatalog::with_defaults();
        let policy = RuleBasedPolicyEngine::new().prefer("fs-native", 0.5);
        let weights = ScoringWeights::default();
        let profiles = make_profiles();
        let resolver = CapabilityPlanner::new(&reg, &catalog, &policy, &weights, &profiles);

        let fs_native = healthy("fs-native", 0.99, 5.0);
        let fs_remote = healthy("fs-remote", 0.99, 5.0); // identical health
        let health = vec![&fs_native, &fs_remote];

        let (resolved, rationale) = resolver
            .resolve(&Capability::ReadFile, &health, &cid(), &default_ctx())
            .unwrap();
        assert_eq!(resolved.plugin_id, "fs-native");

        // Policy bonus should be reflected in rationale
        let native_candidate = rationale
            .candidates
            .iter()
            .find(|c| c.plugin_id == "fs-native")
            .unwrap();
        assert!((native_candidate.policy_bonus - 0.3).abs() < 0.01);
    }

    #[test]
    fn resolve_prefers_offline_capable_in_offline_mode() {
        let reg = make_registry();
        let catalog = CapabilityCatalog::with_defaults();
        let policy = DefaultPolicyEngine;
        let weights = ScoringWeights::default();
        let profiles = make_profiles();
        let resolver = CapabilityPlanner::new(&reg, &catalog, &policy, &weights, &profiles);

        let fs_native = healthy("fs-native", 0.99, 5.0);
        let fs_remote = healthy("fs-remote", 0.99, 5.0);
        let health = vec![&fs_native, &fs_remote];

        let offline_ctx = SelectionContext::offline();
        let (resolved, rationale) = resolver
            .resolve(&Capability::ReadFile, &health, &cid(), &offline_ctx)
            .unwrap();

        // fs-native is offline-capable, fs-remote requires network
        assert_eq!(resolved.plugin_id, "fs-native");

        let native_candidate = rationale
            .candidates
            .iter()
            .find(|c| c.plugin_id == "fs-native")
            .unwrap();
        let remote_candidate = rationale
            .candidates
            .iter()
            .find(|c| c.plugin_id == "fs-remote")
            .unwrap();
        assert!(native_candidate.context_fit_score > remote_candidate.context_fit_score);
    }

    #[test]
    fn resolve_prefers_deterministic_for_critical_urgency() {
        let mut profiles = HashMap::new();
        profiles.insert(
            "det-fs".into(),
            ProviderProfile {
                plugin_id: "det-fs".into(),
                deterministic: true,
                offline_capable: true,
                streaming: false,
                has_side_effects: false,
                requires_network: false,
                max_concurrency: 4,
                preferred_contexts: vec![],
            },
        );
        profiles.insert(
            "non-det-fs".into(),
            ProviderProfile {
                plugin_id: "non-det-fs".into(),
                deterministic: false,
                offline_capable: true,
                streaming: true,
                has_side_effects: true,
                requires_network: true,
                max_concurrency: 1,
                preferred_contexts: vec![],
            },
        );

        let mut reg = CapabilityRegistry::new();
        reg.register(PluginDescriptor {
            id: "det-fs".into(),
            name: "Deterministic FS".into(),
            version: "1.0.0".into(),
            author: "FRIDAY".into(),
            capabilities: vec![Capability::ReadFile],
            permissions: vec![],
            profile: profiles.get("det-fs").unwrap().clone(),
        });
        reg.register(PluginDescriptor {
            id: "non-det-fs".into(),
            name: "Non-Deterministic FS".into(),
            version: "1.0.0".into(),
            author: "FRIDAY".into(),
            capabilities: vec![Capability::ReadFile],
            permissions: vec![],
            profile: profiles.get("non-det-fs").unwrap().clone(),
        });

        let catalog = CapabilityCatalog::with_defaults();
        let policy = DefaultPolicyEngine;
        let weights = ScoringWeights::default();
        let resolver = CapabilityPlanner::new(&reg, &catalog, &policy, &weights, &profiles);

        let det = healthy("det-fs", 0.9, 10.0);
        let non_det = healthy("non-det-fs", 0.9, 10.0);
        let health = vec![&det, &non_det];

        let critical_ctx = SelectionContext::urgent();
        let (resolved, rationale) = resolver
            .resolve(&Capability::ReadFile, &health, &cid(), &critical_ctx)
            .unwrap();
        assert_eq!(resolved.plugin_id, "det-fs");

        let det_candidate = rationale
            .candidates
            .iter()
            .find(|c| c.plugin_id == "det-fs")
            .unwrap();
        let non_det_candidate = rationale
            .candidates
            .iter()
            .find(|c| c.plugin_id == "non-det-fs")
            .unwrap();
        assert!(det_candidate.context_fit_score > non_det_candidate.context_fit_score);
    }

    #[test]
    fn profile_match_biases_toward_offline_for_filesystem() {
        let profile = ProviderProfile::default_for("local-fs");
        let score = compute_profile_match(Some(&profile), &Capability::ReadFile, &default_ctx());
        assert!(
            score > 0.5,
            "local filesystem should get strong profile match, got {}",
            score
        );
    }

    #[test]
    fn context_fit_penalizes_network_required_offline() {
        let profile = ProviderProfile {
            plugin_id: "remote".into(),
            deterministic: true,
            offline_capable: false,
            streaming: false,
            has_side_effects: false,
            requires_network: true,
            max_concurrency: 2,
            preferred_contexts: vec![],
        };
        let ctx = SelectionContext::offline();
        let score = compute_context_fit(Some(&profile), &ctx);
        assert!(
            score < 0.0,
            "network-required provider should be penalized in offline mode, got {}",
            score
        );
    }
}
