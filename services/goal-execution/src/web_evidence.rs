//! Resolver-backed web evidence — the wiring half of Phase A.2.
//!
//! The [`ResolvingWebTool`] sits where the planner's evidence seam already is
//! (the same `Tool` the `AdapterTool` uses) but routes `web.search` and
//! `web.fetch` through a [`BindingResolver`](friday_capabilities::BindingResolver)
//! instead of a single hardwired adapter:
//!
//! ```text
//! Planner
//!   ↓  "gather evidence: web (web.search | web.fetch)"
//! ToolRegistry → ResolvingWebTool
//!   ↓
//! BindingResolver (CandidateBinding: health + policy)
//!   ↓
//! Action Runtime (WebAdapter → search endpoint | page fetch)
//!   ↓
//! normalized Evidence[]
//! ```
//!
//! **The guardrail this module enforces:** provider output never leaks upward.
//! The resolver returns the *winning provider's* raw adapter output; this tool
//! normalizes it into [`WebEvidence`] — `source`, `title`, `url`, `content`,
//! `retrieved_at`, `confidence` — so the cognitive layer blends and verifies
//! evidence, not `DuckDuckGoResponse`/`JinaResponse`/`FirecrawlResponse`
//! shapes. Whatever scraper happened to win is metadata (`source`), not
//! architecture.
//!
//! A.2b wired `web.search`; A.2c wires `web.fetch` through the same resolver.
//! A search-only tool (see [`ResolvingWebTool::for_web_search`]) refuses a
//! fetch instruction honestly; the combined tool
//! ([`ResolvingWebTool::for_web`]) serves both verbs with the same
//! health-gated fallback.

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
/// the parameters, so the adapter's contract (`query`, `endpoint`, `limit`,
/// `url`) stays clean. Mirrors what [`AdapterTool`](crate::AdapterTool)
/// forwards onto its `ActionRequest` so the resolver path loses none of the
/// context.
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

/// The canonical Jina Reader gateway prefix. The target URL is percent-encoded
/// and appended, so a fetch of `https://example.com/a?b=c` composes
/// `https://r.jina.ai/https%3A%2F%2Fexample.com%2Fa%3Fb%3Dc` — the gateway
/// receives exactly one unbreakable path component (no raw `?`/`&`/`#`/space
/// survives to be misinterpreted). Free without an API key (rate-limited).
pub const JINA_READER_PREFIX: &str = "https://r.jina.ai/";

/// One provider configuration for `web.fetch`. Providers differ in *how* the
/// target page is fetched: the `url_prefix` is prepended to the target URL to
/// form the URL the adapter GETs. Empty prefix = direct fetch (the plain HTTP
/// path); a non-empty prefix = a **gateway** — a reader service such as
/// `https://r.jina.ai/` ([`JINA_READER_PREFIX`]) that is given the target
/// embedded in its URL. The resolver owns which provider is healthy and
/// therefore selected.
///
/// **Gateway encoding contract:** composition is handled by the dispatch
/// ([`compose_gateway_url`]) — for a gateway the target is **percent-encoded**
/// before appending, so a target containing `?`, `&`, `#`, spaces, or non-ASCII
/// bytes can never be misinterpreted by the gateway's URL parsing. The direct
/// provider (empty prefix) is fetched verbatim.
#[derive(Debug, Clone)]
pub struct WebFetchProvider {
    pub name: String,
    /// Prefix prepended to the target URL (a gateway template); empty = direct.
    pub url_prefix: String,
    pub health: CapabilityHealth,
}

impl WebFetchProvider {
    pub fn new(name: impl Into<String>, url_prefix: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            url_prefix: url_prefix.into(),
            health: CapabilityHealth::Healthy,
        }
    }

    /// The canonical gateway shape: the dispatch percent-encodes the target
    /// before appending it to `prefix` (see [`compose_gateway_url`]). Use for
    /// any reader-service gateway, e.g. `WebFetchProvider::gateway("jina",
    /// JINA_READER_PREFIX)`.
    pub fn gateway(name: impl Into<String>, url_prefix: impl Into<String>) -> Self {
        Self::new(name, url_prefix)
    }
}

