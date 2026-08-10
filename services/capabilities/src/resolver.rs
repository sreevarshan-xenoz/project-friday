//! Binding resolver with health-gated fallback.
//!
//! This is FRIDAY's expression of Agent-Reach's *resilience philosophy*: a
//! capability id (`web.search`, `web.fetch`) resolves to an ordered set of
//! candidate bindings, each with live health; the resolver skips what is
//! offline, dispatches through the best available binding, and on execution
//! failure falls back to the next candidate. Provider failure surfaces as a
//! structured [`ResolvedOutcome`] — never an unhandled transport error.
//!
//! It deliberately reuses the existing seams — [`ExecutionBinding`] describes
//! what to execute, [`CapabilityHealth`] gates selection — and adds *no* new
//! capability/provider trait. Dispatch is injected as a plain function so the
//! caller (the action runtime) stays the sole authority on execution; this
//! module only routes and falls back.
//!
//! The planner stays transport-blind: it asks for `web.search`, the resolver
//! owns "which provider serves it and what happens when it fails".

use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use futures::future::BoxFuture;
use serde::{Deserialize, Serialize};

use crate::spec::{CapabilityHealth, ExecutionBinding};

/// One candidate binding for a capability, tagged with its provider and live
/// health. Selection order follows registration order within the same health
/// tier (Healthy before Degraded; Offline is never selected).
#[derive(Debug, Clone)]
pub struct CandidateBinding {
    /// Provider name, e.g. `duckduckgo`, `jina`.
    pub provider: String,
    /// The resolved executable binding (adapter + action + permissions).
    pub binding: ExecutionBinding,
    /// Live health used to gate selection.
    pub health: CapabilityHealth,
}

/// Structured outcome of one resolve-then-execute pass. This is the
/// normalized evidence/result representation callers map into their own
/// result type (the goal-execution `ToolResult`, the action kernel's
/// `ActionResult`, …): `success` + `output`/`error` + provider provenance.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResolvedOutcome {
    pub success: bool,
    /// Canonical capability id, e.g. `web.search`.
    pub capability: String,
    /// Provider that served the request; empty when nothing succeeded.
    pub provider: String,
    /// Providers attempted in order (for audit / telemetry / fallback trail).
    pub attempted: Vec<String>,
    /// Normalized provider output (already in FRIDAY's result shape).
    pub output: serde_json::Value,
    /// Structured error when every binding failed (never a bare transport
    /// exception).
    pub error: Option<String>,
}

/// The dispatch seam: execute one binding against `params`, returning the
/// normalized output or a string error. Plain function, not a trait — the
/// action runtime wires its adapters here.
pub type Dispatch =
    Arc<dyn Fn(&ExecutionBinding, &serde_json::Value) -> BoxFuture<'static, Result<serde_json::Value, String>>
        + Send
        + Sync>;

/// Thread-safe resolver over candidate bindings keyed by canonical capability
/// id. Registration is `Healthy` by default; probes/orchestration write live
/// health via [`BindingResolver::set_health`].
pub struct BindingResolver {
    candidates: RwLock<HashMap<String, Vec<CandidateBinding>>>,
    policy: Arc<dyn Fn(&ExecutionBinding) -> bool + Send + Sync>,
}

impl Default for BindingResolver {
    fn default() -> Self {
        Self::new()
    }
}

impl BindingResolver {
    pub fn new() -> Self {
        Self::with_policy(Arc::new(|_| true))
    }

    /// Build a resolver whose selection additionally gates every candidate on
    /// a policy predicate (e.g. risk/permission checks). Denied candidates are
    /// treated as unavailable for selection.
    pub fn with_policy(policy: Arc<dyn Fn(&ExecutionBinding) -> bool + Send + Sync>) -> Self {
        Self {
            candidates: RwLock::new(HashMap::new()),
            policy,
        }
    }

    /// Register a candidate binding for a capability. Starts [`CapabilityHealth::Healthy`].
    pub fn register(&self, capability: impl Into<String>, candidate: CandidateBinding) {
        self.candidates
            .write()
            .expect("resolver poisoned")
            .entry(capability.into())
            .or_default()
            .push(candidate);
    }

    /// Convenience: register a binding that is currently *not* healthy.
    pub fn register_with_health(
        &self,
        capability: impl Into<String>,
        provider: impl Into<String>,
        binding: ExecutionBinding,
        health: CapabilityHealth,
    ) {
        self.register(
            capability,
            CandidateBinding {
                provider: provider.into(),
                binding,
                health,
            },
        );
    }

