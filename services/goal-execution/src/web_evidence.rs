//! Resolver-backed `web.search` evidence — the wiring half of Phase A.2.
//!
//! The [`ResolvingWebTool`] sits where the planner's evidence seam already is
//! (the same `Tool` the `AdapterTool` uses) but routes `web.search` through a
//! [`BindingResolver`](friday_capabilities::BindingResolver) instead of a
//! single hardwired adapter:
//!
//! ```text
//! Planner
//!   ↓  "gather evidence: web (web.search)"
//! ToolRegistry → ResolvingWebTool
//!   ↓
//! BindingResolver (CandidateBinding: health + policy)
//!   ↓
//! Action Runtime (WebAdapter → search endpoint)
//!   ↓
//! normalized Evidence[]
//! ```
//!
//! **The guardrail this module enforces:** provider output never leaks upward.
//! The resolver returns the *winning provider's* raw adapter output; this tool
//! normalizes it into [`WebEvidence`] — `source`, `title`, `url`, `content`,
//! `retrieved_at`, `confidence` — so the cognitive layer blends and verifies
//! evidence, not `DuckDuckGoResponse`/`JinaResponse` shapes. Whatever scraper
//! happened to win is metadata (`source`), not architecture.
//!
//! A.2b wires `web.search` only. `web.fetch` deliberately stays on the plain
//! evidence adapter (see the A.2b guardrail) until the fetch adapter is wired
//! through the same resolver in a later pass — this tool refuses any
//! non-`web.search` instruction honestly rather than pretending to serve it.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use friday_actions::{ActionAdapter, ActionRequest};
use friday_capabilities::spec::{CapabilityHealth, ExecutionBinding, Risk};
use friday_capabilities::BindingResolver;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::common::{Action, Capability, ExecutionContext};
use crate::verification::{Tool, ToolResult};

/// Reserved parameter keys the tool's [`Tool::execute`] uses to thread the
/// [`ExecutionContext`]'s timeout and permissions into the dispatch closure —
/// which, per the resolver's frozen contract, only ever receives
/// `(&CandidateBinding, &Value)`. They are stripped before the adapter sees
/// the parameters, so the adapter's contract (`query`, `endpoint`, `limit`)
/// stays clean. Mirrors what [`AdapterTool`](crate::AdapterTool) forwards onto
/// its `ActionRequest` so the resolver path loses none of the context.
const RESERVED_TIMEOUT_MS: &str = "__timeout_ms";
const RESERVED_PERMISSIONS: &str = "__permissions";

/// One provider configuration for `web.search`. Each provider is an endpoint
/// the existing [`WebAdapter`](friday_actions::WebAdapter) can be pointed at;
/// the resolver owns which is healthy and therefore selected.
#[derive(Debug, Clone)]
pub struct WebSearchProvider {
    pub name: String,
    /// Search endpoint (the `endpoint` param the adapter honors).
    pub endpoint: String,
    pub health: CapabilityHealth,
}

impl WebSearchProvider {
    pub fn new(name: impl Into<String>, endpoint: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            endpoint: endpoint.into(),
            health: CapabilityHealth::Healthy,
        }
    }
}

/// Normalized web evidence — the only shape the cognitive layer ever sees.
/// Provider identity survives only as `source`; there are no provider-specific
/// response structs.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebEvidence {
    /// Provider that produced this hit (e.g. `duckduckgo`).
    pub source: String,
    pub title: String,
    pub url: String,
    pub content: String,
    pub retrieved_at: DateTime<Utc>,
    /// Retrieval confidence (the verification phase later assesses truth).
    pub confidence: f64,
}

/// Resolver-backed web search tool, registered for `web.search` in the
/// evidence [`ToolRegistry`](crate::ToolRegistry).
pub struct ResolvingWebTool {
    name: String,
    resolver: BindingResolver,
    dispatch: friday_capabilities::Dispatch,
    capabilities: Vec<Capability>,
}