/// Compose the URL the adapter GETs for a fetch, from a provider's `url_prefix`
/// and the target URL:
///
/// - **empty prefix (direct provider):** the target is returned **verbatim** —
///   a direct fetch must GET exactly what was asked.
/// - **non-empty prefix (gateway provider):** `prefix` + the target
///   **percent-encoded** (RFC 3986 component encoding) — the gateway receives
///   the target as a single opaque component, so embedded `?`, `&`, `#`,
///   spaces, or non-ASCII bytes can never be misread as gateway syntax.
///
/// `target` must be the **raw, unencoded** URL (as the planner supplied it) —
/// passing an already-encoded URL double-encodes it (`%` → `%25`).
///
/// Jina Reader canonical form: `https://r.jina.ai/https%3A%2F%2Fexample.com%2F…`.
pub fn compose_gateway_url(prefix: &str, target: &str) -> String {
    if prefix.is_empty() {
        target.to_string()
    } else {
        format!("{prefix}{}", friday_actions::urlencode(target))
    }
}

/// Normalized web evidence — the only shape the cognitive layer ever sees.
/// Provider identity survives only as `source`; there are no provider-specific
/// response structs.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebEvidence {
    /// Provider that produced this hit (e.g. `duckduckgo`, `direct`).
    pub source: String,
    pub title: String,
    pub url: String,
    pub content: String,
    pub retrieved_at: DateTime<Utc>,
    /// Retrieval confidence (the verification phase later assesses truth).
    pub confidence: f64,
}

/// Resolver-backed web evidence tool, registered for `web.search` / `web.fetch`
/// in the evidence [`ToolRegistry`](crate::ToolRegistry).
pub struct ResolvingWebTool {
    name: String,
    resolver: BindingResolver,
    dispatch: friday_capabilities::Dispatch,
    capabilities: Vec<Capability>,
}

impl ResolvingWebTool {
    /// Build a `web.search`-only tool over a real action-runtime adapter and
    /// the given providers (A.2b shape). Providers are registered in order
    /// (first healthy wins; health can be updated live via
    /// [`ResolvingWebTool::set_health`]).
    pub fn for_web_search(
        adapter: Arc<dyn ActionAdapter>,
        providers: Vec<WebSearchProvider>,
    ) -> Self {
        Self::new(
            adapter,
            providers,
            Vec::new(),
            vec![Capability::Custom("web.search".into())],
        )
    }

    /// Build a `web.fetch`-only tool (A.2c shape).
    pub fn for_web_fetch(
        adapter: Arc<dyn ActionAdapter>,
        providers: Vec<WebFetchProvider>,
    ) -> Self {
        Self::new(
            adapter,
            Vec::new(),
            providers,
            vec![Capability::Custom("web.fetch".into())],
        )
    }

    /// The production shape: one tool serving both `web.search` and
    /// `web.fetch` with the same health-gated fallback. Registered as
    /// `evidence:web` and advertising both verbs, it replaces the plain
    /// adapter tool for the web evidence channel entirely.
    pub fn for_web(
        adapter: Arc<dyn ActionAdapter>,
        search: Vec<WebSearchProvider>,
        fetch: Vec<WebFetchProvider>,
    ) -> Self {
        Self::new(
            adapter,
            search,
            fetch,
            vec![
                Capability::Custom("web.search".into()),
                Capability::Custom("web.fetch".into()),
            ],
        )
    }