    /// Write live health for one `(capability, provider)` candidate, so the
    /// orchestrator / Monitor can drive selection without re-registering.
    pub fn set_health(&self, capability: &str, provider: &str, health: CapabilityHealth) {
        let mut guard = self.candidates.write().expect("resolver poisoned");
        if let Some(list) = guard.get_mut(capability) {
            if let Some(c) = list.iter_mut().find(|c| c.provider == provider) {
                c.health = health;
            }
        }
    }

    /// The policy-passing, online candidates for a capability, ordered
    /// Healthy-first (registration order within a tier). Offline and
    /// policy-denied candidates are excluded from selection but still reported
    /// so callers can observe *why* a provider was skipped.
    pub fn resolve(&self, capability: &str) -> Resolved {
        let mut ready = Vec::new();
        let mut skipped: Vec<(String, &'static str)> = Vec::new();
        if let Some(list) = self
            .candidates
            .read()
            .expect("resolver poisoned")
            .get(capability)
            .cloned()
        {
            for c in list {
                if c.health == CapabilityHealth::Offline {
                    skipped.push((c.provider.clone(), "offline"));
                    continue;
                }
                if !(self.policy)(&c.binding) {
                    skipped.push((c.provider.clone(), "policy_denied"));
                    continue;
                }
                ready.push(c);
            }
        }
        ready.sort_by_key(|c| !matches!(c.health, CapabilityHealth::Healthy));
        Resolved {
            capability: capability.to_string(),
            ready,
            skipped,
        }
    }

    /// Resolve then dispatch with fallback. Tries each online, policy-passing
    /// candidate in order; the first successful dispatch wins. When every
    /// candidate fails (or none exist), returns a structured [`ResolvedOutcome`]
    /// listing the attempted providers — a clean capability error, not a
    /// propagated transport exception.
    pub async fn execute(
        &self,
        capability: &str,
        params: &serde_json::Value,
        dispatch: &Dispatch,
    ) -> ResolvedOutcome {
        let resolved = self.resolve(capability);
        let mut attempted: Vec<String> = Vec::new();
        let mut failures: Vec<String> = Vec::new();

        for candidate in resolved.ready {
            attempted.push(candidate.provider.clone());
            match dispatch(&candidate.binding, params).await {
                Ok(output) => {
                    return ResolvedOutcome {
                        success: true,
                        capability: capability.to_string(),
                        provider: candidate.provider,
                        attempted,
                        output,
                        error: None,
                    };
                }
                Err(e) => failures.push(format!("{}: {e}", candidate.provider)),
            }
        }

        let error = if !failures.is_empty() {
            format!("capability {capability} unavailable: {} attempted and failed", failures.join("; "))
        } else if attempted.is_empty() {
                let skipped = if resolved.skipped.is_empty() {
                    String::new()
                } else {
                    format!(
                        " (skipped: {})",
                        resolved
                            .skipped
                            .iter()
                            .map(|(p, why)| format!("{p}:{why}"))
                            .collect::<Vec<_>>()
                            .join(", ")
                    )
                };
                format!("capability {capability} unavailable: no online, policy-allowed binding{skipped}")
        } else {
            format!("capability {capability} unavailable")
        };
        ResolvedOutcome {
            success: false,
            capability: capability.to_string(),
            provider: String::new(),
            attempted,
            output: serde_json::Value::Null,
            error: Some(error),
        }
    }

    /// Registered providers for a capability (any health) — for discovery and
    /// audit.
    pub fn providers(&self, capability: &str) -> Vec<String> {
        self.candidates
            .read()
            .expect("resolver poisoned")
            .get(capability)
            .map(|list| list.iter().map(|c| c.provider.clone()).collect())
            .unwrap_or_default()
    }
}

/// The selection result of [`BindingResolver::resolve`].
#[derive(Debug, Clone)]
pub struct Resolved {
    /// Canonical capability id.
    pub capability: String,
    /// Online, policy-passing candidates, Healthy-first.
    pub ready: Vec<CandidateBinding>,
    /// Candidates excluded from selection and why (`offline` / `policy_denied`).
    pub skipped: Vec<(String, &'static str)>,
}

impl Resolved {
    pub fn is_empty(&self) -> bool {
        self.ready.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::spec::Risk;

    fn binding(adapter: &str, action: &str) -> ExecutionBinding {
        ExecutionBinding {
            adapter: adapter.to_string(),
            action: action.to_string(),
            schema: serde_json::Value::Null,
            permissions: Vec::new(),
            risk: Risk::Safe,
        }
    }

    fn fake_ok() -> Dispatch {
        Arc::new(|binding, params| {
            let adapter = binding.adapter.clone();
            let params = params.clone();
            Box::pin(async move {
                Ok(serde_json::json!({
                    "provider": adapter,
                    "echo": params,
                }))
            })
        })
    }

    fn fake_result(results: HashMap<&'static str, Result<serde_json::Value, &'static str>>) -> Dispatch {
        Arc::new(move |binding, _| {
            let key = binding.adapter.clone();
            let outcome = results.get(key.as_str()).cloned();
            Box::pin(async move {
                match outcome {
                    Some(Ok(v)) => Ok(v),
                    Some(Err(e)) => Err(e.to_string()),
                    None => Err(format!("no scripted outcome for {key}")),
                }
            })
        })
    }

    #[tokio::test]
    async fn healthy_primary_is_selected_and_normalized() {
        let r = BindingResolver::new();
        r.register(
            "web.search",
            CandidateBinding {
                provider: "duckduckgo".into(),
                binding: binding("web", "search"),
                health: CapabilityHealth::Healthy,
            },
        );
        r.register(
            "web.search",
            CandidateBinding {
                provider: "jina".into(),
                binding: binding("web", "search"),
                health: CapabilityHealth::Healthy,
            },
        );

        let out = r
            .execute("web.search", &serde_json::json!({ "query": "rust" }), &fake_ok())
            .await;
        assert!(out.success, "out: {out:?}");
        assert_eq!(out.provider, "duckduckgo", "healthy primary wins");
        assert_eq!(out.attempted, vec!["duckduckgo"]);
        assert_eq!(out.output["provider"], "web");
    }

    #[tokio::test]
    async fn offline_primary_falls_back_to_healthy() {
        // The acceptance test: primary offline → resolver selects fallback →
        // search succeeds → normalized evidence returned.
        let r = BindingResolver::new();
        r.register_with_health(
            "web.search",
            "duckduckgo",
            binding("web", "search"),
            CapabilityHealth::Offline,
        );
        r.register_with_health(
            "web.search",
            "jina",
            binding("web", "search"),
            CapabilityHealth::Healthy,
        );

        let out = r
            .execute("web.search", &serde_json::json!({ "query": "rust" }), &fake_ok())
            .await;
        assert!(out.success, "fallback must succeed: {out:?}");
        assert_eq!(out.provider, "jina", "fallback selected");
        assert_eq!(out.attempted, vec!["jina"], "offline primary is never attempted");
        assert_eq!(out.output["echo"]["query"], "rust", "normalized evidence returned");
    }

    #[tokio::test]
    async fn degraded_primary_defers_to_healthy_fallback() {
        let r = BindingResolver::new();
        r.register_with_health(
            "web.search",
            "duckduckgo",
            binding("ddg", "search"),
            CapabilityHealth::Degraded,
        );
        r.register_with_health(
            "web.search",
            "jina",
            binding("jina", "search"),
            CapabilityHealth::Healthy,
        );

        // Healthy outranks Degraded: a degraded primary is still offered, but
        // ranked below the healthy fallback.
        let resolved = r.resolve("web.search");
        assert_eq!(resolved.ready.len(), 2);
        assert_eq!(resolved.ready[0].provider, "jina", "healthy wins over degraded");

        let out = r
            .execute("web.search", &serde_json::json!({}), &fake_ok())
            .await;
        assert!(out.success);
        assert_eq!(out.provider, "jina");
    }

    #[tokio::test]
    async fn degraded_outranks_offline() {
        let r = BindingResolver::new();
        r.register_with_health(
            "web.search",
            "duckduckgo",
            binding("ddg", "search"),
            CapabilityHealth::Degraded,
        );
        r.register_with_health(
            "web.search",
            "jina",
            binding("jina", "search"),
            CapabilityHealth::Offline,
        );

        let resolved = r.resolve("web.search");
        assert_eq!(resolved.ready.len(), 1, "offline is excluded from selection");
        assert_eq!(resolved.ready[0].provider, "duckduckgo");
        assert!(
            resolved.skipped.iter().any(|(p, why)| p == "jina" && *why == "offline"),
            "offline provider recorded as skipped"
        );
    }

    #[tokio::test]
    async fn primary_execution_failure_falls_back() {
        let r = BindingResolver::new();
        r.register_with_health(
            "web.search",
            "duckduckgo",
            binding("ddg", "search"),
            CapabilityHealth::Healthy,
        );
        r.register_with_health(
            "web.search",
            "jina",
            binding("jina", "search"),
            CapabilityHealth::Healthy,
        );

        let mut results = HashMap::new();
        results.insert("ddg", Err::<serde_json::Value, _>("500 upstream"));
        results.insert("jina", Ok::<serde_json::Value, _>(serde_json::json!({ "hits": [] })));
        let dispatch = fake_result(results);

        let out = r
            .execute("web.search", &serde_json::json!({}), &dispatch)
            .await;
        assert!(out.success, "fallback must rescue a failed primary: {out:?}");
        assert_eq!(out.provider, "jina");
        assert_eq!(out.attempted, vec!["duckduckgo", "jina"], "failure trail recorded");
    }

    #[tokio::test]
    async fn all_online_providers_failing_is_a_structured_error() {
        let r = BindingResolver::new();
        r.register_with_health(
            "web.search",
            "duckduckgo",
            binding("ddg", "search"),
            CapabilityHealth::Healthy,
        );
        r.register_with_health(
            "web.search",
            "jina",
            binding("jina", "search"),
            CapabilityHealth::Healthy,
        );

        let mut results = HashMap::new();
        results.insert("ddg", Err::<serde_json::Value, _>("boom"));
        results.insert("jina", Err::<serde_json::Value, _>("boom"));
        let dispatch = fake_result(results);

        let out = r
            .execute("web.search", &serde_json::json!({}), &dispatch)
            .await;
        assert!(!out.success);
        let err = out.error.expect("structured error");
        assert!(err.contains("web.search unavailable"), "err: {err}");
        assert!(err.contains("duckduckgo"), "err: {err}");
        assert!(err.contains("jina"), "err: {err}");
        assert_eq!(out.attempted.len(), 2);
    }

    #[tokio::test]
    async fn all_offline_is_a_clean_capability_error() {
        let r = BindingResolver::new();
        r.register_with_health(
            "web.search",
            "duckduckgo",
            binding("web", "search"),
            CapabilityHealth::Offline,
        );
        r.register_with_health(
            "web.fetch",
            "duckduckgo",
            binding("web", "fetch"),
            CapabilityHealth::Offline,
        );

        let out = r
            .execute("web.search", &serde_json::json!({}), &fake_ok())
            .await;
        assert!(!out.success);
        let err = out.error.expect("structured error");
        assert!(err.contains("no online, policy-allowed binding"), "err: {err}");
        assert!(err.contains("duckduckgo:offline"), "err: {err}");
    }

    #[tokio::test]
    async fn policy_denied_candidates_are_skipped() {
        let r = BindingResolver::with_policy(Arc::new(|b| b.risk != Risk::High));
        r.register_with_health(
            "web.search",
            "duckduckgo",
            ExecutionBinding {
                adapter: "web".into(),
                action: "search".into(),
                schema: serde_json::Value::Null,
                permissions: vec![],
                risk: Risk::High,
            },
            CapabilityHealth::Healthy,
        );
        r.register_with_health(
            "web.search",
            "jina",
            binding("web", "search"),
            CapabilityHealth::Healthy,
        );

        let resolved = r.resolve("web.search");
        assert!(resolved.skipped.iter().any(|(p, why)| p == "duckduckgo" && *why == "policy_denied"));
        assert_eq!(resolved.ready.len(), 1);
        assert_eq!(resolved.ready[0].provider, "jina");
    }

    #[tokio::test]
    async fn set_health_updates_selection_live() {
        let r = BindingResolver::new();
        r.register_with_health(
            "web.search",
            "duckduckgo",
            binding("web", "search"),
            CapabilityHealth::Healthy,
        );
        r.register_with_health(
            "web.search",
            "jina",
            binding("web", "search"),
            CapabilityHealth::Healthy,
        );

        r.set_health("web.search", "duckduckgo", CapabilityHealth::Offline);
        let out = r
            .execute("web.search", &serde_json::json!({}), &fake_ok())
            .await;
        assert!(out.success);
        assert_eq!(out.provider, "jina", "offline primary bypassed after live health update");
    }

    #[tokio::test]
    async fn unknown_capability_is_a_clean_error() {
        let r = BindingResolver::new();
        let out = r
            .execute("web.search", &serde_json::json!({}), &fake_ok())
            .await;
        assert!(!out.success);
        assert_eq!(out.error.unwrap(), "capability web.search unavailable: no online, policy-allowed binding");
    }
}