impl ResolvingWebTool {
    /// Build a `web.search` tool over a real action-runtime adapter and the
    /// given providers. Providers are registered in order (first healthy wins;
    /// health can be updated live via [`ResolvingWebTool::set_health`]).
    pub fn for_web_search(
        adapter: Arc<dyn ActionAdapter>,
        providers: Vec<WebSearchProvider>,
    ) -> Self {
        let resolver = BindingResolver::new();
        for p in &providers {
            resolver.register_with_health(
                "web.search",
                &p.name,
                ExecutionBinding {
                    adapter: adapter.tool().to_string(),
                    action: "search".to_string(),
                    schema: json!({ "query": "string" }),
                    permissions: Vec::new(),
                    risk: Risk::Safe,
                },
                p.health,
            );
        }

        let endpoints: HashMap<String, String> = providers
            .into_iter()
            .map(|p| (p.name.clone(), p.endpoint.clone()))
            .collect();
        let dispatch: friday_capabilities::Dispatch =
            Arc::new(move |candidate: &friday_capabilities::CandidateBinding, params: &serde_json::Value| {
                let adapter = adapter.clone();
                let adapter_name = candidate.binding.adapter.clone();
                let action = candidate.binding.action.clone();
                let endpoint = endpoints.get(&candidate.provider).cloned();
                // Reserved context keys injected by `execute` (see the const
                // docs above). Read them out, strip them, and forward onto the
                // ActionRequest exactly as `AdapterTool::route` does.
                let timeout_ms = params.get(RESERVED_TIMEOUT_MS).and_then(|v| v.as_u64());
                let permissions: Vec<String> = params
                    .get(RESERVED_PERMISSIONS)
                    .and_then(|v| v.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|v| v.as_str().map(str::to_string))
                            .collect()
                    })
                    .unwrap_or_default();
                let mut parameters = params.clone();
                if let Some(map) = parameters.as_object_mut() {
                    map.remove(RESERVED_TIMEOUT_MS);
                    map.remove(RESERVED_PERMISSIONS);
                }
                if let Some(e) = endpoint {
                    parameters["endpoint"] = json!(e);
                }
                Box::pin(async move {
                    let mut req = ActionRequest::new(adapter_name, action, parameters);
                    if let Some(ms) = timeout_ms {
                        req.timeout = Some(std::time::Duration::from_millis(ms));
                    }
                    req.permissions = permissions;
                    let result = adapter.execute(req).await;
                    if result.success {
                        Ok(result.output)
                    } else {
                        Err(result
                            .error
                            .unwrap_or_else(|| "web adapter returned no error".to_string()))
                    }
                })
            });

        Self {
            name: "evidence:web".to_string(),
            resolver,
            dispatch,
            capabilities: vec![Capability::Custom("web.search".into())],
        }
    }

    /// Live health update, forwarded to the resolver — an orchestrator / health
    /// Monitor can mark a provider offline without rebuilding the tool.
    pub fn set_health(&self, provider: &str, health: CapabilityHealth) {
        self.resolver.set_health("web.search", provider, health);
    }

    /// Providers currently registered for `web.search` (any health) — for
    /// discovery and audit.
    pub fn providers(&self) -> Vec<String> {
        self.resolver.providers("web.search")
    }
}

#[async_trait]
impl Tool for ResolvingWebTool {
    fn name(&self) -> &str {
        &self.name
    }

    fn description(&self) -> &str {
        "Resolver-backed web search evidence (web.search) with health-gated \
         provider fallback; returns normalized Evidence[]"
    }

    fn capabilities(&self) -> Vec<Capability> {
        self.capabilities.clone()
    }