    /// Shared constructor: registers one candidate per provider under its
    /// capability (`web.search` / `web.fetch`), builds a dispatch that routes
    /// on the binding's action verb, and advertises exactly the capabilities
    /// passed in.
    fn new(
        adapter: Arc<dyn ActionAdapter>,
        search: Vec<WebSearchProvider>,
        fetch: Vec<WebFetchProvider>,
        capabilities: Vec<Capability>,
    ) -> Self {
        let resolver = BindingResolver::new();
        for p in &search {
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
        for p in &fetch {
            resolver.register_with_health(
                "web.fetch",
                &p.name,
                ExecutionBinding {
                    adapter: adapter.tool().to_string(),
                    action: "fetch".to_string(),
                    schema: json!({ "url": "string" }),
                    permissions: Vec::new(),
                    risk: Risk::Safe,
                },
                p.health,
            );
        }

        let search_endpoints: HashMap<String, String> = search
            .iter()
            .map(|p| (p.name.clone(), p.endpoint.clone()))
            .collect();
        let fetch_prefixes: HashMap<String, String> = fetch
            .iter()
            .map(|p| (p.name.clone(), p.url_prefix.clone()))
            .collect();
        let dispatch: friday_capabilities::Dispatch = Arc::new(
            move |candidate: &friday_capabilities::CandidateBinding, params: &serde_json::Value| {
                let adapter = adapter.clone();
                let adapter_name = candidate.binding.adapter.clone();
                let verb = candidate.binding.action.clone();
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
                // Provider-specific parameter shaping, keyed on the binding's
                // verb: search injects the provider's endpoint; fetch composes
                // the provider's URL prefix with the target URL.
                match verb.as_str() {
                    "search" => {
                        if let Some(e) = search_endpoints.get(&candidate.provider) {
                            parameters["endpoint"] = json!(e);
                        }
                    }
                    "fetch" => {
                        if let Some(prefix) = fetch_prefixes.get(&candidate.provider) {
                            if let Some(url) = parameters.get("url").and_then(|v| v.as_str()) {
                                parameters["url"] = json!(compose_gateway_url(prefix, url));
                            }
                        }
                    }
                    other => {
                        let other = other.to_string();
                        return Box::pin(async move {
                            Err(format!("web resolver cannot dispatch action {other}"))
                        });
                    }
                }
                Box::pin(async move {
                    let mut req = ActionRequest::new(adapter_name, verb, parameters);
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
            },
        );

        Self {
            name: "evidence:web".to_string(),
            resolver,
            dispatch,
            capabilities,
        }
    }

    /// Live health update for the `web.search` candidates — an orchestrator /
    /// health Monitor can mark a provider offline without rebuilding the tool.
    pub fn set_health(&self, provider: &str, health: CapabilityHealth) {
        self.resolver.set_health("web.search", provider, health);
    }

    /// Live health update for any capability this tool serves (`web.search` /
    /// `web.fetch`).
    pub fn set_health_for(&self, capability: &str, provider: &str, health: CapabilityHealth) {
        self.resolver.set_health(capability, provider, health);
    }

    /// Providers currently registered for `web.search` (any health) — for
    /// discovery and audit.
    pub fn providers(&self) -> Vec<String> {
        self.resolver.providers("web.search")
    }

    /// The verbs this tool actually serves, e.g. `"web.search, web.fetch"` —
    /// used for honest refusal messages.
    fn served_list(&self) -> String {
        self.capabilities
            .iter()
            .filter_map(|c| match c {
                Capability::Custom(id) => Some(id.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join(", ")
    }
}

#[async_trait]
impl Tool for ResolvingWebTool {
    fn name(&self) -> &str {
        &self.name
    }

    fn description(&self) -> &str {
        "Resolver-backed web evidence (web.search / web.fetch) with health-gated \
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
        // Refuse anything the tool does not actually serve — both the channel
        // (`web`) and the verb it advertises in `capabilities`. A search-only
        // tool refuses web.fetch; the combined tool serves both.
        let capability = format!("web.{verb}");
        let serves = self
            .capabilities
            .iter()
            .any(|c| matches!(c, Capability::Custom(id) if id.as_str() == capability.as_str()));
        if tool != "web" || !serves {
            return ToolResult::error(format!(
                "{} only serves {}, not {tool}.{verb}",
                self.name,
                self.served_list()
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
        let mut outcome = self
            .resolver
            .execute(&capability, &parameters, &self.dispatch)
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

        let evidence = if verb == "search" {
            normalize_search_evidence(&outcome)
        } else {
            // The canonical page URL is the target the planner asked for. A
            // gateway winner echoes the composed gateway URL
            // (`https://r.jina.ai/https%3A%2F%2F…`) back from the adapter — that
            // must never leak into evidence: it is the gateway shape, and it
            // would break the evidence pipeline's dedupe/corroboration, which
            // keys on `url` (the same page via jina vs direct must collide).
            // For the direct provider this restore is a no-op.
            if let Some(target) = action.parameters.get("url").and_then(|v| v.as_str()) {
                if let Some(map) = outcome.output.as_object_mut() {
                    map.insert("url".to_string(), json!(target));
                }
            }
            normalize_fetch_evidence(&outcome)
        };
        let output =
            serde_json::to_string_pretty(&evidence).unwrap_or_else(|_| json!([]).to_string());
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

/// Normalize a successful `web.search` resolver outcome into [`WebEvidence`] —
/// the single provider-agnostic evidence shape. `content` is populated when
/// the provider supplies it; search hits carry title/url only.
pub fn normalize_search_evidence(
    outcome: &friday_capabilities::ResolvedOutcome,
) -> serde_json::Value {
    let query = outcome.output.get("query").cloned().unwrap_or(json!(null));
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
            title: r
                .get("title")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
            url: r
                .get("url")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
            content: r
                .get("content")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
            retrieved_at: Utc::now(),
            confidence: 1.0,
        })
        .collect();
    json!({ "query": query, "evidence": evidence })
}

/// Normalize a successful `web.fetch` resolver outcome into one [`WebEvidence`]
/// entry (the fetched page) plus fetch metadata — the provider-agnostic shape
/// for page evidence. Only `url`, `chars`, `truncated`, and `evidence[]`
/// escape; raw adapter keys (`text`, `title` source noise, …) never do.
pub fn normalize_fetch_evidence(
    outcome: &friday_capabilities::ResolvedOutcome,
) -> serde_json::Value {
    let url = outcome
        .output
        .get("url")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let title = outcome
        .output
        .get("title")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let content = outcome
        .output
        .get("text")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let chars = outcome.output.get("chars").cloned().unwrap_or(json!(0));
    let truncated = outcome
        .output
        .get("truncated")
        .cloned()
        .unwrap_or(json!(false));
    let evidence = vec![WebEvidence {
        source: outcome.provider.clone(),
        title,
        url: url.clone(),
        content,
        retrieved_at: Utc::now(),
        confidence: 1.0,
    }];
    json!({ "url": url, "chars": chars, "truncated": truncated, "evidence": evidence })
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

    /// Serve one canned HTML page whose title AND body echo the request path
    /// — the "gateway role": a real reader service receives the composed URL
    /// and reports back exactly what path it was asked for. Proves the
    /// percent-encoded target survives HTTP transport unmodified.
    async fn serve_echo() -> String {
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
                    // Request line: `GET /<path> HTTP/1.1`. The path is the
                    // composed, percent-encoded target.
                    let line = String::from_utf8_lossy(&buf)
                        .lines()
                        .next()
                        .unwrap_or("")
                        .to_string();
                    let path = line.split_whitespace().nth(1).unwrap_or("").to_string();
                    let body = format!(
                        "<html><head><title>{path}</title></head><body>gateway-path:{path}</body></html>"
                    );
                    let head = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        body.len()
                    );
                    let _ = sock.write_all(head.as_bytes()).await;
                    let _ = sock.write_all(body.as_bytes()).await;
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

    const FETCH_HTML: &str = r#"<html><head><title>Rust Async Book</title></head><body>
        <p>This page explains async in Rust with tokio.</p>
    </body></html>"#;

    fn evidence_action() -> Action {
        Action::single(
            "gather evidence: web (web.search)",
            Capability::Custom("web.search".into()),
            "g1",
        )
        .with_parameters(json!({ "query": "rust async" }))
    }

    fn fetch_action(url: &str) -> Action {
        Action::single(
            "gather evidence: web (web.fetch)",
            Capability::Custom("web.fetch".into()),
            "g1",
        )
        .with_parameters(json!({ "url": url }))
    }

    fn parsed_output(result: &ToolResult) -> Value {
        serde_json::from_str(&result.output).expect("tool output is JSON")
    }

    // ── Gateway URL composition (the percent-encoding contract) ────

    #[test]
    fn direct_provider_composes_the_target_verbatim() {
        let target = "https://example.com/a b?x=1&y=2#frag";
        assert_eq!(compose_gateway_url("", target), target);
        assert_eq!(
            compose_gateway_url("", "http://127.0.0.1:9/"),
            "http://127.0.0.1:9/"
        );
    }

    #[test]
    fn gateway_provider_percent_encodes_the_target() {
        assert_eq!(
            compose_gateway_url(JINA_READER_PREFIX, "https://example.com/page"),
            "https://r.jina.ai/https%3A%2F%2Fexample.com%2Fpage"
        );
        // Query, fragment, space, and unicode must all be encoded into one
        // opaque component — no raw `?`/`&`/`#`/space survives for the gateway
        // to misinterpret.
        let composed =
            compose_gateway_url(JINA_READER_PREFIX, "https://example.com/a b?x=1&y=2#frag/Ü");
        assert_eq!(
            composed,
            "https://r.jina.ai/https%3A%2F%2Fexample.com%2Fa%20b%3Fx%3D1%26y%3D2%23frag%2F%C3%9C"
        );
        assert!(
            !composed[..JINA_READER_PREFIX.len()].contains('?'),
            "{composed}"
        );
        assert!(!composed.contains(' '), "{composed}");
        assert!(!composed.contains('&'), "{composed}");
        assert!(!composed.contains('#'), "{composed}");
    }

    #[test]
    fn query_style_gateway_prefix_gets_an_encoded_target_value() {
        // `…/fetch?url=` gateways receive the target as a query VALUE: the
        // encoded target cannot contain `&` or `#`, so it can never break out
        // of the value.
        let composed =
            compose_gateway_url("https://reader.example/fetch?url=", "https://x.io/?a=b&c=d");
        assert_eq!(
            composed,
            "https://reader.example/fetch?url=https%3A%2F%2Fx.io%2F%3Fa%3Db%26c%3Dd"
        );
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
            assert_eq!(
                hit["source"], "duckduckgo",
                "provider survives as source only"
            );
            assert_eq!(hit["confidence"], 1.0);
        }
        assert_eq!(evidence[0]["title"], "The Rust Book");
        assert_eq!(evidence[0]["url"], "https://doc.rust-lang.org/book/");
        assert_eq!(evidence[1]["url"], "https://docs.rs/tokio");
    }

    #[test]
    fn normalize_fetch_evidence_emits_provider_agnostic_shape() {
        let outcome = friday_capabilities::ResolvedOutcome {
            success: true,
            capability: "web.fetch".into(),
            provider: "direct".into(),
            attempted: vec!["direct".into()],
            output: json!({
                "url": "https://example.com/page",
                "title": "Example Page",
                "text": "Some readable content.",
                "chars": 21,
                "truncated": false,
                // Provider-specific noise must never reach the cognitive layer.
                "response_id": "y-456",
                "provider_specific": { "crawl_depth": 3 },
            }),
            error: None,
        };

        let v = normalize_fetch_evidence(&outcome);
        let obj = v.as_object().expect("normalized object");
        // Only the fetch contract keys escape: url + chars + truncated +
        // evidence[] (the page itself). `text`/provider noise never do.
        let mut keys: Vec<&String> = obj.keys().collect();
        keys.sort_unstable();
        assert_eq!(
            keys,
            vec![
                &"chars".to_string(),
                &"evidence".to_string(),
                &"truncated".to_string(),
                &"url".to_string(),
            ],
            "provider shape must not leak: {obj:?}"
        );
        assert_eq!(obj["url"], "https://example.com/page");
        assert_eq!(obj["chars"], 21);
        assert_eq!(obj["truncated"], false);
        let evidence = obj["evidence"].as_array().expect("evidence array");
        assert_eq!(evidence.len(), 1);
        let hit = evidence[0].as_object().expect("evidence hit object");
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
        assert_eq!(hit["source"], "direct", "provider survives as source only");
        assert_eq!(hit["title"], "Example Page");
        assert_eq!(hit["url"], "https://example.com/page");
        assert_eq!(hit["content"], "Some readable content.");
        assert_eq!(hit["confidence"], 1.0);
    }

    // ── Acceptance 1: web.search → resolver → healthy provider → Evidence ──

    #[tokio::test]
    async fn web_search_resolves_through_healthy_provider_to_normalized_evidence() {
        let base = serve_html(RESULTS_HTML).await;
        let tool = ResolvingWebTool::for_web_search(
            Arc::new(WebAdapter::new()),
            vec![WebSearchProvider::new("duckduckgo", base)],
        );

        let result = tool
            .execute(&evidence_action(), &ExecutionContext::for_goal("g1"))
            .await;
        assert!(
            result.success,
            "end-to-end web.search failed: {:?}",
            result.error
        );

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

    // ── Acceptance 2: search primary fails → resolver → fallback → Evidence ──
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

        let result = tool
            .execute(&evidence_action(), &ExecutionContext::for_goal("g1"))
            .await;
        assert!(
            result.success,
            "fallback must rescue the failed primary: {:?}",
            result.error
        );

        let output = parsed_output(&result);
        let evidence = output["evidence"].as_array().expect("evidence array");
        assert_eq!(
            evidence.len(),
            2,
            "fallback served real results: {output:?}"
        );
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

    // ── Acceptance 3: web.fetch → resolver → healthy provider → Evidence ──

    #[tokio::test]
    async fn web_fetch_resolves_through_healthy_provider_to_normalized_evidence() {
        let base = serve_html(FETCH_HTML).await;
        let tool = ResolvingWebTool::for_web_fetch(
            Arc::new(WebAdapter::new()),
            vec![WebFetchProvider::new("direct", "")],
        );
        let url = format!("{base}article");

        let result = tool
            .execute(&fetch_action(&url), &ExecutionContext::for_goal("g1"))
            .await;
        assert!(
            result.success,
            "end-to-end web.fetch failed: {:?}",
            result.error
        );

        let output = parsed_output(&result);
        assert_eq!(output["url"], url);
        assert_eq!(output["truncated"], false);
        let evidence = output["evidence"].as_array().expect("evidence array");
        assert_eq!(evidence.len(), 1, "one fetched page: {output:?}");
        assert_eq!(evidence[0]["source"], "direct");
        assert_eq!(evidence[0]["title"], "Rust Async Book");
        assert_eq!(evidence[0]["url"], url);
        let content = evidence[0]["content"].as_str().expect("content");
        assert!(content.contains("async in Rust"), "content: {content}");
        assert!(evidence[0]["retrieved_at"].is_string());
        assert_eq!(evidence[0]["confidence"], 1.0);
        assert!(
            output.get("text").is_none(),
            "raw adapter shape must not leak: {output:?}"
        );
        assert_eq!(result.metadata["provider"], "direct");
        assert_eq!(result.metadata["attempted"], json!(["direct"]));
    }

    // ── Acceptance 4: fetch primary fails → resolver → fallback → Evidence ──

    #[tokio::test]
    async fn web_fetch_falls_back_when_primary_provider_fails() {
        let base = serve_html(FETCH_HTML).await;
        let url = format!("{base}article");
        let tool = ResolvingWebTool::for_web_fetch(
            Arc::new(WebAdapter::new()),
            vec![
                // A gateway provider whose prefix points at a dead host: the
                // composed URL is dead/fetch?url=<target> → connection refused,
                // so the resolver must fall back to the direct provider.
                WebFetchProvider::new("gateway", format!("{}/fetch?url=", dead_endpoint())),
                WebFetchProvider::new("direct", ""),
            ],
        );

        let result = tool
            .execute(&fetch_action(&url), &ExecutionContext::for_goal("g1"))
            .await;
        assert!(
            result.success,
            "fallback must rescue the failed primary: {:?}",
            result.error
        );

        let output = parsed_output(&result);
        let evidence = output["evidence"].as_array().expect("evidence array");
        assert_eq!(
            evidence[0]["source"], "direct",
            "the winning provider is the fallback"
        );
        assert_eq!(evidence[0]["title"], "Rust Async Book");
        assert_eq!(
            evidence[0]["url"], url,
            "the fallback fetched the original target, not the gateway prefix"
        );
        assert_eq!(result.metadata["provider"], "direct");
        assert_eq!(
            result.metadata["attempted"],
            json!(["gateway", "direct"]),
            "the full failure trail is recorded"
        );
    }

    // ── Acceptance 5: a real gateway receives the encoded target ──
    //
    // The `url_prefix` model proven on a real gateway transport: the dispatch
    // composes `prefix + percent-encode(target)`, the adapter GETs it, and the
    // gateway (here an echo server playing the reader-service role) reports
    // back the exact path it was asked for. If composition or encoding broke,
    // the echoed path would differ (raw `?`/`&`/`#`/space would split the URL).

    #[tokio::test]
    async fn jina_style_gateway_receives_the_percent_encoded_target_over_real_transport() {
        let gateway = serve_echo().await;
        let tool = ResolvingWebTool::for_web_fetch(
            Arc::new(WebAdapter::new()),
            vec![WebFetchProvider::gateway("jina", gateway)],
        );
        // A target full of gateway-breaking characters — the exact case the
        // raw concatenation used to get wrong.
        let target = "https://example.com/a b?x=1&y=2#frag";

        let result = tool
            .execute(&fetch_action(target), &ExecutionContext::for_goal("g1"))
            .await;
        assert!(result.success, "gateway fetch failed: {:?}", result.error);

        let output = parsed_output(&result);
        assert_eq!(result.metadata["provider"], "jina");
        // The evidence url is the canonical target — the composed gateway URL
        // must not leak into evidence (it would break pipeline dedupe).
        assert_eq!(output["url"], target);
        assert_eq!(output["evidence"][0]["url"], target);
        let evidence = output["evidence"].as_array().expect("evidence array");
        assert_eq!(evidence[0]["source"], "jina");
        let content = evidence[0]["content"].as_str().expect("content");
        // The gateway echoed the composed path: prefix + the ENCODED target.
        let encoded = friday_actions::urlencode(target);
        assert!(
            content.contains(&format!("gateway-path:/{encoded}")),
            "the gateway must receive the percent-encoded target exactly: {content}"
        );
        // No raw gateway-breaking character may appear in the echoed path.
        assert!(
            !content.contains("x=1&y=2"),
            "the raw query must not survive into the gateway path: {content}"
        );
        assert!(!content.contains(' '), "no raw space: {content}");
        assert!(!content.contains('#'), "no raw fragment: {content}");
    }

    #[tokio::test]
    async fn fetch_dispatch_composes_and_encodes_the_jina_gateway_url() {
        // Recording adapter: asserts the exact URL the adapter would GET, with
        // no real network — the dispatch-level proof of the composition.
        let last = Arc::new(std::sync::Mutex::new(None));
        let tool = ResolvingWebTool::for_web_fetch(
            Arc::new(RecordingFetchAdapter { last: last.clone() }),
            vec![WebFetchProvider::gateway("jina", JINA_READER_PREFIX)],
        );
        let target = "https://example.com/docs?lang=rust&q=async io";

        let result = tool
            .execute(&fetch_action(target), &ExecutionContext::for_goal("g1"))
            .await;
        assert!(
            result.success,
            "recording gateway fetch failed: {:?}",
            result.error
        );

        // The adapter is pointed at the composed, percent-encoded gateway URL…
        let req = last
            .lock()
            .unwrap()
            .take()
            .expect("adapter received a request");
        let received = req.parameters["url"].as_str().expect("composed url");
        assert_eq!(
            received,
            "https://r.jina.ai/https%3A%2F%2Fexample.com%2Fdocs%3Flang%3Drust%26q%3Dasync%20io",
            "the adapter must be pointed at the percent-encoded gateway URL"
        );
        // …but the normalized evidence url is the canonical target the planner
        // asked for — the gateway shape must never leak into evidence.
        let output = parsed_output(&result);
        assert_eq!(output["url"], target, "evidence url is the original target");
        assert_eq!(result.metadata["provider"], "jina");
        assert_eq!(output["evidence"][0]["source"], "jina");
        assert_eq!(
            output["evidence"][0]["url"], target,
            "the evidence hit url is the original target, not the gateway URL"
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
        let result = tool
            .execute(&evidence_action(), &ExecutionContext::for_goal("g1"))
            .await;
        assert!(
            result.success,
            "health-updated fallback failed: {:?}",
            result.error
        );
        let output = parsed_output(&result);
        assert_eq!(
            output["evidence"][0]["source"], "backup",
            "offline primary is bypassed after the live health update"
        );
        assert_eq!(result.metadata["provider"], "backup");
    }

    #[tokio::test]
    async fn fetch_live_health_update_reroutes_selection() {
        let base = serve_html(FETCH_HTML).await;
        let url = format!("{base}article");
        let tool = ResolvingWebTool::for_web_fetch(
            Arc::new(WebAdapter::new()),
            vec![
                WebFetchProvider::new("primary", format!("{}/fetch?url=", dead_endpoint())),
                WebFetchProvider::new("backup", ""),
            ],
        );

        // Same live seam as search, addressed per capability.
        tool.set_health_for("web.fetch", "primary", CapabilityHealth::Offline);
        let result = tool
            .execute(&fetch_action(&url), &ExecutionContext::for_goal("g1"))
            .await;
        assert!(
            result.success,
            "health-updated fetch fallback failed: {:?}",
            result.error
        );
        assert_eq!(
            parsed_output(&result)["evidence"][0]["source"],
            "backup",
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

        let result = tool
            .execute(&evidence_action(), &ExecutionContext::for_goal("g1"))
            .await;
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
    async fn fetch_all_providers_failing_is_a_structured_capability_error() {
        let tool = ResolvingWebTool::for_web_fetch(
            Arc::new(WebAdapter::new()),
            vec![
                WebFetchProvider::new("gateway", format!("{}/fetch?url=", dead_endpoint())),
                WebFetchProvider::new("gateway2", format!("{}/fetch?url=", dead_endpoint())),
            ],
        );

        let result = tool
            .execute(
                &fetch_action("https://example.com/x"),
                &ExecutionContext::for_goal("g1"),
            )
            .await;
        assert!(!result.success);
        let err = result.error.expect("structured error");
        assert!(err.contains("web.fetch unavailable"), "err: {err}");
        assert_eq!(
            result.metadata["attempted"],
            json!(["gateway", "gateway2"]),
            "the failed trail is still observable"
        );
    }

    #[tokio::test]
    async fn no_providers_is_a_clean_capability_error() {
        let tool = ResolvingWebTool::for_web_search(Arc::new(WebAdapter::new()), vec![]);
        let result = tool
            .execute(&evidence_action(), &ExecutionContext::for_goal("g1"))
            .await;
        assert!(!result.success);
        assert!(
            result
                .error
                .unwrap()
                .contains("no online, policy-allowed binding"),
            "no providers ⇒ clean error, not a panic"
        );
    }

    // ── Honest refusal: a tool serves exactly what it advertises ──

    #[tokio::test]
    async fn search_only_tool_refuses_fetch_and_other_channels_honestly() {
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

        // A search-only tool refuses web.fetch honestly (the combined tool
        // serves it — see below).
        let fetch = Action::single(
            "gather evidence: web (web.fetch)",
            Capability::Custom("web.fetch".into()),
            "g1",
        );
        let r = tool.execute(&fetch, &ctx).await;
        assert!(!r.success);
        assert!(r.error.unwrap().contains("only serves web.search"));

        // A non-evidence instruction is refused too.
        let r = tool
            .execute(
                &Action::single("run cargo test", Capability::ExecuteCommand, "g1"),
                &ctx,
            )
            .await;
        assert!(!r.success);
        assert!(r.error.unwrap().contains("not an evidence action"));
    }

    #[tokio::test]
    async fn combined_tool_serves_both_verbs_and_refuses_others() {
        let tool = ResolvingWebTool::for_web(
            Arc::new(WebAdapter::new()),
            vec![WebSearchProvider::new("duckduckgo", dead_endpoint())],
            vec![WebFetchProvider::new("direct", "")],
        );
        let caps = tool.capabilities();
        assert!(caps.contains(&Capability::Custom("web.search".into())));
        assert!(caps.contains(&Capability::Custom("web.fetch".into())));

        let ctx = ExecutionContext::for_goal("g1");
        // A verb the tool does not serve is refused with the served list.
        let unknown = Action::single(
            "gather evidence: web (web.unknown)",
            Capability::Custom("web.unknown".into()),
            "g1",
        );
        let r = tool.execute(&unknown, &ctx).await;
        assert!(!r.success);
        assert!(
            r.error
                .unwrap()
                .contains("only serves web.search, web.fetch"),
            "refusal names the served verbs"
        );

        // A different channel is refused, not misrouted.
        let git = Action::single(
            "gather evidence: git (git.status)",
            Capability::Custom("git.status".into()),
            "g1",
        );
        let r = tool.execute(&git, &ctx).await;
        assert!(!r.success);
        assert!(r
            .error
            .unwrap()
            .contains("only serves web.search, web.fetch"));
    }

    #[tokio::test]
    async fn combined_tool_dispatches_fetch_through_the_resolver() {
        let base = serve_html(FETCH_HTML).await;
        let url = format!("{base}article");
        let tool = ResolvingWebTool::for_web(
            Arc::new(WebAdapter::new()),
            vec![WebSearchProvider::new("duckduckgo", dead_endpoint())],
            vec![WebFetchProvider::new("direct", "")],
        );
        let ctx = ExecutionContext::for_goal("g1");

        // Search through the combined tool uses its own (dead) provider.
        let search = tool.execute(&evidence_action(), &ctx).await;
        assert!(!search.success, "dead search provider must fail honestly");

        // Fetch through the combined tool resolves via its own candidate list
        // — capabilities and dispatch stay consistent on the production shape.
        let fetch = tool.execute(&fetch_action(&url), &ctx).await;
        assert!(
            fetch.success,
            "combined-tool fetch failed: {:?}",
            fetch.error
        );
        assert_eq!(parsed_output(&fetch)["evidence"][0]["source"], "direct");
        assert_eq!(fetch.metadata["provider"], "direct");
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

    /// Like [`RecordingAdapter`] but for `fetch`: records the composed URL the
    /// adapter was pointed at and echoes it back like a real fetch would — so
    /// a test can assert both the *request* URL (composed + encoded) and the
    /// normalized evidence `url` (the original target).
    struct RecordingFetchAdapter {
        last: Arc<std::sync::Mutex<Option<ActionRequest>>>,
    }

    #[async_trait]
    impl ActionAdapter for RecordingFetchAdapter {
        fn tool(&self) -> &str {
            "web"
        }

        fn actions(&self) -> Vec<&str> {
            vec!["fetch"]
        }

        async fn execute(&self, req: ActionRequest) -> friday_actions::ActionResult {
            let url = req
                .parameters
                .get("url")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            *self.last.lock().unwrap() = Some(req);
            friday_actions::ActionResult::success(json!({
                "url": url,
                "title": "page",
                "text": "body",
                "chars": 4,
                "truncated": false,
            }))
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
        assert!(
            result.success,
            "recording adapter must succeed: {:?}",
            result.error
        );

        let req = last
            .lock()
            .unwrap()
            .take()
            .expect("adapter received a request");
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
        assert!(
            !params.contains_key("__timeout_ms"),
            "reserved key stripped: {params:?}"
        );
        assert!(
            !params.contains_key("__permissions"),
            "reserved key stripped: {params:?}"
        );
        assert_eq!(params["endpoint"], "http://127.0.0.1:9/");
        assert_eq!(params["query"], "rust async");
    }
}