    async fn execute(&self, action: &Action, context: &ExecutionContext) -> ToolResult {
        let Some((tool, verb)) = crate::evidence::parse_evidence_id(&action.instruction) else {
            return ToolResult::error(format!(
                "{} cannot execute '{}' — not an evidence action (gather evidence: <channel> (<tool>.<verb>))",
                self.name, action.instruction
            ));
        };
        if tool != "web" || verb != "search" {
            return ToolResult::error(format!(
                "{} only serves web.search, not {tool}.{verb}",
                self.name
            ));
        }

        // Thread the context's timeout/permissions to the dispatch (which sees
        // only params) via reserved keys — the same contract `AdapterTool::route`
        // forwards, so the resolver path drops nothing.
        let mut parameters = action.parameters.clone();
        if let Some(ms) = context.timeout_ms {
            parameters[RESERVED_TIMEOUT_MS] = json!(ms);
        }
        if !context.permissions.is_empty() {
            parameters[RESERVED_PERMISSIONS] = json!(context.permissions);
        }

        let started = Instant::now();
        let outcome = self
            .resolver
            .execute("web.search", &parameters, &self.dispatch)
            .await;
        let duration_ms = started.elapsed().as_millis() as u64;

        if !outcome.success {
            // Structured failure: the error text plus the attempted-provider
            // trail in metadata — a clean capability error, never a bare
            // transport exception leaking up.
            return ToolResult {
                success: false,
                output: String::new(),
                error: outcome.error.clone(),
                duration_ms,
                metadata: json!({ "attempted": outcome.attempted }),
            };
        }

        let evidence = normalize_search_evidence(&outcome);
        let output = serde_json::to_string_pretty(&evidence)
            .unwrap_or_else(|_| json!([]).to_string());
        ToolResult {
            success: true,
            output,
            error: None,
            duration_ms,
            // Winner + failure trail as metadata: observable for audit and
            // telemetry without leaking provider-specific output into the
            // evidence the cognitive layer consumes.
            metadata: json!({
                "provider": outcome.provider,
                "attempted": outcome.attempted,
            }),
        }
    }
}

/// Normalize a successful resolver outcome into [`WebEvidence`] — the single
/// provider-agnostic evidence shape. `content` is populated when the provider
/// supplies it; search hits carry title/url only.
pub fn normalize_search_evidence(outcome: &friday_capabilities::ResolvedOutcome) -> serde_json::Value {
    let query = outcome
        .output
        .get("query")
        .cloned()
        .unwrap_or(json!(null));
    let results = outcome
        .output
        .get("results")
        .and_then(|r| r.as_array())
        .cloned()
        .unwrap_or_default();
    let evidence: Vec<WebEvidence> = results
        .into_iter()
        .map(|r| WebEvidence {
            source: outcome.provider.clone(),
            title: r.get("title").and_then(|v| v.as_str()).unwrap_or("").to_string(),
            url: r.get("url").and_then(|v| v.as_str()).unwrap_or("").to_string(),
            content: r.get("content").and_then(|v| v.as_str()).unwrap_or("").to_string(),
            retrieved_at: Utc::now(),
            confidence: 1.0,
        })
        .collect();
    json!({ "query": query, "evidence": evidence })
}

#[cfg(test)]
mod tests {
    use super::*;
    use friday_actions::WebAdapter;
    use serde_json::Value;

    // ── Helpers ──────────────────────────────────────────────────

    /// Serve one canned HTML page on an ephemeral **loop** server — every
    /// request gets the same body (the adapter may follow up, and the resolver
    /// may retry a provider), so the server must not exit after one connection.
    async fn serve_html(html: &'static str) -> String {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            loop {
                let Ok((mut sock, _)) = listener.accept().await else {
                    break;
                };
                tokio::spawn(async move {
                    let mut buf = [0u8; 4096];
                    let _ = sock.read(&mut buf).await;
                    let body = html.as_bytes();
                    let head = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        body.len()
                    );
                    let _ = sock.write_all(head.as_bytes()).await;
                    let _ = sock.write_all(body).await;
                });
            }
        });
        format!("http://{addr}/")
    }

    /// A guaranteed-dead endpoint: bind an ephemeral port, take its address,
    /// then drop the listener so connections are refused. Used to make a
    /// provider *fail at execution time* so the resolver must fall back.
    fn dead_endpoint() -> String {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind dead port");
        let addr = listener.local_addr().expect("dead port address");
        drop(listener);
        format!("http://{addr}/")
    }

    const RESULTS_HTML: &str = r#"<html><body>
        <a href="https://doc.rust-lang.org/book/">The Rust Book</a>
        <a href="https://docs.rs/tokio/latest/tokio/">Tokio documentation</a>
    </body></html>"#;

    fn evidence_action() -> Action {
        Action::single(
            "gather evidence: web (web.search)",
            Capability::Custom("web.search".into()),
            "g1",
        )
        .with_parameters(json!({ "query": "rust async" }))
    }

    fn parsed_output(result: &ToolResult) -> Value {
        serde_json::from_str(&result.output).expect("tool output is JSON")
    }

    // ── Guardrail: provider output never leaks upward ─────────────

    #[test]
    fn normalize_search_evidence_emits_provider_agnostic_shape() {
        let outcome = friday_capabilities::ResolvedOutcome {
            success: true,
            capability: "web.search".into(),
            provider: "duckduckgo".into(),
            attempted: vec!["duckduckgo".into()],
            output: json!({
                "query": "rust",
                "results": [
                    { "title": "The Rust Book", "url": "https://doc.rust-lang.org/book/" },
                    { "title": "Tokio docs", "url": "https://docs.rs/tokio" },
                ],
                // Provider-specific noise must never reach the cognitive layer.
                "response_id": "x-123",
                "provider_specific": { "ranking": [1, 2] },
            }),
            error: None,
        };

        let v = normalize_search_evidence(&outcome);
        let obj = v.as_object().expect("normalized object");
        // Only the two contract keys escape: query + evidence[].
        let mut keys: Vec<&String> = obj.keys().collect();
        keys.sort_unstable();
        assert_eq!(
            keys,
            vec![&"evidence".to_string(), &"query".to_string()],
            "provider shape must not leak: {obj:?}"
        );
        assert_eq!(obj["query"], "rust");
        let evidence = obj["evidence"].as_array().expect("evidence array");
        assert_eq!(evidence.len(), 2);
        for hit in evidence {
            let hit = hit.as_object().expect("evidence hit object");
            for key in [
                "source",
                "title",
                "url",
                "content",
                "retrieved_at",
                "confidence",
            ] {
                assert!(hit.contains_key(key), "evidence carries {key}: {hit:?}");
            }
            assert_eq!(hit["source"], "duckduckgo", "provider survives as source only");
            assert_eq!(hit["confidence"], 1.0);
        }
        assert_eq!(evidence[0]["title"], "The Rust Book");
        assert_eq!(evidence[0]["url"], "https://doc.rust-lang.org/book/");
        assert_eq!(evidence[1]["url"], "https://docs.rs/tokio");
    }

    // ── Acceptance 1: web.search → resolver → healthy provider → Evidence ──

    #[tokio::test]
    async fn web_search_resolves_through_healthy_provider_to_normalized_evidence() {
        let base = serve_html(RESULTS_HTML).await;
        let tool = ResolvingWebTool::for_web_search(
            Arc::new(WebAdapter::new()),
            vec![WebSearchProvider::new("duckduckgo", base)],
        );

        let result = tool.execute(&evidence_action(), &ExecutionContext::for_goal("g1")).await;
        assert!(result.success, "end-to-end web.search failed: {:?}", result.error);

        let output = parsed_output(&result);
        let evidence = output["evidence"].as_array().expect("evidence array");
        assert_eq!(evidence.len(), 2, "both mock hits normalized: {output:?}");
        assert_eq!(
            evidence[0]["source"], "duckduckgo",
            "the winning provider is the source"
        );
        assert_eq!(evidence[0]["title"], "The Rust Book");
        assert!(evidence[0]["url"].as_str().unwrap().starts_with("https://"));
        assert!(evidence[0]["retrieved_at"].is_string());
        assert_eq!(evidence[0]["confidence"], 1.0);
        assert!(
            output.get("results").is_none(),
            "raw adapter shape must not leak: {output:?}"
        );
        // The winner and the failure trail are observable as metadata.
        assert_eq!(result.metadata["provider"], "duckduckgo");
        assert_eq!(
            result.metadata["attempted"],
            json!(["duckduckgo"]),
            "single-provider trail"
        );
    }

    // ── Acceptance 2: primary fails → resolver → fallback → Evidence ──
    //
    // The test that turns the resolver from a routing library into an actual
    // FRIDAY capability: the healthy primary's endpoint refuses connections,
    // the resolver falls back to the secondary provider, and the search still
    // completes with normalized evidence sourced from the fallback.

    #[tokio::test]
    async fn primary_failure_falls_back_to_secondary_provider() {
        let base = serve_html(RESULTS_HTML).await;
        let tool = ResolvingWebTool::for_web_search(
            Arc::new(WebAdapter::new()),
            vec![
                WebSearchProvider::new("primary", dead_endpoint()),
                WebSearchProvider::new("fallback", base),
            ],
        );

        let result = tool.execute(&evidence_action(), &ExecutionContext::for_goal("g1")).await;
        assert!(
            result.success,
            "fallback must rescue the failed primary: {:?}",
            result.error
        );

        let output = parsed_output(&result);
        let evidence = output["evidence"].as_array().expect("evidence array");
        assert_eq!(evidence.len(), 2, "fallback served real results: {output:?}");
        assert_eq!(
            evidence[0]["source"], "fallback",
            "the winning provider is the fallback"
        );
        assert_eq!(evidence[0]["title"], "The Rust Book");
        assert_eq!(result.metadata["provider"], "fallback");
        assert_eq!(
            result.metadata["attempted"],
            json!(["primary", "fallback"]),
            "the full failure trail is recorded"
        );
    }

    // ── Health-gated fallback is live, not just registration-time ──

    #[tokio::test]
    async fn live_health_update_reroutes_selection() {
        let base = serve_html(RESULTS_HTML).await;
        let tool = ResolvingWebTool::for_web_search(
            Arc::new(WebAdapter::new()),
            vec![
                WebSearchProvider::new("primary", base.clone()),
                WebSearchProvider::new("backup", base),
            ],
        );
        assert_eq!(
            tool.providers(),
            vec!["primary".to_string(), "backup".to_string()]
        );

        // An orchestrator marks the primary offline → the backup serves.
        tool.set_health("primary", CapabilityHealth::Offline);
        let result = tool.execute(&evidence_action(), &ExecutionContext::for_goal("g1")).await;
        assert!(result.success, "health-updated fallback failed: {:?}", result.error);
        let output = parsed_output(&result);
        assert_eq!(
            output["evidence"][0]["source"], "backup",
            "offline primary is bypassed after the live health update"
        );
        assert_eq!(result.metadata["provider"], "backup");
    }

    // ── Structured failure when every provider fails ──────────────

    #[tokio::test]
    async fn all_providers_failing_is_a_structured_capability_error() {
        let tool = ResolvingWebTool::for_web_search(
            Arc::new(WebAdapter::new()),
            vec![
                WebSearchProvider::new("primary", dead_endpoint()),
                WebSearchProvider::new("fallback", dead_endpoint()),
            ],
        );

        let result = tool.execute(&evidence_action(), &ExecutionContext::for_goal("g1")).await;
        assert!(!result.success);
        let err = result.error.expect("structured error");
        assert!(err.contains("web.search unavailable"), "err: {err}");
        assert!(err.contains("primary"), "err: {err}");
        assert!(err.contains("fallback"), "err: {err}");
        assert_eq!(
            result.metadata["attempted"],
            json!(["primary", "fallback"]),
            "the failed trail is still observable"
        );
    }

    #[tokio::test]
    async fn no_providers_is_a_clean_capability_error() {
        let tool = ResolvingWebTool::for_web_search(Arc::new(WebAdapter::new()), vec![]);
        let result = tool.execute(&evidence_action(), &ExecutionContext::for_goal("g1")).await;
        assert!(!result.success);
        assert!(
            result
                .error
                .unwrap()
                .contains("no online, policy-allowed binding"),
            "no providers ⇒ clean error, not a panic"
        );
    }

    // ── A.2b scope: this tool serves web.search ONLY ──────────────

    #[tokio::test]
    async fn refuses_non_web_search_instructions_honestly() {
        let tool = ResolvingWebTool::for_web_search(
            Arc::new(WebAdapter::new()),
            vec![WebSearchProvider::new("duckduckgo", dead_endpoint())],
        );
        let ctx = ExecutionContext::for_goal("g1");

        // A different channel must be refused, not misrouted.
        let git = Action::single(
            "gather evidence: git (git.status)",
            Capability::Custom("git.status".into()),
            "g1",
        );
        let r = tool.execute(&git, &ctx).await;
        assert!(!r.success);
        assert!(r.error.unwrap().contains("only serves web.search"));

        // web.fetch is deliberately NOT wired through the resolver in A.2b —
        // the tool must say so rather than pretend to serve it.
        let fetch = Action::single(
            "gather evidence: web (web.fetch)",
            Capability::Custom("web.fetch".into()),
            "g1",
        );
        let r = tool.execute(&fetch, &ctx).await;
        assert!(!r.success);
        assert!(r.error.unwrap().contains("only serves web.search"));

        // A non-evidence instruction is refused too.
        let r = tool.execute(
            &Action::single("run cargo test", Capability::ExecuteCommand, "g1"),
            &ctx,
        )
        .await;
        assert!(!r.success);
        assert!(r.error.unwrap().contains("not an evidence action"));
    }

    // ── Context fidelity: timeout + permissions reach the ActionRequest ──

    /// Captures the `ActionRequest` it receives so a test can assert the
    /// dispatch forwarded the execution context's timeout/permissions — the
    /// same contract [`AdapterTool`](crate::AdapterTool) honors.
    struct RecordingAdapter {
        last: Arc<std::sync::Mutex<Option<ActionRequest>>>,
    }

    #[async_trait]
    impl ActionAdapter for RecordingAdapter {
        fn tool(&self) -> &str {
            "web"
        }

        fn actions(&self) -> Vec<&str> {
            vec!["search"]
        }

        async fn execute(&self, req: ActionRequest) -> friday_actions::ActionResult {
            *self.last.lock().unwrap() = Some(req);
            friday_actions::ActionResult::success(json!({ "query": "q", "results": [] }))
        }
    }

    #[tokio::test]
    async fn execution_context_timeout_and_permissions_are_forwarded() {
        let last = Arc::new(std::sync::Mutex::new(None));
        let tool = ResolvingWebTool::for_web_search(
            Arc::new(RecordingAdapter { last: last.clone() }),
            vec![WebSearchProvider::new("duckduckgo", "http://127.0.0.1:9/")],
        );
        let ctx = ExecutionContext::for_goal("g1")
            .with_timeout_ms(5000)
            .with_permissions(vec!["web.search".into()]);

        let result = tool.execute(&evidence_action(), &ctx).await;
        assert!(result.success, "recording adapter must succeed: {:?}", result.error);

        let req = last.lock().unwrap().take().expect("adapter received a request");
        assert_eq!(
            req.timeout,
            Some(std::time::Duration::from_millis(5000)),
            "context timeout forwarded onto the ActionRequest"
        );
        assert_eq!(
            req.permissions,
            vec!["web.search".to_string()],
            "context permissions forwarded onto the ActionRequest"
        );
        // The reserved keys never reach the adapter's parameter contract.
        let params = req.parameters.as_object().expect("params object");
        assert!(!params.contains_key("__timeout_ms"), "reserved key stripped: {params:?}");
        assert!(!params.contains_key("__permissions"), "reserved key stripped: {params:?}");
        assert_eq!(params["endpoint"], "http://127.0.0.1:9/");
        assert_eq!(params["query"], "rust async");
    }
}
