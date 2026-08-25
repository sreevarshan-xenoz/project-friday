use async_trait::async_trait;
use futures::stream::unfold;
use futures::Stream;
use futures::StreamExt;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::collections::VecDeque;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use crate::egress::EgressGate;
use crate::qualification::{
    routing_score, Budget, CandidateView, Preference, ProviderHealth, QualificationProfile,
    RequestRequirements, Requirement, QUALIFICATION_SCHEMA_VERSION,
};
use crate::secrets::SecretsVault;

/// The standard provider credential environment variables, used by
/// [`crate::secrets::SecretsVault::seed_from_env`] to register production
/// keys into the vault (so the egress gate can scan for them and nothing
/// auto-prints them). Mirrors the provider-qual environment matrix.
pub const PROVIDER_ENV_VARS: [(&str, &str); 10] = [
    ("OMNIROUTE_API_KEY", "omniroute"),
    ("OPENROUTER_API_KEY", "openrouter"),
    ("GROQ_API_KEY", "groq"),
    ("CEREBRAS_API_KEY", "cerebras"),
    ("GEMINI_API_KEY", "gemini"),
    ("OPENGATEWAY_API_KEY", "opengateway"),
    ("SAMBA_API_KEY", "sambanova"),
    ("GITHUB_TOKEN", "github-models"),
    ("SILICONFLOW_API_KEY", "siliconflow"),
    ("ZHIPU_API_KEY", "zhipu"),
];

/// Pluggable interface for calling large language models.
/// Isolating this ensures we can swap backends (e.g. Gemini to OpenAI)
/// without touching prompt compilers or response interpreters.
#[async_trait]
pub trait ModelClient: Send + Sync {
    async fn complete(&self, prompt: &str) -> Result<String, String>;
}

/// A mock model client used for deterministic unit/integration testing of the cognition layer.
pub struct MockModelClient {
    pub response: String,
}

#[async_trait]
impl ModelClient for MockModelClient {
    async fn complete(&self, _prompt: &str) -> Result<String, String> {
        Ok(self.response.clone())
    }
}

/// Governs how the Inference Fabric selects providers and models.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum RoutingMode {
    Fast,
    Quality,
    Free,
    Local,
    Hybrid,
}

/// Supported capabilities that a model can advertise.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ModelCapability {
    Json,
    Tools,
    Vision,
    Local,
    LongContext,
}

/// FRIDAY-level logical model descriptors — provider-agnostic capability tiers.
/// Vendors map these to concrete model IDs internally (`ModelInfo.tier`), so a
/// model rename or repricing on the provider side never reaches FRIDAY. Callers
/// name capabilities ("Reasoning.Free"), never vendor model IDs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum CapabilityTier {
    /// Provider smart routing (`auto`): the gateway picks and escalates on failure.
    Auto,
    /// Deep reasoning on the free tier.
    ReasoningFree,
    /// Fast conversational / coding on the free tier.
    ConversationFast,
    /// Long-context on the free tier.
    LongContextFree,
    /// Vision on the free tier.
    VisionFree,
}

impl CapabilityTier {
    /// Stable wire/config tag, e.g. `reasoning-free`.
    pub fn tag(&self) -> &'static str {
        match self {
            CapabilityTier::Auto => "auto",
            CapabilityTier::ReasoningFree => "reasoning-free",
            CapabilityTier::ConversationFast => "conversation-fast",
            CapabilityTier::LongContextFree => "long-context-free",
            CapabilityTier::VisionFree => "vision-free",
        }
    }

    /// Parse a tag back into a tier. Returns `None` for unknown tags so callers
    /// can fall back to explicit model IDs.
    pub fn from_tag(s: &str) -> Option<Self> {
        match s {
            "auto" => Some(CapabilityTier::Auto),
            "reasoning-free" => Some(CapabilityTier::ReasoningFree),
            "conversation-fast" => Some(CapabilityTier::ConversationFast),
            "long-context-free" => Some(CapabilityTier::LongContextFree),
            "vision-free" => Some(CapabilityTier::VisionFree),
            _ => None,
        }
    }
}

/// Represents metadata for a model hosted by a specific provider.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelInfo {
    pub id: String,       // e.g. "google/gemini-2.5-flash"
    pub family: String,   // e.g. "gemini-2.5-flash"
    pub provider: String, // e.g. "openrouter"
    pub latency_ms: u64,
    pub reliability: f64,
    pub capabilities: Vec<ModelCapability>,
    pub max_context: usize,
    /// Stable FRIDAY capability tier, when assigned (see `CapabilityTier`).
    /// Absent for models only reachable by vendor ID.
    #[serde(default)]
    pub tier: Option<CapabilityTier>,
}

/// Pluggable adapter for communicating with an external API host.
#[async_trait]
pub trait Provider: Send + Sync {
    fn name(&self) -> &str;
    async fn complete(
        &self,
        model_id: &str,
        prompt: &str,
        require_json: bool,
    ) -> Result<String, String>;

    /// Complete with usage metadata. The default delegates to `complete` and
    /// returns `None` for usage — providers that can extract token counts from
    /// the API response should override this.
    async fn complete_with_usage(
        &self,
        model_id: &str,
        prompt: &str,
        require_json: bool,
    ) -> Result<ProviderResponse, String> {
        let content = self.complete(model_id, prompt, require_json).await?;
        Ok(ProviderResponse {
            content,
            usage: None,
        })
    }

    async fn check_health(&self) -> bool;
}

/// Response from a provider, carrying content and optional usage metadata.
#[derive(Debug, Clone)]
pub struct ProviderResponse {
    pub content: String,
    pub usage: Option<ProviderUsage>,
}

/// Usage metadata extracted from an API response.
#[derive(Debug, Clone)]
pub struct ProviderUsage {
    pub prompt_tokens: Option<u64>,
    pub completion_tokens: Option<u64>,
    pub total_tokens: Option<u64>,
    pub cached_tokens: Option<u64>,
    pub reasoning_tokens: Option<u64>,
    pub finish_reason: Option<String>,
}

/// Circuit Breaker States
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CircuitState {
    Closed,
    Open,
    HalfOpen,
}

/// State machine for a provider's circuit breaker.
pub struct CircuitBreaker {
    pub state: CircuitState,
    pub failures: usize,
    pub last_failure: Option<Instant>,
    pub cooldown_seconds: u64,
}

impl CircuitBreaker {
    pub fn new(cooldown_seconds: u64) -> Self {
        Self {
            state: CircuitState::Closed,
            failures: 0,
            last_failure: None,
            cooldown_seconds,
        }
    }

    pub fn record_success(&mut self) {
        self.failures = 0;
        self.state = CircuitState::Closed;
    }

    pub fn record_failure(&mut self) {
        self.failures += 1;
        self.last_failure = Some(Instant::now());
        if self.failures >= 3 {
            self.state = CircuitState::Open;
            tracing::warn!("Circuit breaker tripped to OPEN state!");
        }
    }

    pub fn allow_request(&mut self) -> bool {
        match self.state {
            CircuitState::Closed => true,
            CircuitState::Open => {
                if let Some(last) = self.last_failure {
                    if last.elapsed().as_secs() >= self.cooldown_seconds {
                        self.state = CircuitState::HalfOpen;
                        tracing::info!("Circuit breaker cooling down. Transitioned to HALF-OPEN.");
                        true
                    } else {
                        false
                    }
                } else {
                    true
                }
            }
            CircuitState::HalfOpen => true,
        }
    }
}

/// An OpenAI-compatible API client that handles OpenRouter, Groq, Cerebras, Gemini, and local Ollama.
pub struct OpenAiProvider {
    pub name: String,
    pub base_url: String,
    pub api_key: String,
}

#[async_trait]
impl Provider for OpenAiProvider {
    fn name(&self) -> &str {
        &self.name
    }

    async fn complete(
        &self,
        model_id: &str,
        prompt: &str,
        _require_json: bool,
    ) -> Result<String, String> {
        let client = reqwest::Client::new();
        let url = format!("{}/chat/completions", self.base_url.trim_end_matches('/'));

        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert(
            reqwest::header::AUTHORIZATION,
            reqwest::header::HeaderValue::from_str(&format!("Bearer {}", self.api_key))
                .map_err(|e| e.to_string())?,
        );
        headers.insert(
            reqwest::header::CONTENT_TYPE,
            reqwest::header::HeaderValue::from_static("application/json"),
        );

        if self.name == "openrouter" {
            headers.insert(
                reqwest::header::HeaderName::from_static("http-referer"),
                reqwest::header::HeaderValue::from_static(
                    "https://github.com/project-friday/friday",
                ),
            );
            headers.insert(
                reqwest::header::HeaderName::from_static("x-title"),
                reqwest::header::HeaderValue::from_static("FRIDAY Cognitive Desktop Agent"),
            );
        }

        let body = serde_json::json!({
            "model": model_id,
            "messages": [
                {
                    "role": "user",
                    "content": prompt
                }
            ]
        });

        let res = client
            .post(&url)
            .headers(headers)
            .json(&body)
            .send()
            .await
            .map_err(|e| format!("HTTP request failed: {}", e))?;

        if !res.status().is_success() {
            let status = res.status();
            let err_body = res.text().await.unwrap_or_default();
            return Err(format!(
                "Provider {} returned error status {}: {}",
                self.name, status, err_body
            ));
        }

        let json: serde_json::Value = res
            .json()
            .await
            .map_err(|e| format!("Failed to parse response JSON: {}", e))?;

        let content = json["choices"][0]["message"]["content"]
            .as_str()
            .ok_or_else(|| format!("Unexpected response structure: {:?}", json))?;

        Ok(content.to_string())
    }

    async fn check_health(&self) -> bool {
        let client = reqwest::Client::new();
        let url = format!("{}/models", self.base_url.trim_end_matches('/'));
        let res = client
            .get(&url)
            .header(
                reqwest::header::AUTHORIZATION,
                format!("Bearer {}", self.api_key),
            )
            .send()
            .await;
        res.is_ok() && res.unwrap().status().is_success()
    }

    async fn complete_with_usage(
        &self,
        model_id: &str,
        prompt: &str,
        _require_json: bool,
    ) -> Result<ProviderResponse, String> {
        let client = reqwest::Client::new();
        let url = format!("{}/chat/completions", self.base_url.trim_end_matches('/'));

        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert(
            reqwest::header::AUTHORIZATION,
            reqwest::header::HeaderValue::from_str(&format!("Bearer {}", self.api_key))
                .map_err(|e| e.to_string())?,
        );
        headers.insert(
            reqwest::header::CONTENT_TYPE,
            reqwest::header::HeaderValue::from_static("application/json"),
        );

        if self.name == "openrouter" {
            headers.insert(
                reqwest::header::HeaderName::from_static("http-referer"),
                reqwest::header::HeaderValue::from_static(
                    "https://github.com/project-friday/friday",
                ),
            );
            headers.insert(
                reqwest::header::HeaderName::from_static("x-title"),
                reqwest::header::HeaderValue::from_static("FRIDAY Cognitive Desktop Agent"),
            );
        }

        let body = serde_json::json!({
            "model": model_id,
            "messages": [
                {
                    "role": "user",
                    "content": prompt
                }
            ]
        });

        let res = client
            .post(&url)
            .headers(headers)
            .json(&body)
            .send()
            .await
            .map_err(|e| format!("HTTP request failed: {}", e))?;

        if !res.status().is_success() {
            let status = res.status();
            let err_body = res.text().await.unwrap_or_default();
            return Err(format!(
                "Provider {} returned error status {}: {}",
                self.name, status, err_body
            ));
        }

        let json: serde_json::Value = res
            .json()
            .await
            .map_err(|e| format!("Failed to parse response JSON: {}", e))?;

        let content = json["choices"][0]["message"]["content"]
            .as_str()
            .ok_or_else(|| format!("Unexpected response structure: {:?}", json))?
            .to_string();

        let usage = extract_usage_from_json(&json);

        Ok(ProviderResponse { content, usage })
    }
}

/// OmniRoute provider — a self-hosted, OpenAI-compatible multi-provider gateway.
///
/// OmniRoute aggregates 236+ upstream providers (OpenAI, Anthropic, Gemini, Ollama,
/// Groq, …) behind a single `/v1` endpoint and performs its OWN internal routing,
/// fallback, and circuit breaking. FRIDAY treats it as ONE provider — exactly like
/// Ollama or OpenRouter — so the Inference Fabric keeps ownership of top-level
/// routing decisions. This is intentionally a thin adapter over the existing
/// `Provider` trait; no trait changes, no streaming, no registry changes (Phase 1).
pub struct OmniRouteProvider {
    pub name: String,
    pub base_url: String,
    pub api_key: String,
}

impl OmniRouteProvider {
    pub fn new(base_url: String, api_key: String) -> Self {
        Self {
            name: "omniroute".to_string(),
            base_url,
            api_key,
        }
    }
}

#[async_trait]
impl Provider for OmniRouteProvider {
    fn name(&self) -> &str {
        &self.name
    }

    async fn complete(
        &self,
        model_id: &str,
        prompt: &str,
        _require_json: bool,
    ) -> Result<String, String> {
        let client = reqwest::Client::new();
        let url = format!("{}/chat/completions", self.base_url.trim_end_matches('/'));

        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert(
            reqwest::header::AUTHORIZATION,
            reqwest::header::HeaderValue::from_str(&format!("Bearer {}", self.api_key))
                .map_err(|e| e.to_string())?,
        );
        headers.insert(
            reqwest::header::CONTENT_TYPE,
            reqwest::header::HeaderValue::from_static("application/json"),
        );

        let body = serde_json::json!({
            "model": model_id,
            "messages": [
                {
                    "role": "user",
                    "content": prompt
                }
            ]
        });

        let res = client
            .post(&url)
            .headers(headers)
            .json(&body)
            .send()
            .await
            .map_err(|e| format!("HTTP request failed: {}", e))?;

        if !res.status().is_success() {
            let status = res.status();
            let err_body = res.text().await.unwrap_or_default();
            return Err(format!(
                "Provider {} returned error status {}: {}",
                self.name, status, err_body
            ));
        }

        let json: serde_json::Value = res
            .json()
            .await
            .map_err(|e| format!("Failed to parse response JSON: {}", e))?;

        let content = json["choices"][0]["message"]["content"]
            .as_str()
            .ok_or_else(|| format!("Unexpected response structure: {:?}", json))?;

        Ok(content.to_string())
    }

    async fn check_health(&self) -> bool {
        let client = reqwest::Client::new();
        let url = format!("{}/models", self.base_url.trim_end_matches('/'));
        let res = client
            .get(&url)
            .header(
                reqwest::header::AUTHORIZATION,
                format!("Bearer {}", self.api_key),
            )
            .send()
            .await;
        res.is_ok() && res.unwrap().status().is_success()
    }

    async fn complete_with_usage(
        &self,
        model_id: &str,
        prompt: &str,
        _require_json: bool,
    ) -> Result<ProviderResponse, String> {
        let client = reqwest::Client::new();
        let url = format!("{}/chat/completions", self.base_url.trim_end_matches('/'));

        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert(
            reqwest::header::AUTHORIZATION,
            reqwest::header::HeaderValue::from_str(&format!("Bearer {}", self.api_key))
                .map_err(|e| e.to_string())?,
        );
        headers.insert(
            reqwest::header::CONTENT_TYPE,
            reqwest::header::HeaderValue::from_static("application/json"),
        );

        let body = serde_json::json!({
            "model": model_id,
            "messages": [
                {
                    "role": "user",
                    "content": prompt
                }
            ]
        });

        let res = client
            .post(&url)
            .headers(headers)
            .json(&body)
            .send()
            .await
            .map_err(|e| format!("HTTP request failed: {}", e))?;

        if !res.status().is_success() {
            let status = res.status();
            let err_body = res.text().await.unwrap_or_default();
            return Err(format!(
                "Provider {} returned error status {}: {}",
                self.name, status, err_body
            ));
        }

        let json: serde_json::Value = res
            .json()
            .await
            .map_err(|e| format!("Failed to parse response JSON: {}", e))?;

        let content = json["choices"][0]["message"]["content"]
            .as_str()
            .ok_or_else(|| format!("Unexpected response structure: {:?}", json))?
            .to_string();

        let usage = extract_usage_from_json(&json);

        Ok(ProviderResponse { content, usage })
    }
}

/// Extract usage metadata from an OpenAI-compatible JSON response.
/// Returns `None` if the response doesn't contain a `usage` object.
fn extract_usage_from_json(json: &serde_json::Value) -> Option<ProviderUsage> {
    let usage = json.get("usage")?;
    let prompt_tokens = usage.get("prompt_tokens").and_then(|v| v.as_u64());
    let completion_tokens = usage.get("completion_tokens").and_then(|v| v.as_u64());
    let total_tokens = usage.get("total_tokens").and_then(|v| v.as_u64());
    // Some providers include cached_tokens inside prompt_tokens_details.
    let cached_tokens = usage
        .get("prompt_tokens_details")
        .and_then(|d| d.get("cached_tokens"))
        .and_then(|v| v.as_u64());
    // Some providers include reasoning_tokens inside completion_tokens_details.
    let reasoning_tokens = usage
        .get("completion_tokens_details")
        .and_then(|d| d.get("reasoning_tokens"))
        .and_then(|v| v.as_u64());
    let finish_reason = json["choices"][0]["finish_reason"]
        .as_str()
        .map(|s| s.to_string());

    Some(ProviderUsage {
        prompt_tokens,
        completion_tokens,
        total_tokens,
        cached_tokens,
        reasoning_tokens,
        finish_reason,
    })
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InferenceMetrics {
    pub request_id: String,
    pub timestamp: chrono::DateTime<chrono::Utc>,
    pub provider: String,
    pub model: String,
    pub latency_ms: u64,
    pub success: bool,
    pub retry_count: usize,
    pub error: Option<String>,
}

/// A provider health transition recorded for the observation week (Phase 1
/// finding 1.3). Mirrors the `[Provider Health]` trace line as JSON Lines so
/// demotion/degradation/recovery can be analyzed after the fact.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthChangeEvent {
    pub timestamp: chrono::DateTime<chrono::Utc>,
    pub provider: String,
    pub health_before: f64,
    pub health_after: f64,
    /// Coarse failure bucket: `timeout`, `rate_limit`, `auth`, `unavailable`,
    /// `server_error`, `error`, `success`, or `cooldown_elapsed`.
    pub reason: String,
    /// Action taken: `recovered`, `degraded`, `demoted`, `probing`, `none`.
    pub decision: String,
}

/// Representation of health state persisted locally.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersistedProviderHealth {
    pub name: String,
    pub reliability: f64,
    pub consecutive_failures: usize,
    pub circuit_state: String, // "Closed", "Open", "HalfOpen"
}

/// One candidate a routing decision considered, with the two scores routing
/// sorts by (provider reliability descending, then model latency ascending).
/// Serialized into qualification evidence so routing is auditable, not just
/// reportable by outcome.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CandidateConsideration {
    pub provider: String,
    pub model: String,
    /// Provider reliability (0.0..=1.0) — the primary sort key.
    pub reliability: f64,
    /// Model latency ms — the secondary sort key.
    pub latency_ms: u64,
}

/// One candidate failure during routing, in attempt order — the structured
/// "why" behind `served != chosen` (or behind nothing serving).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CandidateFailure {
    pub provider: String,
    pub model: String,
    pub error: String,
}

/// **FROZEN contract v1.** IMMUTABLE audit record of one routed completion.
///
/// Recorded once when routing finishes, then **never edited** — the same rule
/// as `DecisionRecord` ("history is immutable; intelligence is derived").
/// Every routing metric, qualification report, and future provider-reputation
/// analytic DERIVES from this record; none of them write back to it. The
/// human-readable failover summary in reports is derived from `failures`, never
/// stored here. Changing this shape requires a new contract version (bump
/// `version`) via ADR, not an ordinary implementation commit.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoutingDecision {
    /// Contract version. Bumped only when the frozen shape changes (v1 = 1).
    pub version: u32,
    /// RFC 3339 instant the decision was recorded (routing start).
    pub timestamp: String,
    /// The FRIDAY capability tier requested (`None` = mode-based routing, e.g.
    /// `RoutingMode::Free` over the whole catalog).
    pub requested_tier: Option<CapabilityTier>,
    /// The routing mode in effect (`Fast`/`Quality`/`Free`/`Local`/`Hybrid`).
    pub routing_mode: String,
    /// Candidates considered, in the order routing attempted them, with the
    /// two scores routing sorts by.
    pub candidates: Vec<CandidateConsideration>,
    /// The candidate selected to serve first (highest-ranked).
    pub chosen: Option<(String, String)>,
    /// The (provider, vendor model) that actually answered, if any.
    pub served: Option<(String, String)>,
    /// Per-candidate failures, in attempt order. Empty when `served == chosen`.
    pub failures: Vec<CandidateFailure>,
}

/// Coordinates model selection, state tracking, and error recovery.
pub struct InferenceFabric {
    pub providers: HashMap<String, Box<dyn Provider>>,
    pub models: Vec<ModelInfo>,
    pub health_scores: Arc<Mutex<HashMap<String, f64>>>,
    pub circuits: Arc<Mutex<HashMap<String, CircuitBreaker>>>,
    pub default_mode: RoutingMode,
    pub required_capabilities: Vec<ModelCapability>,
    /// The routing decision that served the last completion. Lets qualification
    /// evidence record which vendor model actually answered AND why (failover
    /// through the catalog, e.g. auto-route quota → free-model fallback).
    pub last_decision: Arc<Mutex<Option<RoutingDecision>>>,
    /// Immutable qualification profiles (offline benchmark artifacts from
    /// `provider-qual`), keyed `"provider/model"`. Loaded at startup via
    /// `QUALIFICATION_DIR`; seed routing signals but do not change selection.
    pub qualification_profiles: Arc<Mutex<HashMap<String, QualificationProfile>>>,
    /// Per-attempt deadline for a single provider call. `None` = wait forever
    /// (the pre-chaos behavior); `Some(d)` bounds one candidate's call so a
    /// hung or pathologically slow provider fails over instead of stalling the
    /// request. Surfaced as a defect by the chaos laboratory (Phase 1: hangs,
    /// extreme latency) and confirmed to be a robustness knob, not a new layer.
    pub request_timeout: Option<std::time::Duration>,
    /// Where provider health (scores + circuit state) persists. Defaults to
    /// `.fabric/provider_health.json` (cwd-relative). Chaos laboratory Phase 2
    /// scenarios point this at a controlled file so corruption is injectable.
    pub health_state_path: std::path::PathBuf,
    /// The outbound-data gate (#12 egress policy): `None` = egress scanning
    /// is OFF (current behavior); `Some(gate)` = every routed prompt is
    /// scanned for registered secrets before it leaves to ANY provider, with
    /// a per-provider Block/Redact/LogOnly policy (`services/intelligence/src/
    /// egress.rs`). Attaching is one line — autonomy's "can this secret leave
    /// the machine?" prerequisite.
    pub egress: Option<Arc<EgressGate>>,
}

impl InferenceFabric {
    pub fn new() -> Self {
        Self {
            providers: HashMap::new(),
            models: Vec::new(),
            health_scores: Arc::new(Mutex::new(HashMap::new())),
            circuits: Arc::new(Mutex::new(HashMap::new())),
            default_mode: RoutingMode::Free,
            required_capabilities: Vec::new(),
            last_decision: Arc::new(Mutex::new(None)),
            qualification_profiles: Arc::new(Mutex::new(HashMap::new())),
            request_timeout: None,
            health_state_path: std::path::PathBuf::from(".fabric/provider_health.json"),
            egress: None,
        }
    }

    /// Attach the egress gate: every routed outbound prompt is then scanned
    /// (registered secrets + heuristic) and blocked/redacted per provider.
    pub fn with_egress(mut self, gate: Arc<EgressGate>) -> Self {
        self.egress = Some(gate);
        self
    }

    /// The outbound seam shared by every dispatch site: apply the gate to a
    /// prompt about to leave to `provider`. `Err` = the payload never leaves
    /// (Block); `Ok(redacted)` = sanitized payload to send. No gate attached
    /// → the prompt is returned unchanged (behavior unchanged).
    fn egress_process(&self, provider: &str, prompt: &str) -> Result<String, String> {
        match &self.egress {
            None => Ok(prompt.to_string()),
            Some(gate) => gate.process(provider, prompt),
        }
    }

    pub fn register_provider(&mut self, provider: Box<dyn Provider>) {
        self.health_scores
            .lock()
            .unwrap()
            .insert(provider.name().to_string(), 1.0);
        self.circuits
            .lock()
            .unwrap()
            .insert(provider.name().to_string(), CircuitBreaker::new(30)); // 30s cooldown
        self.providers.insert(provider.name().to_string(), provider);
    }

    pub fn register_model(&mut self, model: ModelInfo) {
        self.models.push(model);
    }

    /// Resolve a FRIDAY capability tier to the concrete registered model for a
    /// provider. Callers name capabilities (`CapabilityTier`), never vendor IDs;
    /// when the provider renames a model, only the catalog changes.
    pub fn resolve_tier(&self, provider: &str, tier: CapabilityTier) -> Option<&ModelInfo> {
        self.models
            .iter()
            .find(|m| m.provider == provider && m.tier == Some(tier))
    }

    /// The last routing decision (full audit trail: candidates, chosen, served,
    /// and why they differed). `None` before the first routed completion.
    pub fn last_decision(&self) -> Option<RoutingDecision> {
        self.last_decision.lock().unwrap().clone()
    }

    /// The (provider, model id) that served the last successful completion.
    pub fn last_served(&self) -> Option<(String, String)> {
        self.last_decision
            .lock()
            .unwrap()
            .as_ref()
            .and_then(|d| d.served.clone())
    }

    /// Stamp a routing decision with the frozen contract (version + timestamp
    /// at routing start) and record it. The record is immutable once recorded:
    /// no caller ever edits a returned `RoutingDecision`.
    pub fn record_decision(&self, decision: RoutingDecision) {
        *self.last_decision.lock().unwrap() = Some(decision);
    }

    /// Register an immutable qualification profile for a provider/model pair.
    /// Profiles are immutable once registered; re-qualification replaces the
    /// profile wholesale (the schema version gates compatibility).
    pub fn register_qualification_profile(&mut self, profile: QualificationProfile) {
        let key = format!("{}/{}", profile.provider_id, profile.model_id);
        self.qualification_profiles
            .lock()
            .unwrap()
            .insert(key, profile);
    }

    /// Look up a registered qualification profile (clone; profiles are immutable).
    pub fn qualification_profile(
        &self,
        provider_id: &str,
        model_id: &str,
    ) -> Option<QualificationProfile> {
        self.qualification_profiles
            .lock()
            .unwrap()
            .get(&format!("{provider_id}/{model_id}"))
            .cloned()
    }

    /// Load every `qualification.json` under `dir` (recursively) and register
    /// profiles whose schema version this build understands. Incompatible
    /// versions are skipped with a warning, never guessed at.
    pub fn load_qualification_profiles(&self, dir: &std::path::Path) -> std::io::Result<usize> {
        let mut files = Vec::new();
        collect_qual_files(dir, &mut files);
        let mut loaded = 0;
        for f in files {
            let content = std::fs::read_to_string(&f)?;
            match serde_json::from_str::<QualificationProfile>(&content) {
                Ok(p) if p.is_compatible() => {
                    let key = format!("{}/{}", p.provider_id, p.model_id);
                    self.qualification_profiles.lock().unwrap().insert(key, p);
                    loaded += 1;
                }
                Ok(p) => tracing::warn!(
                    "skipping {}: schema version {} != {}",
                    f.display(),
                    p.schema_version,
                    QUALIFICATION_SCHEMA_VERSION
                ),
                Err(e) => tracing::warn!(
                    "skipping {}: not a valid qualification profile: {e}",
                    f.display()
                ),
            }
        }
        Ok(loaded)
    }

    /// Build a v1 routing decision stamped with the recording instant.
    fn start_decision(
        &self,
        requested_tier: Option<CapabilityTier>,
        candidates: Vec<CandidateConsideration>,
        chosen: Option<(String, String)>,
    ) -> RoutingDecision {
        RoutingDecision {
            version: 1,
            timestamp: chrono::Utc::now().to_rfc3339(),
            requested_tier,
            routing_mode: format!("{:?}", self.default_mode),
            candidates,
            chosen,
            served: None,
            failures: Vec::new(),
        }
    }

    /// Complete against a capability tier, resolving the vendor model ID
    /// internally. Returns an error if the provider or tier is unregistered.
    pub async fn complete_tier(
        &self,
        provider: &str,
        tier: CapabilityTier,
        prompt: &str,
        require_json: bool,
    ) -> Result<String, String> {
        let model = self
            .resolve_tier(provider, tier)
            .ok_or_else(|| format!("no model registered for {provider} tier {}", tier.tag()))?;
        let prov = self
            .providers
            .get(provider)
            .ok_or_else(|| format!("provider {provider} not registered"))?;

        let reliability = self
            .health_scores
            .lock()
            .unwrap()
            .get(provider)
            .copied()
            .unwrap_or(1.0);
        let mut decision = self.start_decision(
            Some(tier),
            vec![CandidateConsideration {
                provider: provider.to_string(),
                model: model.id.clone(),
                reliability,
                latency_ms: model.latency_ms,
            }],
            Some((provider.to_string(), model.id.clone())),
        );

        // Egress gate (#12): scan the prompt before it leaves to this
        // provider. `Err` = blocked — the payload never leaves. The block is
        // recorded on the routing decision FIRST so it stays auditable
        // ("which provider was receiving it?"), then propagated.
        let prompt = match self.egress_process(provider, prompt) {
            Ok(prompt) => prompt,
            Err(e) => {
                decision.failures.push(CandidateFailure {
                    provider: provider.to_string(),
                    model: model.id.clone(),
                    error: e.clone(),
                });
                self.record_decision(decision);
                return Err(e);
            }
        };
        match run_attempt(
            self.request_timeout,
            prov.complete(&model.id, &prompt, require_json),
        )
        .await
        {
            Ok(response) => {
                decision.served = Some((provider.to_string(), model.id.clone()));
                self.record_decision(decision);
                Ok(response)
            }
            Err(e) => {
                decision.failures.push(CandidateFailure {
                    provider: provider.to_string(),
                    model: model.id.clone(),
                    error: e.clone(),
                });
                self.record_decision(decision);
                Err(e)
            }
        }
    }

    /// Selects and ranks provider-model candidates based on routing policy and capabilities.
    pub fn select_candidates(&self, mode: RoutingMode) -> Vec<(ModelInfo, &dyn Provider)> {
        let mut candidates = Vec::new();
        let mut circuit_guard = self.circuits.lock().unwrap();
        let mut probe_entered = None;

        for model in &self.models {
            // Check capability match
            let has_caps = self
                .required_capabilities
                .iter()
                .all(|cap| model.capabilities.contains(cap));
            if !has_caps {
                continue;
            }

            // Check circuit breaker status
            if let Some(cb) = circuit_guard.get_mut(&model.provider) {
                let state_before = cb.state;
                if !cb.allow_request() {
                    continue;
                }
                if state_before != cb.state && cb.state == CircuitState::HalfOpen {
                    probe_entered = Some(model.provider.clone());
                }
            }

            let matches = match mode {
                RoutingMode::Fast => {
                    model.provider == "groq"
                        || model.provider == "cerebras"
                        || model.provider == "nvidia-nim"
                }
                RoutingMode::Quality => {
                    model.provider == "gemini"
                        || model.provider == "groq"
                        || model.provider == "cerebras"
                        || model.provider == "nvidia-nim"
                }
                RoutingMode::Free => is_free_provider(&model.provider),
                RoutingMode::Local => model.capabilities.contains(&ModelCapability::Local),
                RoutingMode::Hybrid => {
                    model.capabilities.contains(&ModelCapability::Local)
                        || model.provider == "openrouter"
                        || model.provider == "omniroute"
                        || model.provider == "nvidia-nim"
                        || model.provider == "opengateway"
                        || model.provider == "sambanova"
                        || model.provider == "github-models"
                }
            };

            if matches {
                if let Some(prov) = self.providers.get(&model.provider) {
                    candidates.push((model.clone(), prov.as_ref()));
                }
            }
        }

        drop(circuit_guard);

        if let Some(provider) = probe_entered {
            let health = self
                .health_scores
                .lock()
                .unwrap()
                .get(&provider)
                .copied()
                .unwrap_or(1.0);
            self.log_health_change(&provider, health, health, "cooldown_elapsed", "probing");
        }

        let scores = self.health_scores.clone();
        candidates.sort_by(|a, b| {
            let guard = scores.lock().unwrap();
            let rel_a = guard.get(a.1.name()).copied().unwrap_or(1.0);
            let rel_b = guard.get(b.1.name()).copied().unwrap_or(1.0);

            // Primary sort: reliability descending
            let rel_cmp = rel_b
                .partial_cmp(&rel_a)
                .unwrap_or(std::cmp::Ordering::Equal);
            if rel_cmp != std::cmp::Ordering::Equal {
                return rel_cmp;
            }

            // Secondary sort: latency ascending
            a.0.latency_ms.cmp(&b.0.latency_ms)
        });

        candidates
    }

    /// Whether a model is eligible under the free budget: a free-tier provider
    /// or a local model (running local costs nothing).
    fn free_eligible(model: &ModelInfo) -> bool {
        is_free_provider(&model.provider) || model.capabilities.contains(&ModelCapability::Local)
    }

    /// Whether `model` can serve `req` at all. Hard filters only: budget
    /// (free-tier allowlist + local), privacy (required ⇒ local capability),
    /// required capabilities, and the latency bound. Scoring happens after.
    fn requirements_met(req: &RequestRequirements, model: &ModelInfo) -> bool {
        if req.budget == Budget::Free && !Self::free_eligible(model) {
            return false;
        }
        if req.privacy == Preference::Required
            && !model.capabilities.contains(&ModelCapability::Local)
        {
            return false;
        }
        if req.vision == Requirement::Required
            && !model.capabilities.contains(&ModelCapability::Vision)
        {
            return false;
        }
        if req.long_context == Requirement::Required
            && !model.capabilities.contains(&ModelCapability::LongContext)
        {
            return false;
        }
        if req.tools == Requirement::Required
            && !model.capabilities.contains(&ModelCapability::Tools)
        {
            return false;
        }
        if let Some(bound) = req.latency_ms {
            if model.latency_ms > u64::from(bound) {
                return false;
            }
        }
        true
    }

    /// Select and rank candidates for a requirement-driven request (the Model
    /// Budget path). Hard requirements filter first; the survivors are ranked
    /// by the requirement-aware [`routing_score`]. Unlike mode-based
    /// [`InferenceFabric::select_candidates`], local-capability models are
    /// always eligible (the free-first cascade starts local).
    pub fn select_for(&self, req: &RequestRequirements) -> Vec<(ModelInfo, &dyn Provider)> {
        let mut candidates = Vec::new();
        let mut circuit_guard = self.circuits.lock().unwrap();
        let mut probe_entered = None;

        for model in &self.models {
            if !Self::requirements_met(req, model) {
                continue;
            }
            // The fabric-global capability gate applies to every selection
            // path — select_candidates and select_for must agree on what is
            // routable, or a caller setting `required_capabilities` would get
            // different candidate sets depending on the entry point.
            let has_global_caps = self
                .required_capabilities
                .iter()
                .all(|cap| model.capabilities.contains(cap));
            if !has_global_caps {
                continue;
            }
            if let Some(cb) = circuit_guard.get_mut(&model.provider) {
                let state_before = cb.state;
                if !cb.allow_request() {
                    continue;
                }
                if state_before != cb.state && cb.state == CircuitState::HalfOpen {
                    probe_entered = Some(model.provider.clone());
                }
            }
            if let Some(prov) = self.providers.get(&model.provider) {
                candidates.push((model.clone(), prov.as_ref()));
            }
        }
        drop(circuit_guard);

        if let Some(provider) = probe_entered {
            let health = self
                .health_scores
                .lock()
                .unwrap()
                .get(&provider)
                .copied()
                .unwrap_or(1.0);
            self.log_health_change(&provider, health, health, "cooldown_elapsed", "probing");
        }

        candidates.sort_by(|a, b| {
            let sa = self.score_for(req, &a.0, a.1.name());
            let sb = self.score_for(req, &b.0, b.1.name());
            sb.partial_cmp(&sa).unwrap_or(std::cmp::Ordering::Equal)
        });

        candidates
    }

    /// Requirement-aware routing score for one candidate: offline benchmark
    /// evidence + runtime health + requirement adjustments. Missing profiles
    /// fall back to a neutral midpoint (see [`routing_score`]).
    fn score_for(&self, req: &RequestRequirements, model: &ModelInfo, provider: &str) -> f64 {
        let reliability = self
            .health_scores
            .lock()
            .unwrap()
            .get(provider)
            .copied()
            .unwrap_or(1.0);
        let circuit = self
            .circuits
            .lock()
            .unwrap()
            .get(provider)
            .map(|c| c.state)
            .unwrap_or(CircuitState::Closed);
        let health = ProviderHealth {
            success_rate: reliability,
            availability: if circuit == CircuitState::Closed {
                1.0
            } else {
                0.0
            },
            circuit_state: circuit,
            ..ProviderHealth::default()
        };
        let profile = self.qualification_profile(provider, &model.id);
        let view = CandidateView {
            provider: provider.to_string(),
            local: model.capabilities.contains(&ModelCapability::Local),
            vision: model.capabilities.contains(&ModelCapability::Vision),
            long_context: model.capabilities.contains(&ModelCapability::LongContext),
            tools: model.capabilities.contains(&ModelCapability::Tools),
            latency_ms: model.latency_ms,
        };
        routing_score(profile.as_ref(), &health, req, &view)
    }

    /// Emits a structured health-change event so provider health transitions
    /// are observable (chaos laboratory Phase 1 finding 1.3: transient failures
    /// permanently demote a provider). We log BEFORE changing the scoring
    /// algorithm, so there is telemetry to justify any hysteresis later.
    fn log_health_change(
        &self,
        provider: &str,
        before: f64,
        after: f64,
        reason: &str,
        decision: &str,
    ) {
        tracing::info!(
            "[Provider Health] Provider: {} / Health: {:.2} → {:.2} / Reason: {} / Decision: {}",
            provider,
            before,
            after,
            reason,
            decision
        );

        let event = HealthChangeEvent {
            timestamp: chrono::Utc::now(),
            provider: provider.to_string(),
            health_before: before,
            health_after: after,
            reason: reason.to_string(),
            decision: decision.to_string(),
        };
        let _ = std::fs::create_dir_all(".fabric");
        if let Ok(mut file) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(".fabric/health-changes.jsonl")
        {
            use std::io::Write;
            if let Ok(line) = serde_json::to_string(&event) {
                let _ = writeln!(file, "{}", line);
            }
        }
    }

    /// Persists health scores and circuit states to a local file.
    ///
    /// Written atomically (temp file + rename) so a crash mid-write cannot
    /// destroy the previous health snapshot (chaos laboratory Phase 2:
    /// partially written file / crash during persistence).
    pub fn persist_health(&self) {
        let scores = self.health_scores.lock().unwrap();
        let circuits = self.circuits.lock().unwrap();

        let mut persisted = Vec::new();
        for (name, reliability) in scores.iter() {
            let (consec_failures, state_str) = if let Some(cb) = circuits.get(name) {
                let state_str = match cb.state {
                    CircuitState::Closed => "Closed",
                    CircuitState::Open => "Open",
                    CircuitState::HalfOpen => "HalfOpen",
                };
                (cb.failures, state_str.to_string())
            } else {
                (0, "Closed".to_string())
            };
            persisted.push(PersistedProviderHealth {
                name: name.clone(),
                reliability: *reliability,
                consecutive_failures: consec_failures,
                circuit_state: state_str,
            });
        }

        let path = self.health_state_path.clone();
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let tmp = temp_sibling(&path);
        let written = std::fs::File::create(&tmp)
            .and_then(|file| serde_json::to_writer_pretty(file, &persisted).map_err(Into::into))
            .and_then(|()| std::fs::rename(&tmp, &path));
        if written.is_err() {
            let _ = std::fs::remove_file(&tmp);
        }
    }

    /// Loads health scores and circuit states from the health-state file, if it
    /// exists. Tolerates every corruption Phase 2 injects: a missing, empty,
    /// truncated, corrupt, older-schema, or partially-written file falls back to
    /// the default health (1.0 / circuit Closed) with a warning; impossible
    /// values (NaN or out-of-range reliability) are sanitized, never loaded
    /// verbatim, so they cannot poison scoring.
    pub fn load_health(&self) {
        let path = self.health_state_path.clone();
        let file = match std::fs::File::open(&path) {
            Ok(f) => f,
            Err(e) => {
                if e.kind() != std::io::ErrorKind::NotFound {
                    tracing::warn!(
                        "[Provider Health] could not open {}: {e}; falling back to default health",
                        path.display()
                    );
                }
                return;
            }
        };

        let persisted = match serde_json::from_reader::<_, Vec<PersistedProviderHealth>>(file) {
            Ok(p) => p,
            Err(e) => {
                tracing::warn!(
                    "[Provider Health] {} is unreadable ({e}); falling back to default health",
                    path.display()
                );
                return;
            }
        };

        let mut scores = self.health_scores.lock().unwrap();
        let mut circuits = self.circuits.lock().unwrap();
        for p in persisted {
            let reliability = sanitize_reliability(p.reliability);
            if !p.reliability.is_finite() || !(0.0..=1.0).contains(&p.reliability) {
                tracing::warn!(
                    "[Provider Health] provider {} persisted impossible reliability {:?}; \
                     sanitized to {reliability}",
                    p.name,
                    p.reliability
                );
            }
            scores.insert(p.name.clone(), reliability);
            if let Some(cb) = circuits.get_mut(&p.name) {
                cb.failures = p.consecutive_failures;
                cb.state = match p.circuit_state.as_str() {
                    "Open" => CircuitState::Open,
                    "HalfOpen" => CircuitState::HalfOpen,
                    _ => CircuitState::Closed,
                };
                if cb.state == CircuitState::Open {
                    cb.last_failure = Some(Instant::now());
                }
            }
        }
    }

    /// Logs request telemetry to a local JSON Lines file.
    pub fn log_metrics(&self, metrics: &InferenceMetrics) {
        let _ = std::fs::create_dir_all(".fabric");
        if let Ok(mut file) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(".fabric/metrics.jsonl")
        {
            use std::io::Write;
            if let Ok(line) = serde_json::to_string(metrics) {
                let _ = writeln!(file, "{}", line);
            }
        }
    }

    /// Create default fabric configuration dynamically based on registry config file or env vars.
    pub fn default_from_env() -> Self {
        let mut fabric = Self::new();
        let security_vault = Arc::new(SecretsVault::new());

        // Attempt to load dynamic model registry from workspace config file
        let mut registry_loaded = false;
        if let Ok(content) = std::fs::read_to_string("model_registry.json") {
            if let Ok(models) = serde_json::from_str::<Vec<ModelInfo>>(&content) {
                for m in models {
                    fabric.register_model(m);
                }
                registry_loaded = true;
                tracing::info!("Loaded model registry dynamically from model_registry.json.");
            }
        }

        // OpenRouter (Tier 1 Free Aggregator)
        // Free models: 20 req/min, 50/day (1,000/day with $10+ credit history)
        if let Ok(key) = std::env::var("OPENROUTER_API_KEY") {
            security_vault.store("provider", "openrouter", key.clone());
            fabric.register_provider(Box::new(OpenAiProvider {
                name: "openrouter".to_string(),
                base_url: "https://openrouter.ai/api/v1".to_string(),
                api_key: key,
            }));
            if !registry_loaded {
                // Auto-router: picks best free model per request (most reliable)
                fabric.register_model(ModelInfo {
                    id: "openrouter/free".to_string(),
                    family: "auto".to_string(),
                    provider: "openrouter".to_string(),
                    latency_ms: 2000,
                    reliability: 1.0,
                    capabilities: vec![
                        ModelCapability::Json,
                        ModelCapability::Tools,
                        ModelCapability::LongContext,
                    ],
                    max_context: 1_000_000,
                    tier: None,
                });
                // Coding-focused free model: 1M context
                fabric.register_model(ModelInfo {
                    id: "qwen/qwen3-coder:free".to_string(),
                    family: "qwen3-coder".to_string(),
                    provider: "openrouter".to_string(),
                    latency_ms: 3000,
                    reliability: 1.0,
                    capabilities: vec![
                        ModelCapability::Json,
                        ModelCapability::Tools,
                        ModelCapability::LongContext,
                    ],
                    max_context: 1_000_000,
                    tier: None,
                });
                // Most-used free model but may return empty under load: 1M context
                fabric.register_model(ModelInfo {
                    id: "nvidia/nemotron-3-ultra-550b-a55b:free".to_string(),
                    family: "nemotron-3-ultra".to_string(),
                    provider: "openrouter".to_string(),
                    latency_ms: 4000,
                    reliability: 0.8,
                    capabilities: vec![
                        ModelCapability::Json,
                        ModelCapability::Tools,
                        ModelCapability::LongContext,
                    ],
                    max_context: 1_000_000,
                    tier: None,
                });
                // Agentic coding free model: 262K context
                fabric.register_model(ModelInfo {
                    id: "poolside/laguna-m.1:free".to_string(),
                    family: "laguna-m".to_string(),
                    provider: "openrouter".to_string(),
                    latency_ms: 5000,
                    reliability: 1.0,
                    capabilities: vec![ModelCapability::Json, ModelCapability::Tools],
                    max_context: 262_144,
                    tier: None,
                });
            }
        }

        // Groq Console (Tier 1 Ultra-fast)
        if let Ok(key) = std::env::var("GROQ_API_KEY") {
            fabric.register_provider(Box::new(OpenAiProvider {
                name: "groq".to_string(),
                base_url: "https://api.groq.com/openai/v1".to_string(),
                api_key: key,
            }));
            if !registry_loaded {
                fabric.register_model(ModelInfo {
                    id: "llama-3.3-70b-versatile".to_string(),
                    family: "llama-3.3-70b".to_string(),
                    provider: "groq".to_string(),
                    latency_ms: 200,
                    reliability: 1.0,
                    capabilities: vec![ModelCapability::Json, ModelCapability::Tools],
                    max_context: 128000,
                    tier: None,
                });
            }
        }

        // Cerebras Cloud (Tier 1 Fast)
        if let Ok(key) = std::env::var("CEREBRAS_API_KEY") {
            fabric.register_provider(Box::new(OpenAiProvider {
                name: "cerebras".to_string(),
                base_url: "https://api.cerebras.ai/v1".to_string(),
                api_key: key,
            }));
            if !registry_loaded {
                fabric.register_model(ModelInfo {
                    id: "llama3.3-70b".to_string(),
                    family: "llama-3.3-70b".to_string(),
                    provider: "cerebras".to_string(),
                    latency_ms: 100,
                    reliability: 1.0,
                    capabilities: vec![ModelCapability::Json, ModelCapability::Tools],
                    max_context: 128000,
                    tier: None,
                });
            }
        }

        // Google AI Studio
        if let Ok(key) = std::env::var("GEMINI_API_KEY") {
            fabric.register_provider(Box::new(OpenAiProvider {
                name: "gemini".to_string(),
                base_url: "https://generativelanguage.googleapis.com/v1beta/openai/".to_string(),
                api_key: key,
            }));
            if !registry_loaded {
                fabric.register_model(ModelInfo {
                    id: "gemini-3.5-flash".to_string(),
                    family: "gemini-3.5-flash".to_string(),
                    provider: "gemini".to_string(),
                    latency_ms: 600,
                    reliability: 1.0,
                    capabilities: vec![
                        ModelCapability::Json,
                        ModelCapability::Tools,
                        ModelCapability::LongContext,
                    ],
                    max_context: 1_000_000,
                    tier: None,
                });
            }
        }

        // NVIDIA NIMs (Tier 2 — fast inference, OpenAI-compatible)
        // Free tier: ~40 req/min, no credit card, credits never expire
        if let Ok(key) = std::env::var("NVIDIA_NIM_API_KEY") {
            fabric.register_provider(Box::new(OpenAiProvider {
                name: "nvidia-nim".to_string(),
                base_url: "https://integrate.api.nvidia.com/v1".to_string(),
                api_key: key,
            }));
            if !registry_loaded {
                fabric.register_model(ModelInfo {
                    id: "nvidia/nemotron-3-super-120b-a12b".to_string(),
                    family: "nemotron-3-super".to_string(),
                    provider: "nvidia-nim".to_string(),
                    latency_ms: 2000,
                    reliability: 1.0,
                    capabilities: vec![ModelCapability::Json, ModelCapability::Tools],
                    max_context: 131_072,
                    tier: None,
                });
            }
        }

        // Local Ollama
        fabric.register_provider(Box::new(OpenAiProvider {
            name: "ollama".to_string(),
            base_url: "http://localhost:11434/v1".to_string(),
            api_key: "ollama".to_string(),
        }));
        if !registry_loaded {
            fabric.register_model(ModelInfo {
                id: "qwen2.5-coder:latest".to_string(),
                family: "qwen2.5-coder".to_string(),
                provider: "ollama".to_string(),
                latency_ms: 1000,
                reliability: 1.0,
                capabilities: vec![
                    ModelCapability::Json,
                    ModelCapability::Tools,
                    ModelCapability::Local,
                ],
                max_context: 32768,
                tier: None,
            });
        }

        // llama.cpp (llama-server) — OpenAI-compatible local backend.
        // Preferred local engine for low first-token latency (M8 voice roadmap).
        // Registered when the server is running (LLAMACPP_ENABLED=true|1); the
        // served GGUF is whatever `-m` loaded at startup, so the model id is a
        // label only — override the default with LLAMACPP_MODEL.
        {
            let base_url = std::env::var("LLAMACPP_BASE_URL")
                .unwrap_or_else(|_| "http://localhost:8080/v1".to_string());
            let enabled = std::env::var("LLAMACPP_ENABLED")
                .map(|v| v == "true" || v == "1")
                .unwrap_or(false);
            if enabled {
                fabric.register_provider(Box::new(OpenAiProvider {
                    name: "llamacpp".to_string(),
                    base_url,
                    api_key: "unused".to_string(),
                }));
                if !registry_loaded {
                    fabric.register_model(ModelInfo {
                        id: std::env::var("LLAMACPP_MODEL")
                            .unwrap_or_else(|_| "gemma4:e4b".to_string()),
                        family: "gemma4-e4b".to_string(),
                        provider: "llamacpp".to_string(),
                        latency_ms: 120,
                        reliability: 1.0,
                        capabilities: vec![
                            ModelCapability::Json,
                            ModelCapability::Tools,
                            ModelCapability::Local,
                        ],
                        max_context: 32_768,
                        tier: None,
                    });
                }
                tracing::info!("Registered llama.cpp provider (OpenAI-compatible local backend).");
            }
        }

        // OmniRoute (self-hosted multi-provider gateway; see Phase 1 integration notes)
        // Enabled via OMNIROUTE_ENABLED=true or a provided OMNIROUTE_API_KEY.
        // Base URL defaults to the local gateway; override with OMNIROUTE_BASE_URL.
        {
            let base_url = std::env::var("OMNIROUTE_BASE_URL")
                .unwrap_or_else(|_| "http://localhost:20128/v1".to_string());
            let enabled = std::env::var("OMNIROUTE_ENABLED")
                .map(|v| v == "true" || v == "1")
                .unwrap_or(false);
            let api_key = std::env::var("OMNIROUTE_API_KEY").unwrap_or_default();

            if enabled || !api_key.is_empty() {
                fabric.register_provider(Box::new(OmniRouteProvider::new(base_url, api_key)));
                if !registry_loaded {
                    // OmniRoute itself performs provider/model routing, so we expose a
                    // single "auto" candidate that lets it choose the best backend.
                    fabric.register_model(ModelInfo {
                        id: "auto".to_string(),
                        family: "omniroute-auto".to_string(),
                        provider: "omniroute".to_string(),
                        latency_ms: 500,
                        reliability: 1.0,
                        capabilities: vec![
                            ModelCapability::Json,
                            ModelCapability::Tools,
                            ModelCapability::LongContext,
                        ],
                        max_context: 200_000,
                        tier: None,
                    });
                }
                tracing::info!("Registered OmniRoute provider (multi-provider gateway).");
            }
        }

        // OpenGateway (gitlawb.com) — OpenAI-compatible multi-provider gateway.
        // NOTE: this is gitlawb's OpenGateway, NOT a GitLab service. Free models
        // (verified against the live /v1/models catalog): nemotron-3-ultra,
        // ling-3.0-flash, macaron-v1-tall, plus `auto` smart routing.
        if let Ok(key) = std::env::var("OPENGATEWAY_API_KEY") {
            let base_url = std::env::var("OPENGATEWAY_BASE_URL")
                .unwrap_or_else(|_| "https://opengateway.gitlawb.com/v1".to_string());
            fabric.register_provider(Box::new(OpenAiProvider {
                name: "opengateway".to_string(),
                base_url,
                api_key: key,
            }));
            if !registry_loaded {
                for m in opengateway_models() {
                    fabric.register_model(m);
                }
            }
            tracing::info!("Registered OpenGateway provider (gitlawb, OpenAI-compatible).");
        }

        // SambaNova Cloud (free tier) — OpenAI-compatible, US-hosted, no card.
        // Free tier: 20 req/min, 20 req/day, 200K tokens/day; solid open models
        // (Llama-3.3-70B, DeepSeek-V3.1, gpt-oss-120b). Base URL overridable
        // via SAMBA_BASE_URL (in case the hosting moves post-acquisition).
        if let Ok(key) = std::env::var("SAMBA_API_KEY") {
            let base_url = std::env::var("SAMBA_BASE_URL")
                .unwrap_or_else(|_| "https://api.sambanova.ai/v1".to_string());
            fabric.register_provider(Box::new(OpenAiProvider {
                name: "sambanova".to_string(),
                base_url,
                api_key: key,
            }));
            if !registry_loaded {
                for m in sambanova_models() {
                    fabric.register_model(m);
                }
            }
            tracing::info!("Registered SambaNova provider (free tier).");
        }

        // GitHub Models (free tier) — OpenAI-compatible, US-hosted. Free frontier
        // proprietary models (gpt-4o, o4-mini) + open models, no separate billing.
        // Free tier: 10-15 req/min, 50-150 req/day depending on model. Base URL
        // overridable via GITHUB_MODELS_BASE_URL.
        //
        // Opt-in gate: GITHUB_TOKEN is set by default in GitHub Actions and many
        // CI systems, so a bare token must NOT auto-register this provider (that
        // would silently add a cloud candidate to every CI run). Register only
        // with an explicit GITHUB_MODELS_KEY, or with GITHUB_MODELS_ENABLED=true
        // plus the generic GITHUB_TOKEN.
        {
            let key = github_models_key(
                std::env::var("GITHUB_MODELS_KEY").ok().as_deref(),
                std::env::var("GITHUB_MODELS_ENABLED")
                    .map(|v| v == "true" || v == "1")
                    .unwrap_or(false),
                std::env::var("GITHUB_TOKEN").ok().as_deref(),
            );
            if let Some(key) = key {
                let base_url = std::env::var("GITHUB_MODELS_BASE_URL")
                    .unwrap_or_else(|_| "https://models.github.ai/inference".to_string());
                fabric.register_provider(Box::new(OpenAiProvider {
                    name: "github-models".to_string(),
                    base_url,
                    api_key: key,
                }));
                if !registry_loaded {
                    for m in github_models() {
                        fabric.register_model(m);
                    }
                }
                tracing::info!("Registered GitHub Models provider (free tier).");
            }
        }

        // SiliconFlow (free models) — OpenAI-compatible, CHINA-hosted. Strong
        // free open models (Qwen3-8B, DeepSeek-R1-Distill). ~30 req/min free.
        // Deliberately Free-mode-only (never Hybrid/Quality): the fabric's
        // privacy stance is "local or Western-hosted for private code", so this
        // provider is opt-in and never a silent failover target.
        if let Ok(key) = std::env::var("SILICONFLOW_API_KEY") {
            fabric.register_provider(Box::new(OpenAiProvider {
                name: "siliconflow".to_string(),
                base_url: "https://api.siliconflow.cn/v1".to_string(),
                api_key: key,
            }));
            if !registry_loaded {
                for m in siliconflow_models() {
                    fabric.register_model(m);
                }
            }
            tracing::info!("Registered SiliconFlow provider (free models, CN-hosted).");
        }

        // Zhipu GLM (BigModel) — OpenAI-compatible, CHINA-hosted. Free tier:
        // GLM-4.7-Flash (strong free coding model), ~1 concurrent request. Same
        // privacy stance as SiliconFlow: Free-mode-only, opt-in.
        if let Ok(key) = std::env::var("ZHIPU_API_KEY") {
            fabric.register_provider(Box::new(OpenAiProvider {
                name: "zhipu".to_string(),
                base_url: "https://open.bigmodel.cn/api/paas/v4".to_string(),
                api_key: key,
            }));
            if !registry_loaded {
                for m in zhipu_models() {
                    fabric.register_model(m);
                }
            }
            tracing::info!("Registered Zhipu GLM provider (free tier, CN-hosted).");
        }

        fabric.load_health();

        // Qualification profiles: immutable offline benchmark artifacts that
        // seed routing signals (see `qualification.rs`). Additive only — never
        // changes selection or the audit trail.
        if let Ok(dir) = std::env::var("QUALIFICATION_DIR") {
            match fabric.load_qualification_profiles(std::path::Path::new(&dir)) {
                Ok(n) => tracing::info!("Loaded {n} qualification profile(s) from {dir}"),
                Err(e) => tracing::warn!("Failed to load qualification profiles from {dir}: {e}"),
            }
        }

        if security_vault.store_count() > 0 {
            fabric = fabric.with_egress(Arc::new(EgressGate::new(security_vault)));
        }

        fabric
    }
}

/// The Inference Fabric is the model client for the Investigation Engine's
/// LLM-backed strategy. The strategy stays model-agnostic (it only knows
/// [`friday_investigation::LlmStrategyClient`]); this impl wires the real
/// routing fabric in: free-first requirement-driven routing with failover
/// across the catalog, and the strategy's heuristic catches any residual
/// routing failure — so an unreachable model still produces a plan.
#[async_trait]
impl friday_investigation::LlmStrategyClient for InferenceFabric {
    async fn complete(&self, prompt: &str) -> Result<String, String> {
        self.complete_with(&RequestRequirements::default(), prompt)
            .await
    }
}

/// Recursively collect every `qualification.json` under `dir` (missing or
/// unreadable directories contribute nothing).
fn collect_qual_files(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_qual_files(&path, out);
        } else if path.file_name().and_then(|n| n.to_str()) == Some("qualification.json") {
            out.push(path);
        }
    }
}

/// Single source of truth for the free-tier provider allowlist — the only
/// place that enumerates free providers. Both the mode-based Free arm of
/// [`InferenceFabric::select_candidates`] and the budget filter
/// [`InferenceFabric::free_eligible`] consult it, so adding the next free
/// provider updates exactly one list. Local-capability models are additionally
/// free (`free_eligible` adds that); a provider is free when its entire tier is
/// free (OpenRouter/Gemini/OpenGateway/OmniRoute/SambaNova/GitHub Models) or
/// when it is a free-models-only CN-hosted tier (SiliconFlow/Zhipu — which stay
/// Free-mode-only by the privacy stance, never silent Hybrid failovers).
fn is_free_provider(name: &str) -> bool {
    matches!(
        name,
        "openrouter"
            | "omniroute"
            | "gemini"
            | "opengateway"
            | "sambanova"
            | "github-models"
            | "siliconflow"
            | "zhipu"
    )
}

/// OpenGateway (gitlawb) model catalog — single source of truth for the free
/// tiers FRIDAY resolves. Tier → concrete model ID (verified against the live
/// `/v1/models` catalog; `auto` is smart routing that escalates on failure):
///   reasoning-free     → mindai/macaron-v1-tall (verified 200 + reasoning_content)
///   long-context-free  → inclusionai/ling-3.0-flash:free (262K ctx)
///   conversation-fast  → nvidia/nemotron-3-ultra-550b-a55b:free
///   auto               → gateway smart routing (per-key daily free quota)
pub fn opengateway_models() -> Vec<ModelInfo> {
    vec![
        ModelInfo {
            id: "auto".to_string(),
            family: "opengateway-auto".to_string(),
            provider: "opengateway".to_string(),
            latency_ms: 800,
            reliability: 1.0,
            capabilities: vec![
                ModelCapability::Json,
                ModelCapability::Tools,
                ModelCapability::LongContext,
            ],
            max_context: 200_000,
            tier: Some(CapabilityTier::Auto),
        },
        ModelInfo {
            id: "nvidia/nemotron-3-ultra-550b-a55b:free".to_string(),
            family: "nemotron-3-ultra".to_string(),
            provider: "opengateway".to_string(),
            latency_ms: 2500,
            reliability: 1.0,
            capabilities: vec![
                ModelCapability::Json,
                ModelCapability::Tools,
                ModelCapability::LongContext,
            ],
            max_context: 131_072,
            tier: Some(CapabilityTier::ConversationFast),
        },
        ModelInfo {
            id: "inclusionai/ling-3.0-flash:free".to_string(),
            family: "ling-3.0-flash".to_string(),
            provider: "opengateway".to_string(),
            latency_ms: 2000,
            reliability: 1.0,
            capabilities: vec![
                ModelCapability::Json,
                ModelCapability::Tools,
                ModelCapability::LongContext,
            ],
            max_context: 262_144,
            tier: Some(CapabilityTier::LongContextFree),
        },
        ModelInfo {
            id: "mindai/macaron-v1-tall".to_string(),
            family: "macaron-v1-tall".to_string(),
            provider: "opengateway".to_string(),
            latency_ms: 3000,
            reliability: 1.0,
            capabilities: vec![ModelCapability::Json, ModelCapability::LongContext],
            max_context: 262_144,
            tier: Some(CapabilityTier::ReasoningFree),
        },
    ]
}

/// SambaNova Cloud free-tier catalog — verified OpenAI-compatible endpoints,
/// US-hosted, no credit card. Free tier: 20 req/min, 20 req/day, 200K
/// tokens/day. All three entries are strong open models that also speak tool
/// calling and JSON mode.
pub fn sambanova_models() -> Vec<ModelInfo> {
    vec![
        ModelInfo {
            id: "Meta-Llama-3.3-70B-Instruct".to_string(),
            family: "llama-3.3-70b".to_string(),
            provider: "sambanova".to_string(),
            latency_ms: 900,
            reliability: 1.0,
            capabilities: vec![
                ModelCapability::Json,
                ModelCapability::Tools,
                ModelCapability::LongContext,
            ],
            max_context: 131_072,
            tier: None,
        },
        ModelInfo {
            id: "DeepSeek-V3.1".to_string(),
            family: "deepseek-v3.1".to_string(),
            provider: "sambanova".to_string(),
            latency_ms: 1200,
            reliability: 1.0,
            capabilities: vec![
                ModelCapability::Json,
                ModelCapability::Tools,
                ModelCapability::LongContext,
            ],
            max_context: 131_072,
            tier: None,
        },
        ModelInfo {
            id: "gpt-oss-120b".to_string(),
            family: "gpt-oss-120b".to_string(),
            provider: "sambanova".to_string(),
            latency_ms: 1100,
            reliability: 1.0,
            capabilities: vec![
                ModelCapability::Json,
                ModelCapability::Tools,
                ModelCapability::LongContext,
            ],
            max_context: 131_072,
            tier: None,
        },
    ]
}

/// GitHub Models free-tier catalog — the only free tier offering frontier
/// proprietary models (gpt-4o, o4-mini) via a plain GitHub PAT. Free tier:
/// 10-15 req/min, 50-150 req/day (model-dependent). Base endpoint
/// `https://models.github.ai/inference`, OpenAI-compatible.
pub fn github_models() -> Vec<ModelInfo> {
    vec![
        ModelInfo {
            id: "gpt-4o".to_string(),
            family: "gpt-4o".to_string(),
            provider: "github-models".to_string(),
            latency_ms: 1500,
            reliability: 1.0,
            capabilities: vec![
                ModelCapability::Json,
                ModelCapability::Tools,
                ModelCapability::Vision,
                ModelCapability::LongContext,
            ],
            max_context: 128_000,
            tier: None,
        },
        ModelInfo {
            id: "o4-mini".to_string(),
            family: "o4-mini".to_string(),
            provider: "github-models".to_string(),
            latency_ms: 1200,
            reliability: 1.0,
            capabilities: vec![
                ModelCapability::Json,
                ModelCapability::Tools,
                ModelCapability::LongContext,
            ],
            max_context: 200_000,
            tier: None,
        },
        ModelInfo {
            id: "Llama-3.3-70B-Instruct".to_string(),
            family: "llama-3.3-70b".to_string(),
            provider: "github-models".to_string(),
            latency_ms: 1000,
            reliability: 1.0,
            capabilities: vec![ModelCapability::Json, ModelCapability::Tools],
            max_context: 128_000,
            tier: None,
        },
        ModelInfo {
            id: "DeepSeek-R1".to_string(),
            family: "deepseek-r1".to_string(),
            provider: "github-models".to_string(),
            latency_ms: 2500,
            reliability: 1.0,
            capabilities: vec![
                ModelCapability::Json,
                ModelCapability::Tools,
                ModelCapability::LongContext,
            ],
            max_context: 64_000,
            tier: None,
        },
    ]
}

/// SiliconFlow free-model catalog — strong open models, ~30 req/min free.
/// CN-hosted; the fabric deliberately keeps this provider Free-mode-only so it
/// is never a silent failover target for private code (see `select_candidates`).
pub fn siliconflow_models() -> Vec<ModelInfo> {
    vec![
        ModelInfo {
            id: "Qwen/Qwen3-8B".to_string(),
            family: "qwen3-8b".to_string(),
            provider: "siliconflow".to_string(),
            latency_ms: 800,
            reliability: 1.0,
            capabilities: vec![ModelCapability::Json, ModelCapability::Tools],
            max_context: 32_768,
            tier: None,
        },
        ModelInfo {
            id: "DeepSeek-R1-Distill-Qwen-7B".to_string(),
            family: "deepseek-r1-distill-qwen-7b".to_string(),
            provider: "siliconflow".to_string(),
            latency_ms: 1000,
            reliability: 1.0,
            capabilities: vec![ModelCapability::Json, ModelCapability::Tools],
            max_context: 32_768,
            tier: None,
        },
    ]
}

/// Zhipu GLM (BigModel) free-tier catalog — GLM-4.7-Flash is a strong free
/// coding/chat model (~1 concurrent request). CN-hosted; Free-mode-only for the
/// same privacy reason as SiliconFlow.
pub fn zhipu_models() -> Vec<ModelInfo> {
    vec![ModelInfo {
        id: "glm-4.7-flash".to_string(),
        family: "glm-4.7-flash".to_string(),
        provider: "zhipu".to_string(),
        latency_ms: 900,
        reliability: 1.0,
        capabilities: vec![
            ModelCapability::Json,
            ModelCapability::Tools,
            ModelCapability::LongContext,
        ],
        max_context: 128_000,
        tier: None,
    }]
}

/// Run one provider attempt under an optional deadline. Bounds a single
/// candidate call so a hung or pathologically slow provider fails over (chaos
/// laboratory Phase 1: hangs forever, extremely slow) instead of stalling the
/// whole request. `None` preserves the pre-chaos wait-forever behavior.
async fn run_attempt(
    deadline: Option<std::time::Duration>,
    fut: impl std::future::Future<Output = Result<String, String>>,
) -> Result<String, String> {
    match deadline {
        Some(d) => match tokio::time::timeout(d, fut).await {
            Ok(res) => res,
            Err(_) => Err(format!("request timed out after {:?}", d)),
        },
        None => fut.await,
    }
}

/// Collapse a provider error string into a coarse reason bucket for the
/// health-change telemetry log (e.g. `timeout`, `rate_limit`, `auth`,
/// `unavailable`, `server_error`, or `error`).
fn classify_failure_reason(err: &str) -> &str {
    let lower = err.to_lowercase();
    if lower.contains("timed out") || lower.contains("timeout") {
        "timeout"
    } else if lower.contains("429") || lower.contains("rate limit") || lower.contains("quota") {
        "rate_limit"
    } else if lower.contains("401")
        || lower.contains("403")
        || lower.contains("unauthorized")
        || lower.contains("auth")
    {
        "auth"
    } else if lower.contains("503") || lower.contains("unavailable") {
        "unavailable"
    } else if lower.contains("500")
        || lower.contains("502")
        || lower.contains("504")
        || lower.contains("server")
    {
        "server_error"
    } else {
        "error"
    }
}

/// Sibling path for an atomic persistence write: `<path>.tmp`. A crash after
/// writing the temp (but before rename) leaves the real file untouched; stale
/// temps are simply overwritten on the next persist.
fn temp_sibling(path: &std::path::Path) -> std::path::PathBuf {
    let mut os = path.as_os_str().to_owned();
    os.push(".tmp");
    std::path::PathBuf::from(os)
}

/// Clamp a persisted reliability into the valid `0.0..=1.0` range. Non-finite
/// (NaN/±inf) values are unclampable and rejected by the caller via `None`.
fn sanitize_reliability(r: f64) -> f64 {
    if r.is_finite() {
        r.clamp(0.0, 1.0)
    } else {
        1.0
    }
}

/// Opt-in gate for GitHub Models. `GITHUB_TOKEN` is set by default in GitHub
/// Actions and many CI systems, so a bare token must NEVER auto-register the
/// provider — that would silently add a cloud candidate to every CI run. An
/// explicit `GITHUB_MODELS_KEY` always wins; the generic token is used only
/// when `GITHUB_MODELS_ENABLED=true|1`.
fn github_models_key(
    explicit_key: Option<&str>,
    enabled: bool,
    generic_token: Option<&str>,
) -> Option<String> {
    match explicit_key {
        Some(k) => Some(k.to_string()),
        None if enabled => generic_token.map(|t| t.to_string()),
        None => None,
    }
}

impl InferenceFabric {
    /// Requirement-driven completion — the **Model Budget** path. Callers
    /// express [`RequestRequirements`]; the fabric filters and ranks within
    /// them (free-first cascade: local models first when privacy prefers it,
    /// then vision/long-context/tool matches, then quality/speed/health).
    /// Backward-compatible: [`ModelClient::complete`] keeps the mode-based
    /// selection path unchanged; this is the opt-in constraint surface.
    ///
    /// Audit-trail note: the frozen [`RoutingDecision`] v1 contract has no
    /// "requirements" field, so a requirement-driven decision records
    /// `routing_mode` as the fabric's default mode (e.g. `Free`) exactly like
    /// a mode-based one — the trail cannot distinguish a Model Budget request
    /// by shape. Selection is still fully auditable (candidates, chosen,
    /// served, failures); only the mode label is shared.
    pub async fn complete_with(
        &self,
        req: &RequestRequirements,
        prompt: &str,
    ) -> Result<String, String> {
        let request_id = uuid::Uuid::new_v4().to_string();
        tracing::info!(
            "[Inference Request {}] Starting requirement-driven routing (privacy={:?}, budget={:?}, latency={:?})",
            request_id,
            req.privacy,
            req.budget,
            req.latency_ms
        );
        let candidates = self.select_for(req);
        self.complete_candidates(&request_id, candidates, prompt)
            .await
    }

    /// Requirement-driven completion that returns usage metadata.
    pub async fn complete_with_usage(&self, prompt: &str) -> Result<ProviderResponse, String> {
        let request_id = uuid::Uuid::new_v4().to_string();
        tracing::info!(
            "[Inference Request {}] Starting completion routing (with usage)...",
            request_id
        );
        let candidates = self.select_candidates(self.default_mode);
        self.complete_candidates_with_usage(&request_id, candidates, prompt)
            .await
    }

    /// The shared attempt loop: iterate candidates in rank order, attempt each
    /// under the per-attempt deadline, record the immutable audit trail, and
    /// update health/circuits/metrics. Used by both the mode-based
    /// [`ModelClient::complete`] and the requirement-driven `complete_with`.
    async fn complete_candidates(
        &self,
        request_id: &str,
        candidates: Vec<(ModelInfo, &dyn Provider)>,
        prompt: &str,
    ) -> Result<String, String> {
        if candidates.is_empty() {
            let err_msg = "No eligible providers for this request.".to_string();
            let metrics = InferenceMetrics {
                request_id: request_id.to_string(),
                timestamp: chrono::Utc::now(),
                provider: "none".to_string(),
                model: "none".to_string(),
                latency_ms: 0,
                success: false,
                retry_count: 0,
                error: Some(err_msg.clone()),
            };
            self.log_metrics(&metrics);
            self.record_decision(self.start_decision(None, Vec::new(), None));
            return Err(err_msg);
        }

        // Snapshot the full routing trail BEFORE attempting, so evidence shows
        // every candidate considered (with its scores), not just the winner.
        let considerations: Vec<CandidateConsideration> = {
            let scores = self.health_scores.lock().unwrap();
            candidates
                .iter()
                .map(|(m, p)| CandidateConsideration {
                    provider: p.name().to_string(),
                    model: m.id.clone(),
                    reliability: scores.get(p.name()).copied().unwrap_or(1.0),
                    latency_ms: m.latency_ms,
                })
                .collect()
        };
        let mut decision = self.start_decision(
            None,
            considerations,
            candidates
                .first()
                .map(|(m, p)| (p.name().to_string(), m.id.clone())),
        );

        let mut last_error = String::new();
        let mut attempts = 0;
        let start = Instant::now();

        for (model, provider) in candidates {
            tracing::info!(
                "[Inference Request {}] Attempting candidate: {}/{}...",
                request_id,
                provider.name(),
                model.id
            );

            attempts += 1;
            // Egress gate (#12): scan the prompt before it leaves to this
            // candidate provider. `Err` = blocked — the payload never leaves,
            // and the block is recorded on the routing decision so it stays
            // auditable. A policy violation is NOT a provider failure: no
            // failover, no retry.
            let prompt = match self.egress_process(provider.name(), prompt) {
                Ok(prompt) => prompt,
                Err(e) => {
                    decision.failures.push(CandidateFailure {
                        provider: provider.name().to_string(),
                        model: model.id.clone(),
                        error: e.clone(),
                    });
                    self.record_decision(decision);
                    return Err(e);
                }
            };
            match run_attempt(
                self.request_timeout,
                provider.complete(&model.id, &prompt, false),
            )
            .await
            {
                Ok(response) => {
                    let latency_ms = start.elapsed().as_millis() as u64;
                    tracing::info!(
                        "[Inference Request {}] Success on candidate {}/{} ({}ms)",
                        request_id,
                        provider.name(),
                        model.id,
                        latency_ms
                    );

                    // Record which vendor model actually served AND why, so
                    // evidence packs show the real path (e.g. auto route quota
                    // → free-model fallback) rather than the declared target.
                    decision.served = Some((provider.name().to_string(), model.id.clone()));
                    self.record_decision(decision);

                    // Update reliability score
                    {
                        let mut guard = self.health_scores.lock().unwrap();
                        let (before, after) = if let Some(score) = guard.get_mut(provider.name()) {
                            let before = *score;
                            *score = (*score + 0.1).min(1.0);
                            (before, *score)
                        } else {
                            (0.0, 0.0)
                        };
                        drop(guard);
                        if (after - before).abs() > f64::EPSILON {
                            self.log_health_change(
                                provider.name(),
                                before,
                                after,
                                "success",
                                "recovered",
                            );
                        }
                    }

                    // Reset circuit breaker consecutive failure counter
                    {
                        let mut cb_guard = self.circuits.lock().unwrap();
                        if let Some(cb) = cb_guard.get_mut(provider.name()) {
                            cb.record_success();
                        }
                    }

                    self.persist_health();

                    // Log metrics
                    let metrics = InferenceMetrics {
                        request_id: request_id.to_string(),
                        timestamp: chrono::Utc::now(),
                        provider: provider.name().to_string(),
                        model: model.id.clone(),
                        latency_ms,
                        success: true,
                        retry_count: attempts - 1,
                        error: None,
                    };
                    self.log_metrics(&metrics);

                    return Ok(response);
                }
                Err(err) => {
                    tracing::warn!(
                        "[Inference Request {}] Candidate {}/{} failed: {}. Retrying next...",
                        request_id,
                        provider.name(),
                        model.id,
                        err
                    );

                    // Degrade reliability rating
                    let (health_before, health_after) = {
                        let mut guard = self.health_scores.lock().unwrap();
                        let (before, after) = if let Some(score) = guard.get_mut(provider.name()) {
                            let before = *score;
                            *score = (*score - 0.2).max(0.0);
                            (before, *score)
                        } else {
                            (0.0, 0.0)
                        };
                        drop(guard);
                        (before, after)
                    };

                    // Trip circuit failure
                    let circuit_opened = {
                        let mut cb_guard = self.circuits.lock().unwrap();
                        if let Some(cb) = cb_guard.get_mut(provider.name()) {
                            let state_before = cb.state;
                            cb.record_failure();
                            state_before != cb.state && cb.state == CircuitState::Open
                        } else {
                            false
                        }
                    };

                    if (health_after - health_before).abs() > f64::EPSILON {
                        self.log_health_change(
                            provider.name(),
                            health_before,
                            health_after,
                            classify_failure_reason(&err),
                            if circuit_opened {
                                "demoted"
                            } else {
                                "degraded"
                            },
                        );
                    }

                    self.persist_health();
                    decision.failures.push(CandidateFailure {
                        provider: provider.name().to_string(),
                        model: model.id.clone(),
                        error: err.clone(),
                    });
                    last_error = err;
                }
            }
        }

        let latency_ms = start.elapsed().as_millis() as u64;
        let final_err = format!("All candidates failed. Last error: {}", last_error);
        // Leave the immutable audit trail with every failure so the run is
        // debuggable — nothing served, but the why is fully recorded.
        decision.served = None;
        self.record_decision(decision);
        let metrics = InferenceMetrics {
            request_id: request_id.to_string(),
            timestamp: chrono::Utc::now(),
            provider: "all".to_string(),
            model: "none".to_string(),
            latency_ms,
            success: false,
            retry_count: attempts,
            error: Some(final_err.clone()),
        };
        self.log_metrics(&metrics);

        Err(final_err)
    }

    /// Like [`complete_candidates`] but returns usage metadata from the provider.
    /// Falls back to `None`-usage if the underlying provider doesn't override
    /// `complete_with_usage`.
    async fn complete_candidates_with_usage(
        &self,
        request_id: &str,
        candidates: Vec<(ModelInfo, &dyn Provider)>,
        prompt: &str,
    ) -> Result<ProviderResponse, String> {
        if candidates.is_empty() {
            let err_msg = "No eligible providers for this request.".to_string();
            tracing::error!("[Inference Request {}] {}", request_id, err_msg);
            return Err(err_msg);
        }

        let deadline = std::time::Duration::from_millis(3000);
        let mut attempts = 0;
        let mut last_err = String::new();

        for (model, provider) in &candidates {
            attempts += 1;
            tracing::info!(
                "[Inference Request {}] Attempting {}/{}: provider={}, model={}",
                request_id,
                attempts,
                candidates.len(),
                provider.name(),
                model.id
            );

            let start = std::time::Instant::now();
            let result = tokio::time::timeout(
                deadline,
                provider.complete_with_usage(&model.id, prompt, false),
            )
            .await;
            let latency = start.elapsed().as_millis() as u64;

            match result {
                Ok(Ok(response)) => {
                    let metrics = InferenceMetrics {
                        request_id: request_id.to_string(),
                        timestamp: chrono::Utc::now(),
                        provider: provider.name().to_string(),
                        model: model.id.clone(),
                        latency_ms: latency,
                        success: true,
                        retry_count: attempts,
                        error: None,
                    };
                    self.log_metrics(&metrics);

                    {
                        let mut guard = self.health_scores.lock().unwrap();
                        if let Some(score) = guard.get_mut(provider.name()) {
                            let before = *score;
                            *score = (*score + 0.1).min(1.0);
                            if (before - *score).abs() > f64::EPSILON {
                                self.log_health_change(
                                    provider.name(),
                                    before,
                                    *score,
                                    "success",
                                    "recovered",
                                );
                            }
                        }
                    }

                    {
                        let mut cb_guard = self.circuits.lock().unwrap();
                        if let Some(cb) = cb_guard.get_mut(provider.name()) {
                            cb.record_success();
                        }
                    }

                    self.persist_health();
                    return Ok(response);
                }
                Ok(Err(e)) => {
                    last_err = e;
                    tracing::warn!(
                        "[Inference Request {}] Provider {} failed: {}",
                        request_id,
                        provider.name(),
                        last_err
                    );
                    {
                        let mut guard = self.health_scores.lock().unwrap();
                        if let Some(score) = guard.get_mut(provider.name()) {
                            *score = (*score - 0.2).max(0.0);
                        }
                    }
                    {
                        let mut cb_guard = self.circuits.lock().unwrap();
                        if let Some(cb) = cb_guard.get_mut(provider.name()) {
                            cb.record_failure();
                        }
                    }
                }
                Err(_) => {
                    last_err = format!("Provider {} timed out", provider.name());
                    tracing::warn!("[Inference Request {}] {}", request_id, last_err);
                    {
                        let mut guard = self.health_scores.lock().unwrap();
                        if let Some(score) = guard.get_mut(provider.name()) {
                            *score = (*score - 0.2).max(0.0);
                        }
                    }
                    {
                        let mut cb_guard = self.circuits.lock().unwrap();
                        if let Some(cb) = cb_guard.get_mut(provider.name()) {
                            cb.record_failure();
                        }
                    }
                }
            }
        }

        let final_err = format!(
            "All {} providers failed. Last error: {}",
            candidates.len(),
            last_err
        );
        tracing::error!("[Inference Request {}] {}", request_id, final_err);

        let metrics = InferenceMetrics {
            request_id: request_id.to_string(),
            timestamp: chrono::Utc::now(),
            provider: "all".to_string(),
            model: "none".to_string(),
            latency_ms: 0,
            success: false,
            retry_count: attempts,
            error: Some(final_err.clone()),
        };
        self.log_metrics(&metrics);

        Err(final_err)
    }
}

#[async_trait]
impl ModelClient for InferenceFabric {
    async fn complete(&self, prompt: &str) -> Result<String, String> {
        let request_id = uuid::Uuid::new_v4().to_string();
        tracing::info!(
            "[Inference Request {}] Starting completion routing...",
            request_id
        );
        let candidates = self.select_candidates(self.default_mode);
        self.complete_candidates(&request_id, candidates, prompt)
            .await
    }
}

impl Default for InferenceFabric {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn omniroute_provider_constructs_with_omniroute_identity() {
        let p = OmniRouteProvider::new(
            "http://localhost:20128/v1".to_string(),
            "test-key".to_string(),
        );
        assert_eq!(p.name(), "omniroute");
        assert_eq!(p.base_url, "http://localhost:20128/v1");
        assert_eq!(p.api_key, "test-key");
    }

    #[test]
    fn fabric_registers_omniroute_and_selects_it_in_free_mode() {
        let mut fabric = InferenceFabric::new();
        fabric.register_provider(Box::new(OmniRouteProvider::new(
            "http://localhost:20128/v1".to_string(),
            String::new(),
        )));
        fabric.register_model(ModelInfo {
            id: "auto".to_string(),
            family: "omniroute-auto".to_string(),
            provider: "omniroute".to_string(),
            latency_ms: 500,
            reliability: 1.0,
            capabilities: vec![ModelCapability::Json, ModelCapability::Tools],
            max_context: 200_000,
            tier: None,
        });

        assert!(fabric.providers.contains_key("omniroute"));

        let candidates = fabric.select_candidates(RoutingMode::Free);
        assert!(
            candidates.iter().any(|(m, _)| m.provider == "omniroute"),
            "omniroute should be a Free-mode candidate"
        );

        let hybrid = fabric.select_candidates(RoutingMode::Hybrid);
        assert!(
            hybrid.iter().any(|(m, _)| m.provider == "omniroute"),
            "omniroute should be a Hybrid-mode candidate"
        );
    }

    #[test]
    fn omniroute_is_excluded_from_local_mode() {
        let mut fabric = InferenceFabric::new();
        fabric.register_provider(Box::new(OmniRouteProvider::new(
            "http://localhost:20128/v1".to_string(),
            String::new(),
        )));
        fabric.register_model(ModelInfo {
            id: "auto".to_string(),
            family: "omniroute-auto".to_string(),
            provider: "omniroute".to_string(),
            latency_ms: 500,
            reliability: 1.0,
            capabilities: vec![ModelCapability::Json, ModelCapability::Tools],
            max_context: 200_000,
            tier: None,
        });

        let candidates = fabric.select_candidates(RoutingMode::Local);
        assert!(
            !candidates.iter().any(|(m, _)| m.provider == "omniroute"),
            "omniroute must not be selected in Local-only mode"
        );
    }

    #[test]
    fn local_mode_routes_by_capability_not_provider_name() {
        let mut fabric = InferenceFabric::new();

        // llamacpp: local by capability, so eligible in Local and Hybrid modes.
        fabric.register_provider(Box::new(OpenAiProvider {
            name: "llamacpp".to_string(),
            base_url: "http://localhost:8080/v1".to_string(),
            api_key: "unused".to_string(),
        }));
        fabric.register_model(ModelInfo {
            id: "gemma4:e4b".to_string(),
            family: "gemma4-e4b".to_string(),
            provider: "llamacpp".to_string(),
            latency_ms: 120,
            reliability: 1.0,
            capabilities: vec![ModelCapability::Json, ModelCapability::Local],
            max_context: 32_768,
            tier: None,
        });

        // A model under the "ollama" provider name WITHOUT the Local capability
        // must no longer be routed as local (kills the provider-name leak).
        fabric.register_provider(Box::new(OpenAiProvider {
            name: "ollama".to_string(),
            base_url: "http://localhost:11434/v1".to_string(),
            api_key: "ollama".to_string(),
        }));
        fabric.register_model(ModelInfo {
            id: "remote:latest".to_string(),
            family: "remote".to_string(),
            provider: "ollama".to_string(),
            latency_ms: 300,
            reliability: 1.0,
            capabilities: vec![ModelCapability::Json],
            max_context: 8192,
            tier: None,
        });

        let local = fabric.select_candidates(RoutingMode::Local);
        assert!(
            local.iter().any(|(m, _)| m.provider == "llamacpp"),
            "llamacpp should be selected in Local mode by capability"
        );
        assert!(
            !local.iter().any(|(m, _)| m.provider == "ollama"),
            "provider-name 'ollama' must not imply local"
        );

        let hybrid = fabric.select_candidates(RoutingMode::Hybrid);
        assert!(
            hybrid.iter().any(|(m, _)| m.provider == "llamacpp"),
            "llamacpp should be selected in Hybrid mode by capability"
        );
    }

    #[tokio::test]
    async fn omniroute_health_check_is_graceful_when_unavailable() {
        // No OmniRoute instance should be running on this port in CI; must not panic.
        let p = OmniRouteProvider::new("http://localhost:19999/v1".to_string(), String::new());
        assert!(!p.check_health().await);
    }

    #[test]
    fn opengateway_is_registered_and_selected_in_free_and_hybrid_modes() {
        let mut fabric = InferenceFabric::new();
        fabric.register_provider(Box::new(OpenAiProvider {
            name: "opengateway".to_string(),
            base_url: "https://opengateway.gitlawb.com/v1".to_string(),
            api_key: String::new(),
        }));
        fabric.register_model(ModelInfo {
            id: "auto".to_string(),
            family: "opengateway-auto".to_string(),
            provider: "opengateway".to_string(),
            latency_ms: 800,
            reliability: 1.0,
            capabilities: vec![ModelCapability::Json, ModelCapability::Tools],
            max_context: 200_000,
            tier: None,
        });

        assert!(fabric.providers.contains_key("opengateway"));

        let free = fabric.select_candidates(RoutingMode::Free);
        assert!(
            free.iter().any(|(m, _)| m.provider == "opengateway"),
            "opengateway should be a Free-mode candidate"
        );

        let hybrid = fabric.select_candidates(RoutingMode::Hybrid);
        assert!(
            hybrid.iter().any(|(m, _)| m.provider == "opengateway"),
            "opengateway should be a Hybrid-mode candidate"
        );

        let local = fabric.select_candidates(RoutingMode::Local);
        assert!(
            !local.iter().any(|(m, _)| m.provider == "opengateway"),
            "opengateway must not be selected in Local-only mode"
        );
    }

    #[test]
    fn opengateway_capability_tiers_resolve_to_free_models() {
        let mut fabric = InferenceFabric::new();
        fabric.register_provider(Box::new(OpenAiProvider {
            name: "opengateway".to_string(),
            base_url: "https://opengateway.gitlawb.com/v1".to_string(),
            api_key: String::new(),
        }));
        for m in opengateway_models() {
            fabric.register_model(m);
        }

        // Callers name capabilities, never vendor model IDs.
        assert_eq!(
            fabric
                .resolve_tier("opengateway", CapabilityTier::ReasoningFree)
                .map(|m| m.id.as_str()),
            Some("mindai/macaron-v1-tall")
        );
        assert_eq!(
            fabric
                .resolve_tier("opengateway", CapabilityTier::LongContextFree)
                .map(|m| m.id.as_str()),
            Some("inclusionai/ling-3.0-flash:free")
        );
        assert_eq!(
            fabric
                .resolve_tier("opengateway", CapabilityTier::ConversationFast)
                .map(|m| m.id.as_str()),
            Some("nvidia/nemotron-3-ultra-550b-a55b:free")
        );
        assert_eq!(
            fabric
                .resolve_tier("opengateway", CapabilityTier::Auto)
                .map(|m| m.id.as_str()),
            Some("auto")
        );
        assert!(
            fabric
                .resolve_tier("opengateway", CapabilityTier::VisionFree)
                .is_none(),
            "no vision tier is registered for opengateway"
        );

        // Tag round-trip is stable.
        assert_eq!(
            CapabilityTier::from_tag(CapabilityTier::ReasoningFree.tag()),
            Some(CapabilityTier::ReasoningFree)
        );
        assert_eq!(CapabilityTier::from_tag("unknown-tier"), None);
    }

    /// `load_qualification_profiles` registers schema-compatible profiles and
    /// skips incompatible ones instead of guessing.
    #[test]
    fn load_qualification_profiles_registers_compatible_and_skips_incompatible() {
        let dir = std::env::temp_dir().join(format!("qual-load-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();

        let profile = crate::qualification::QualificationProfile {
            schema_version: crate::qualification::QUALIFICATION_SCHEMA_VERSION,
            provider_id: "llamacpp".into(),
            model_id: "gemma4:e4b".into(),
            benchmark_config: crate::qualification::BenchmarkConfig {
                prompt_set: "test".into(),
                iterations: 1,
                context_size: 8192,
                temperature: 0.0,
                gpu_layers: 0,
                batch_size: 1,
            },
            generated_at_ms: 0,
            ttft_ms: Some(100.0),
            tokens_per_second: None,
            json_accuracy: Some(0.9),
            tool_accuracy: Some(1.0),
            peak_ram_mb: None,
            peak_vram_mb: None,
            benchmark_duration_ms: 10,
        };
        std::fs::write(
            dir.join("qualification.json"),
            serde_json::to_string(&profile).unwrap(),
        )
        .unwrap();

        // A future-schema profile must be skipped, not misread. It lives in its
        // own evidence pack dir, like real harness output would.
        let ollama_dir = dir.join("ollama");
        std::fs::create_dir_all(&ollama_dir).unwrap();
        let incompatible = serde_json::json!({
            "schema_version": 999,
            "provider_id": "ollama",
            "model_id": "qwen2.5-coder:latest"
        });
        std::fs::write(
            ollama_dir.join("qualification.json"),
            serde_json::to_string(&incompatible).unwrap(),
        )
        .unwrap();

        let fabric = InferenceFabric::new();
        let loaded = fabric.load_qualification_profiles(&dir).unwrap();
        assert_eq!(loaded, 1, "only the compatible profile is registered");

        assert!(fabric
            .qualification_profile("llamacpp", "gemma4:e4b")
            .is_some());
        assert!(
            fabric
                .qualification_profile("ollama", "qwen2.5-coder:latest")
                .is_none(),
            "incompatible schema version must not be registered"
        );

        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// Test-only provider whose `complete` either always fails or always succeeds.
    struct ScriptedProvider {
        name: &'static str,
        fail: bool,
    }

    #[async_trait]
    impl Provider for ScriptedProvider {
        fn name(&self) -> &str {
            self.name
        }
        async fn complete(
            &self,
            _model_id: &str,
            _prompt: &str,
            _require_json: bool,
        ) -> Result<String, String> {
            if self.fail {
                Err("429 free-models-per-day-high-balance".to_string())
            } else {
                Ok("ok".to_string())
            }
        }
        async fn check_health(&self) -> bool {
            true
        }
    }

    #[tokio::test]
    async fn routing_decision_records_full_audit_trail_on_failover() {
        let mut fabric = InferenceFabric::new();
        fabric.default_mode = RoutingMode::Free;
        fabric.register_provider(Box::new(ScriptedProvider {
            name: "openrouter",
            fail: true,
        }));
        fabric.register_provider(Box::new(ScriptedProvider {
            name: "opengateway",
            fail: false,
        }));
        let model = |id: &str, provider: &str, latency_ms: u64| ModelInfo {
            id: id.to_string(),
            family: id.to_string(),
            provider: provider.to_string(),
            latency_ms,
            reliability: 1.0,
            capabilities: vec![],
            max_context: 4096,
            tier: None,
        };
        fabric.register_model(model("vend/alpha:free", "openrouter", 5));
        fabric.register_model(model("vend/beta:free", "opengateway", 10));

        let text = fabric
            .complete("hi")
            .await
            .expect("steady provider answers");
        assert_eq!(text, "ok");

        let d = fabric.last_decision().expect("decision recorded");
        // Frozen contract v1: versioned + timestamped.
        assert_eq!(d.version, 1);
        assert!(!d.timestamp.is_empty(), "decision must be timestamped");
        // Mode-based routing: no capability tier was requested; mode is Free.
        assert!(d.requested_tier.is_none());
        assert_eq!(d.routing_mode, "Free");
        // Candidates in routing order (reliability equal → latency asc).
        assert_eq!(d.candidates.len(), 2);
        assert_eq!(d.candidates[0].model, "vend/alpha:free");
        assert_eq!(d.candidates[1].model, "vend/beta:free");
        assert_eq!(d.candidates[0].reliability, 1.0);
        // Chosen was the top-ranked candidate; served was the one that answered.
        assert_eq!(
            d.chosen,
            Some(("openrouter".to_string(), "vend/alpha:free".to_string()))
        );
        assert_eq!(
            d.served,
            Some(("opengateway".to_string(), "vend/beta:free".to_string()))
        );
        // Failure Reasons, structured: the chosen candidate was rate-limited.
        assert_eq!(d.failures.len(), 1);
        assert_eq!(d.failures[0].provider, "openrouter");
        assert_eq!(d.failures[0].model, "vend/alpha:free");
        assert!(
            d.failures[0].error.contains("429"),
            "unexpected error: {}",
            d.failures[0].error
        );
        // The derived served helper agrees with the audit trail.
        assert_eq!(
            fabric.last_served(),
            Some(("opengateway".to_string(), "vend/beta:free".to_string()))
        );
    }

    /// Build a fabric with one provider registered under `name` and its catalog
    /// models loaded — mirrors what `default_from_env` does for real env keys.
    fn provider_fabric(name: &str, base: &str, models: Vec<ModelInfo>) -> InferenceFabric {
        let mut fabric = InferenceFabric::new();
        fabric.register_provider(Box::new(OpenAiProvider {
            name: name.to_string(),
            base_url: base.to_string(),
            api_key: String::new(),
        }));
        for m in models {
            fabric.register_model(m);
        }
        fabric
    }

    #[test]
    fn sambanova_models_are_selected_in_free_and_hybrid_but_not_local() {
        let fabric = provider_fabric(
            "sambanova",
            "https://api.sambanova.ai/v1",
            sambanova_models(),
        );
        assert!(fabric.providers.contains_key("sambanova"));

        let free = fabric.select_candidates(RoutingMode::Free);
        assert!(
            free.iter().any(|(m, _)| m.provider == "sambanova"),
            "sambanova should be a Free-mode candidate"
        );

        let hybrid = fabric.select_candidates(RoutingMode::Hybrid);
        assert!(
            hybrid.iter().any(|(m, _)| m.provider == "sambanova"),
            "sambanova should be a Hybrid-mode candidate"
        );

        let local = fabric.select_candidates(RoutingMode::Local);
        assert!(
            !local.iter().any(|(m, _)| m.provider == "sambanova"),
            "sambanova must not be selected in Local-only mode"
        );
    }

    #[test]
    fn github_models_are_selected_in_free_and_hybrid_but_not_local() {
        let fabric = provider_fabric(
            "github-models",
            "https://models.github.ai/inference",
            github_models(),
        );
        assert!(fabric.providers.contains_key("github-models"));

        let free = fabric.select_candidates(RoutingMode::Free);
        assert!(
            free.iter().any(|(m, _)| m.provider == "github-models"),
            "github-models should be a Free-mode candidate"
        );

        let hybrid = fabric.select_candidates(RoutingMode::Hybrid);
        assert!(
            hybrid.iter().any(|(m, _)| m.provider == "github-models"),
            "github-models should be a Hybrid-mode candidate"
        );

        let local = fabric.select_candidates(RoutingMode::Local);
        assert!(
            !local.iter().any(|(m, _)| m.provider == "github-models"),
            "github-models must not be selected in Local-only mode"
        );
    }

    /// The CN-hosted free providers (SiliconFlow, Zhipu) are deliberate
    /// Free-mode-ONLY: they must never appear as Hybrid or Local candidates so
    /// private code never silently fails over to a China-hosted endpoint.
    #[test]
    fn siliconflow_and_zhipu_are_free_only_by_privacy_stance() {
        let fabric = provider_fabric(
            "siliconflow",
            "https://api.siliconflow.cn/v1",
            siliconflow_models(),
        );
        assert!(fabric.providers.contains_key("siliconflow"));
        let free = fabric.select_candidates(RoutingMode::Free);
        assert!(
            free.iter().any(|(m, _)| m.provider == "siliconflow"),
            "siliconflow should be a Free-mode candidate"
        );
        let hybrid = fabric.select_candidates(RoutingMode::Hybrid);
        assert!(
            !hybrid.iter().any(|(m, _)| m.provider == "siliconflow"),
            "siliconflow must NOT be a Hybrid candidate (CN-hosted privacy stance)"
        );

        let fabric = provider_fabric(
            "zhipu",
            "https://open.bigmodel.cn/api/paas/v4",
            zhipu_models(),
        );
        assert!(fabric.providers.contains_key("zhipu"));
        let free = fabric.select_candidates(RoutingMode::Free);
        assert!(
            free.iter().any(|(m, _)| m.provider == "zhipu"),
            "zhipu should be a Free-mode candidate"
        );
        let hybrid = fabric.select_candidates(RoutingMode::Hybrid);
        assert!(
            !hybrid.iter().any(|(m, _)| m.provider == "zhipu"),
            "zhipu must NOT be a Hybrid candidate (CN-hosted privacy stance)"
        );
    }

    #[test]
    fn new_free_catalogs_carry_tool_capabilities() {
        for m in sambanova_models()
            .into_iter()
            .chain(github_models())
            .chain(siliconflow_models())
            .chain(zhipu_models())
        {
            assert!(
                m.capabilities.contains(&ModelCapability::Json)
                    && m.capabilities.contains(&ModelCapability::Tools),
                "{} ({}): free providers must advertise Json+Tools",
                m.provider,
                m.id
            );
        }
    }

    /// A bare GITHUB_TOKEN (set by default in GitHub Actions) must never
    /// auto-register the GitHub Models provider — CI safety. An explicit
    /// GITHUB_MODELS_KEY or GITHUB_MODELS_ENABLED=true is required.
    #[test]
    fn github_models_requires_explicit_opt_in_for_generic_token() {
        // Bare token, no enable flag → NOT registered (the CI footgun case).
        assert_eq!(github_models_key(None, false, Some("ci-token")), None);
        // Token + explicit enable flag → registered.
        assert_eq!(
            github_models_key(None, true, Some("pat")),
            Some("pat".to_string())
        );
        // Explicit key always wins, even alongside a stray CI token.
        assert_eq!(
            github_models_key(Some("models-key"), false, Some("ci-token")),
            Some("models-key".to_string())
        );
        // Nothing configured → nothing registered.
        assert_eq!(github_models_key(None, false, None), None);
    }

    /// Build a `ModelInfo` for budget-routing tests: provider + latency + caps.
    fn mi(id: &str, provider: &str, latency_ms: u64, caps: &[ModelCapability]) -> ModelInfo {
        ModelInfo {
            id: id.to_string(),
            family: id.to_string(),
            provider: provider.to_string(),
            latency_ms,
            reliability: 1.0,
            capabilities: caps.to_vec(),
            max_context: 4096,
            tier: None,
        }
    }

    /// A fabric with three providers: a free cloud tier (openrouter), a paid
    /// tier (groq), and a local model (ollama, Local capability).
    fn budget_fabric() -> InferenceFabric {
        let mut fabric = InferenceFabric::new();
        for name in ["openrouter", "groq", "ollama"] {
            fabric.register_provider(Box::new(OpenAiProvider {
                name: name.to_string(),
                base_url: "http://localhost/v1".to_string(),
                api_key: String::new(),
            }));
        }
        fabric.register_model(mi(
            "openrouter/free",
            "openrouter",
            2000,
            &[ModelCapability::Tools],
        ));
        fabric.register_model(mi("llama-3.3-70b", "groq", 200, &[ModelCapability::Tools]));
        fabric.register_model(mi(
            "qwen2.5-coder",
            "ollama",
            1000,
            &[ModelCapability::Tools, ModelCapability::Local],
        ));
        fabric
    }

    #[test]
    fn select_for_free_budget_excludes_paid_providers() {
        let fabric = budget_fabric();
        let req = RequestRequirements::default(); // budget = Free
        let picks = fabric.select_for(&req);
        let providers: Vec<&str> = picks.iter().map(|(m, _)| m.provider.as_str()).collect();
        assert!(
            providers.contains(&"openrouter"),
            "free cloud eligible: {providers:?}"
        );
        assert!(
            providers.contains(&"ollama"),
            "local model is free: {providers:?}"
        );
        assert!(
            !providers.contains(&"groq"),
            "paid provider must be excluded under budget=Free: {providers:?}"
        );
    }

    #[test]
    fn select_for_bounded_budget_opens_paid_providers() {
        let fabric = budget_fabric();
        let req = RequestRequirements {
            budget: Budget::Bounded,
            ..RequestRequirements::default()
        };
        let picks = fabric.select_for(&req);
        let providers: Vec<&str> = picks.iter().map(|(m, _)| m.provider.as_str()).collect();
        assert!(
            providers.contains(&"groq"),
            "bounded budget admits paid: {providers:?}"
        );
    }

    #[test]
    fn select_for_privacy_required_excludes_remote_models() {
        let fabric = budget_fabric();
        let req = RequestRequirements {
            budget: Budget::Bounded,
            privacy: Preference::Required,
            ..RequestRequirements::default()
        };
        let picks = fabric.select_for(&req);
        let providers: Vec<&str> = picks.iter().map(|(m, _)| m.provider.as_str()).collect();
        assert_eq!(
            picks.len(),
            1,
            "only the local model survives: {providers:?}"
        );
        assert_eq!(picks[0].0.provider, "ollama");
    }

    #[test]
    fn select_for_latency_bound_excludes_slow_candidates() {
        let fabric = budget_fabric();
        let req = RequestRequirements {
            budget: Budget::Bounded,
            latency_ms: Some(500),
            ..RequestRequirements::default()
        };
        let picks = fabric.select_for(&req);
        let providers: Vec<&str> = picks.iter().map(|(m, _)| m.provider.as_str()).collect();
        assert_eq!(
            picks.len(),
            1,
            "only groq (200ms) is under 500ms: {providers:?}"
        );
        assert_eq!(picks[0].0.provider, "groq");
    }

    #[test]
    fn select_for_required_vision_excludes_non_vision_models() {
        let mut fabric = budget_fabric();
        fabric.register_model(mi(
            "gpt-4o",
            "github-models",
            1500,
            &[
                ModelCapability::Tools,
                ModelCapability::Vision,
                ModelCapability::LongContext,
            ],
        ));
        fabric.register_provider(Box::new(OpenAiProvider {
            name: "github-models".to_string(),
            base_url: "http://localhost/v1".to_string(),
            api_key: String::new(),
        }));
        let req = RequestRequirements {
            budget: Budget::Bounded,
            vision: Requirement::Required,
            ..RequestRequirements::default()
        };
        let picks = fabric.select_for(&req);
        let providers: Vec<&str> = picks.iter().map(|(m, _)| m.provider.as_str()).collect();
        assert_eq!(
            picks.len(),
            1,
            "only the vision model survives: {providers:?}"
        );
        assert_eq!(picks[0].0.provider, "github-models");
    }

    #[test]
    fn select_for_honors_fabric_global_required_capabilities() {
        // The fabric-global capability gate must apply on BOTH selection paths:
        // select_candidates filters by it, so select_for must too, or a caller
        // setting `required_capabilities` gets different sets per entry point.
        let mut fabric = budget_fabric();
        fabric.register_provider(Box::new(OpenAiProvider {
            name: "github-models".to_string(),
            base_url: "http://localhost/v1".to_string(),
            api_key: String::new(),
        }));
        fabric.register_model(mi(
            "gpt-4o",
            "github-models",
            1500,
            &[ModelCapability::Vision],
        ));
        fabric.required_capabilities = vec![ModelCapability::Vision];

        let picks = fabric.select_for(&RequestRequirements::default());
        let providers: Vec<&str> = picks.iter().map(|(m, _)| m.provider.as_str()).collect();
        assert_eq!(
            providers,
            vec!["github-models"],
            "global gate must apply to select_for: {providers:?}"
        );
    }

    #[test]
    fn select_for_privacy_preferred_ranks_local_above_remote() {
        let fabric = budget_fabric();
        let req = RequestRequirements {
            privacy: Preference::Preferred,
            ..RequestRequirements::default()
        };
        let picks = fabric.select_for(&req);
        let providers: Vec<&str> = picks.iter().map(|(m, _)| m.provider.as_str()).collect();
        assert_eq!(
            picks.len(),
            2,
            "free budget keeps openrouter + local: {providers:?}"
        );
        assert_eq!(
            picks[0].0.provider, "ollama",
            "privacy=Preferred must rank the local model first: {providers:?}"
        );
        assert_eq!(picks[1].0.provider, "openrouter");
    }

    #[tokio::test]
    async fn complete_with_falls_back_within_requirements() {
        let mut fabric = InferenceFabric::new();
        fabric.register_provider(Box::new(ScriptedProvider {
            name: "openrouter",
            fail: true,
        }));
        fabric.register_provider(Box::new(ScriptedProvider {
            name: "opengateway",
            fail: false,
        }));
        fabric.register_model(mi(
            "openrouter/free",
            "openrouter",
            2000,
            &[ModelCapability::Tools],
        ));
        fabric.register_model(mi(
            "opengateway/free",
            "opengateway",
            2100,
            &[ModelCapability::Tools],
        ));

        // Both candidates meet the requirements (free budget + tools preferred);
        // the higher-ranked one (openrouter) fails with 429 → opengateway serves.
        let req = RequestRequirements::default();
        let text = fabric
            .complete_with(&req, "hi")
            .await
            .expect("fallback serves");
        assert_eq!(text, "ok");

        let d = fabric.last_decision().expect("decision recorded");
        assert_eq!(d.candidates.len(), 2);
        assert_eq!(d.failures.len(), 1);
        assert_eq!(d.failures[0].provider, "openrouter");
        assert_eq!(
            d.served,
            Some(("opengateway".to_string(), "opengateway/free".to_string()))
        );
    }

    #[tokio::test]
    async fn complete_with_errors_when_no_eligible_candidates() {
        // Only a paid provider is registered; the default free budget excludes it.
        let mut fabric = InferenceFabric::new();
        fabric.register_provider(Box::new(ScriptedProvider {
            name: "groq",
            fail: false,
        }));
        fabric.register_model(mi("llama-3.3-70b", "groq", 200, &[ModelCapability::Tools]));

        let err = fabric
            .complete_with(&RequestRequirements::default(), "hi")
            .await
            .expect_err("no free candidate → honest error");
        assert!(
            err.contains("No eligible providers"),
            "unexpected error: {err}"
        );
    }

    /// The fabric is the model client for the Investigation Engine's LLM
    /// strategy: the strategy prompt routes through free-first requirements
    /// routing, and the model's JSON answer becomes the plan. The fabric is
    /// shared behind an `Arc` (the natural daemon ownership) via the blanket
    /// `LlmStrategyClient for Arc<T>` passthrough, and the mock answers with a
    /// channel the heuristic could NOT produce for this goal (memory, not
    /// git) — so the assertion structurally discriminates the LLM path from
    /// a silent heuristic fallback.
    #[tokio::test]
    async fn fabric_serves_the_llm_strategy_end_to_end() {
        use friday_investigation::{InvestigationChannel, InvestigationEngine};

        struct JsonProvider;
        #[async_trait]
        impl Provider for JsonProvider {
            fn name(&self) -> &str {
                "openrouter"
            }
            async fn complete(
                &self,
                _model_id: &str,
                _prompt: &str,
                _require_json: bool,
            ) -> Result<String, String> {
                Ok(r#"{"requirements":[{"channel":"memory","rationale":"recall prior work","confidence":0.8}]}"#
                    .to_string())
            }
            async fn check_health(&self) -> bool {
                true
            }
        }

        let mut fabric = InferenceFabric::new();
        fabric.register_provider(Box::new(JsonProvider));
        fabric.register_model(mi(
            "openrouter/free",
            "openrouter",
            2000,
            &[ModelCapability::Tools],
        ));

        let engine = InvestigationEngine::llm(Box::new(Arc::new(fabric)));
        let plan = engine.investigate("Show me the repo status").await;
        // The heuristic would return git for this goal — memory proves the
        // model's answer (not the fallback) drove the plan.
        assert_eq!(plan.channels(), vec![InvestigationChannel::Memory]);
        assert_eq!(plan.requirements[0].confidence, 0.8);
    }

    /// Same end-to-end, but the fabric fails to route (no eligible candidate):
    /// the strategy's heuristic fallback still produces a plan — a failed
    /// model is never a failed investigation.
    #[tokio::test]
    async fn llm_strategy_falls_back_when_the_fabric_cannot_route() {
        use friday_investigation::{InvestigationChannel, InvestigationEngine};

        // Only a paid provider is registered; the default free budget excludes it.
        let mut fabric = InferenceFabric::new();
        fabric.register_provider(Box::new(ScriptedProvider {
            name: "groq",
            fail: true,
        }));
        fabric.register_model(mi("llama-3.3-70b", "groq", 200, &[ModelCapability::Tools]));

        let engine = InvestigationEngine::llm(Box::new(fabric));
        let plan = engine
            .investigate("Search the web for the latest Rust async docs")
            .await;
        assert!(
            plan.channels().contains(&InvestigationChannel::Web),
            "heuristic fallback: {plan:?}"
        );
    }

    #[test]
    fn from_capabilities_raises_privacy_for_local_preference() {
        use crate::providers::ModelCapability as C;
        let req = RequestRequirements::from_capabilities(&[C::Local, C::Vision]);
        assert_eq!(req.privacy, Preference::Preferred);
        assert_eq!(req.vision, Requirement::Preferred);
        assert_eq!(req.budget, Budget::Free);
    }
}

// =============================================================================
// M5.2 — Provider Capability Layer (ProviderV2)
//
// The provider stops being a bare `complete()` method and becomes a *capability*
// description. The planner asks `provider.capabilities().tool_calling`, never
// `provider.supports_tools()`. This is extensible: new abilities are data (a bool
// in `ProviderCapabilities`), not new trait methods.
//
// V1 (`Provider`) is intentionally KEPT. `ProviderV2Adapter` wraps any V1 provider
// so it instantly gains the V2 surface with conservative, inferred capabilities.
// Native V2 providers (e.g. `OmniRouteProviderV2`) implement the full surface
// (streaming, embeddings, dynamic model discovery). V1 is removed only once every
// provider is migrated and green.
// =============================================================================

/// What a provider is able to do. Data, not methods — easy to serialize, compare,
/// and feed into routing + qualification decisions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ProviderCapabilities {
    pub completion: bool,
    pub streaming: bool,
    pub tool_calling: bool,
    pub vision: bool,
    pub embeddings: bool,
    pub json_mode: bool,
    pub long_context: bool,
    pub local: bool,
    pub session_affinity: bool,
    /// Provider exposes a live model catalog (e.g. OpenAI `/v1/models`).
    pub dynamic_models: bool,
}

/// Stable, human-readable identity of a provider instance.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderMetadata {
    pub id: String,
    pub display_name: String,
    pub version: String,
    pub endpoint: String,
    pub vendor: String,
    /// Higher is preferred when several providers are viable (Phase 4).
    pub priority: u32,
    /// "free" | "free-tier" | "paid" | "local".
    pub cost_model: String,
    /// Execution location of this provider instance. Set by the registering side
    /// (registration knows the endpoint); consumers read it — never name-match.
    #[serde(default)]
    pub local: bool,
}

/// A model discovered from a provider, richer than a bare id. Populates the
/// registry so routing can pick by capability, not by string matching.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelDescriptor {
    pub id: String,
    pub context: usize,
    pub supports_tools: bool,
    pub supports_stream: bool,
    pub supports_json: bool,
    pub vision: bool,
    pub pricing: Option<String>,
}

/// One incremental piece of a streamed completion. `Stream<CompletionChunk>` is the
/// contract from day one — never `String` — so future UI can render tokens live.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompletionChunk {
    pub delta: String,
    pub finish_reason: Option<String>,
}

/// Result of an embeddings request. The Memory subsystem consumes vectors without
/// knowing which provider produced them.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmbeddingResult {
    pub model: String,
    pub embeddings: Vec<Vec<f32>>,
}

/// Capability-driven provider interface (M5.2).
#[async_trait]
pub trait ProviderV2: Send + Sync {
    fn metadata(&self) -> ProviderMetadata;
    fn capabilities(&self) -> ProviderCapabilities;
    async fn complete(
        &self,
        model_id: &str,
        prompt: &str,
        require_json: bool,
    ) -> Result<String, String>;
    async fn stream(
        &self,
        model_id: &str,
        prompt: &str,
        require_json: bool,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<CompletionChunk, String>> + Send>>, String>;
    async fn list_models(&self) -> Result<Vec<ModelDescriptor>, String>;
    async fn embeddings(&self, model_id: &str, texts: &[String])
        -> Result<EmbeddingResult, String>;
}

/// Parse one SSE event block (`...data: {...}\n\n`) into a completion chunk.
/// Returns `None` for events that carry no usable delta (e.g. empty role deltas),
/// and surfaces `[DONE]` as a chunk with `finish_reason = "[DONE]"`.
pub fn parse_sse_event(event: &str) -> Option<CompletionChunk> {
    let mut delta = String::new();
    let mut finish: Option<String> = None;
    for line in event.lines() {
        let line = line.trim_start_matches("data:").trim();
        if line.is_empty() {
            continue;
        }
        if line == "[DONE]" {
            return Some(CompletionChunk {
                delta: String::new(),
                finish_reason: Some("[DONE]".to_string()),
            });
        }
        if let Ok(json) = serde_json::from_str::<serde_json::Value>(line) {
            if let Some(d) = json["choices"][0]["delta"]["content"].as_str() {
                delta.push_str(d);
            }
            if let Some(f) = json["choices"][0]["finish_reason"].as_str() {
                finish = Some(f.to_string());
            }
        }
    }
    if delta.is_empty() && finish.is_none() {
        return None;
    }
    Some(CompletionChunk {
        delta,
        finish_reason: finish,
    })
}

/// Wraps a V1 `Provider` into the V2 surface. Capabilities are inferred
/// conservatively: completion/json/long-context are assumed; streaming, embeddings,
/// tool-calling and dynamic model listing are unavailable (the V1 trait cannot do
/// them). This lets every existing provider become a V2 immediately.
pub struct ProviderV2Adapter {
    pub inner: Box<dyn Provider>,
    pub metadata: ProviderMetadata,
    pub capabilities: ProviderCapabilities,
}

impl ProviderV2Adapter {
    pub fn from_provider(inner: Box<dyn Provider>, metadata: ProviderMetadata) -> Self {
        let id = metadata.id.clone();
        let capabilities = ProviderCapabilities {
            completion: true,
            streaming: false,
            tool_calling: false,
            vision: false,
            embeddings: false,
            json_mode: true,
            long_context: true,
            local: metadata.local,
            session_affinity: id == "omniroute",
            dynamic_models: false,
        };
        Self {
            inner,
            metadata,
            capabilities,
        }
    }
}

#[async_trait]
impl ProviderV2 for ProviderV2Adapter {
    fn metadata(&self) -> ProviderMetadata {
        self.metadata.clone()
    }

    fn capabilities(&self) -> ProviderCapabilities {
        self.capabilities
    }

    async fn complete(
        &self,
        model_id: &str,
        prompt: &str,
        require_json: bool,
    ) -> Result<String, String> {
        self.inner.complete(model_id, prompt, require_json).await
    }

    async fn stream(
        &self,
        _model_id: &str,
        _prompt: &str,
        _require_json: bool,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<CompletionChunk, String>> + Send>>, String> {
        Err(format!(
            "provider {} does not support streaming (V1 adapter)",
            self.metadata.id
        ))
    }

    async fn list_models(&self) -> Result<Vec<ModelDescriptor>, String> {
        Err(format!(
            "provider {} does not support dynamic model listing (V1 adapter)",
            self.metadata.id
        ))
    }

    async fn embeddings(
        &self,
        _model_id: &str,
        _texts: &[String],
    ) -> Result<EmbeddingResult, String> {
        Err(format!(
            "provider {} does not support embeddings (V1 adapter)",
            self.metadata.id
        ))
    }
}

/// Native V2 provider for OmniRoute — a self-hosted, OpenAI-compatible multi-provider
/// gateway that supports streaming, embeddings, tool-calling, vision, and a live
/// model catalog. This is the reference V2 implementation.
pub struct OmniRouteProviderV2 {
    inner: OmniRouteProvider,
    session_id: Option<String>,
    metadata: ProviderMetadata,
}

impl OmniRouteProviderV2 {
    pub fn new(base_url: String, api_key: String, session_id: Option<String>) -> Self {
        let metadata = ProviderMetadata {
            id: "omniroute".to_string(),
            display_name: "OmniRoute".to_string(),
            version: "v2".to_string(),
            endpoint: base_url.clone(),
            vendor: "omniroute".to_string(),
            priority: 100,
            cost_model: "free-tier".to_string(),
            local: false,
        };
        Self {
            inner: OmniRouteProvider::new(base_url, api_key),
            session_id,
            metadata,
        }
    }

    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities {
            completion: true,
            streaming: true,
            tool_calling: true,
            vision: true,
            embeddings: true,
            json_mode: true,
            long_context: true,
            local: false,
            session_affinity: true,
            dynamic_models: true,
        }
    }
}

#[async_trait]
impl ProviderV2 for OmniRouteProviderV2 {
    fn metadata(&self) -> ProviderMetadata {
        self.metadata.clone()
    }

    fn capabilities(&self) -> ProviderCapabilities {
        self.capabilities()
    }

    async fn complete(
        &self,
        model_id: &str,
        prompt: &str,
        require_json: bool,
    ) -> Result<String, String> {
        self.inner.complete(model_id, prompt, require_json).await
    }

    async fn stream(
        &self,
        model_id: &str,
        prompt: &str,
        _require_json: bool,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<CompletionChunk, String>> + Send>>, String> {
        let client = reqwest::Client::new();
        let url = format!(
            "{}/chat/completions",
            self.inner.base_url.trim_end_matches('/')
        );

        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert(
            reqwest::header::AUTHORIZATION,
            reqwest::header::HeaderValue::from_str(&format!("Bearer {}", self.inner.api_key))
                .map_err(|e| e.to_string())?,
        );
        headers.insert(
            reqwest::header::CONTENT_TYPE,
            reqwest::header::HeaderValue::from_static("application/json"),
        );
        if let Some(sid) = &self.session_id {
            if let Ok(v) = reqwest::header::HeaderValue::from_str(sid) {
                headers.insert(reqwest::header::HeaderName::from_static("x-session-id"), v);
            }
        }

        let body = serde_json::json!({
            "model": model_id,
            "messages": [{ "role": "user", "content": prompt }],
            "stream": true,
        });

        let resp = client
            .post(&url)
            .headers(headers)
            .json(&body)
            .send()
            .await
            .map_err(|e| format!("stream request failed: {}", e))?;

        if !resp.status().is_success() {
            return Err(format!(
                "Provider omniroute returned error status {}",
                resp.status()
            ));
        }

        let byte_stream = resp.bytes_stream();
        let s = unfold(
            (byte_stream, String::new(), VecDeque::new()),
            |state| async move {
                let (mut byte_stream, mut buf, mut queue) = state;
                loop {
                    if let Some(ck) = queue.pop_front() {
                        return Some((Ok(ck), (byte_stream, buf, queue)));
                    }
                    match byte_stream.next().await {
                        Some(Ok(bytes)) => {
                            buf.push_str(&String::from_utf8_lossy(&bytes));
                            while let Some(idx) = buf.find("\n\n") {
                                let event = buf[..idx].to_string();
                                buf.drain(..idx + 2);
                                if let Some(ck) = parse_sse_event(&event) {
                                    let is_done = ck.finish_reason.as_deref() == Some("[DONE]");
                                    if !ck.delta.is_empty() {
                                        queue.push_back(CompletionChunk {
                                            delta: ck.delta,
                                            finish_reason: ck.finish_reason.clone(),
                                        });
                                    }
                                    if is_done {
                                        return queue
                                            .pop_front()
                                            .map(|ck| (Ok(ck), (byte_stream, buf, queue)));
                                    }
                                }
                            }
                        }
                        Some(Err(e)) => {
                            return Some((
                                Err(format!("stream error: {}", e)),
                                (byte_stream, String::new(), VecDeque::new()),
                            ));
                        }
                        None => {
                            if !buf.trim().is_empty() {
                                if let Some(ck) = parse_sse_event(&buf) {
                                    if !ck.delta.is_empty() {
                                        queue.push_back(ck);
                                    }
                                }
                                buf.clear();
                                continue;
                            }
                            return None;
                        }
                    }
                }
            },
        );
        Ok(Box::pin(s))
    }

    async fn list_models(&self) -> Result<Vec<ModelDescriptor>, String> {
        let client = reqwest::Client::new();
        let url = format!("{}/models", self.inner.base_url.trim_end_matches('/'));
        let res = client
            .get(&url)
            .header(
                reqwest::header::AUTHORIZATION,
                format!("Bearer {}", self.inner.api_key),
            )
            .send()
            .await
            .map_err(|e| e.to_string())?;
        if !res.status().is_success() {
            return Err(format!("list_models failed: {}", res.status()));
        }
        let json: serde_json::Value = res.json().await.map_err(|e| e.to_string())?;
        let data = json["data"]
            .as_array()
            .ok_or_else(|| "unexpected /models response shape".to_string())?;
        let mut out = Vec::new();
        for m in data {
            let id = match m["id"].as_str() {
                Some(i) => i.to_string(),
                None => continue,
            };
            let context = m["context_length"].as_u64().unwrap_or(0) as usize;
            out.push(ModelDescriptor {
                id,
                context,
                supports_tools: true,
                supports_stream: true,
                supports_json: true,
                vision: false,
                pricing: m["pricing"].as_str().map(|s| s.to_string()),
            });
        }
        Ok(out)
    }

    async fn embeddings(
        &self,
        model_id: &str,
        texts: &[String],
    ) -> Result<EmbeddingResult, String> {
        let client = reqwest::Client::new();
        let url = format!("{}/embeddings", self.inner.base_url.trim_end_matches('/'));

        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert(
            reqwest::header::AUTHORIZATION,
            reqwest::header::HeaderValue::from_str(&format!("Bearer {}", self.inner.api_key))
                .map_err(|e| e.to_string())?,
        );
        headers.insert(
            reqwest::header::CONTENT_TYPE,
            reqwest::header::HeaderValue::from_static("application/json"),
        );

        let body = serde_json::json!({ "model": model_id, "input": texts });
        let res = client
            .post(&url)
            .headers(headers)
            .json(&body)
            .send()
            .await
            .map_err(|e| e.to_string())?;
        if !res.status().is_success() {
            return Err(format!("embeddings failed: {}", res.status()));
        }
        let json: serde_json::Value = res.json().await.map_err(|e| e.to_string())?;
        let data = json["data"]
            .as_array()
            .ok_or_else(|| "unexpected embeddings response shape".to_string())?;
        let mut embeddings = Vec::new();
        for d in data {
            let vec = d["embedding"]
                .as_array()
                .map(|a| {
                    a.iter()
                        .filter_map(|x| x.as_f64().map(|f| f as f32))
                        .collect::<Vec<f32>>()
                })
                .unwrap_or_default();
            embeddings.push(vec);
        }
        Ok(EmbeddingResult {
            model: model_id.to_string(),
            embeddings,
        })
    }
}

#[cfg(test)]
mod v2_tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::OnceLock;

    fn v2_metadata(id: &str) -> ProviderMetadata {
        ProviderMetadata {
            id: id.to_string(),
            display_name: id.to_string(),
            version: "v1".to_string(),
            endpoint: "http://localhost/v1".to_string(),
            vendor: id.to_string(),
            priority: 50,
            cost_model: "free".to_string(),
            local: false,
        }
    }

    #[test]
    fn omniroute_v2_declares_full_capabilities() {
        let p =
            OmniRouteProviderV2::new("http://localhost:20128/v1".to_string(), String::new(), None);
        let c = p.capabilities();
        assert!(c.completion && c.streaming && c.embeddings && c.tool_calling);
        assert!(c.dynamic_models && c.session_affinity && c.json_mode);
        assert_eq!(p.metadata().id, "omniroute");
        assert!(
            !p.metadata().local,
            "omniroute is a gateway, not a local engine"
        );
    }

    #[test]
    fn v2_adapter_infers_conservative_capabilities() {
        let v1 = OmniRouteProvider::new("http://localhost:20128/v1".to_string(), String::new());
        let adapter = ProviderV2Adapter::from_provider(Box::new(v1), v2_metadata("omniroute"));
        let c = adapter.capabilities();
        assert!(c.completion && c.json_mode && c.long_context);
        assert!(!c.streaming && !c.embeddings && !c.dynamic_models);
        assert!(c.session_affinity); // omniroute keeps session affinity even via V1
    }

    #[test]
    fn v2_adapter_derives_local_from_metadata() {
        // Locality comes from ProviderMetadata, never from the provider id.
        let mut metadata = v2_metadata("llamacpp");
        metadata.local = true;
        let v1 = OpenAiProvider {
            name: "llamacpp".to_string(),
            base_url: "http://localhost:8080/v1".to_string(),
            api_key: "unused".to_string(),
        };
        let adapter = ProviderV2Adapter::from_provider(Box::new(v1), metadata);
        assert!(adapter.capabilities().local);
        assert!(!adapter.capabilities().session_affinity);

        // A remote id with default metadata is not local.
        let adapter = ProviderV2Adapter::from_provider(
            Box::new(OpenAiProvider {
                name: "ollama".to_string(),
                base_url: "http://localhost:11434/v1".to_string(),
                api_key: "ollama".to_string(),
            }),
            v2_metadata("ollama"),
        );
        assert!(!adapter.capabilities().local);
    }

    #[test]
    fn parse_sse_event_extracts_delta_and_finish() {
        let ev =
            "data: {\"choices\":[{\"delta\":{\"content\":\"Hel\"},\"finish_reason\":null}]}\n\n";
        let ck = parse_sse_event(ev).expect("should parse");
        assert_eq!(ck.delta, "Hel");
        assert!(ck.finish_reason.is_none());

        let done = "data: [DONE]\n\n";
        let ck2 = parse_sse_event(done).expect("should parse done");
        assert_eq!(ck2.finish_reason.as_deref(), Some("[DONE]"));

        let empty = "data: {\"choices\":[{\"delta\":{\"role\":\"assistant\"}}]}\n\n";
        assert!(parse_sse_event(empty).is_none());
    }

    #[tokio::test]
    async fn omniroute_v2_stream_is_graceful_when_offline() {
        let p =
            OmniRouteProviderV2::new("http://localhost:19999/v1".to_string(), String::new(), None);
        // send() fails synchronously inside stream(), so it returns Err (no panic).
        assert!(p.stream("auto", "hi", false).await.is_err());
    }

    #[tokio::test]
    async fn omniroute_v2_list_models_graceful_when_offline() {
        let p =
            OmniRouteProviderV2::new("http://localhost:19999/v1".to_string(), String::new(), None);
        assert!(p.list_models().await.is_err());
    }

    #[tokio::test]
    async fn omniroute_v2_embeddings_graceful_when_offline() {
        let p =
            OmniRouteProviderV2::new("http://localhost:19999/v1".to_string(), String::new(), None);
        assert!(p.embeddings("auto", &["x".to_string()]).await.is_err());
    }

    struct CountingProvider {
        name: String,
        calls: Arc<AtomicUsize>,
    }

    impl CountingProvider {
        fn new(name: impl Into<String>, calls: Arc<AtomicUsize>) -> Self {
            Self {
                name: name.into(),
                calls,
            }
        }
    }

    #[async_trait]
    impl Provider for CountingProvider {
        fn name(&self) -> &str {
            &self.name
        }

        async fn complete(
            &self,
            _model_id: &str,
            _prompt: &str,
            _require_json: bool,
        ) -> Result<String, String> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Ok("ok".to_string())
        }

        async fn check_health(&self) -> bool {
            true
        }
    }

    // ── Egress policy (#12): the outbound seam shared by every dispatch ──

    fn fabric_with_egress(
        policy_for: Option<(&str, crate::egress::EgressMode)>,
    ) -> InferenceFabric {
        use crate::egress::EgressGate;
        use crate::secrets::SecretsVault;
        let vault = std::sync::Arc::new(SecretsVault::new());
        vault.store("provider", "openrouter", "sk-token-abcdef");
        let mut gate = EgressGate::new(std::sync::Arc::clone(&vault));
        if let Some((provider, mode)) = policy_for {
            gate = gate.with_provider_policy(provider, mode);
        }
        InferenceFabric::new().with_egress(std::sync::Arc::new(gate))
    }

    #[test]
    fn egress_off_by_default_passes_prompts_through_unchanged() {
        let fabric = InferenceFabric::new();
        assert!(
            fabric.egress.is_none(),
            "egress is OFF unless explicitly attached (no behavior change)"
        );
        assert_eq!(
            fabric
                .egress_process("openrouter", "summarize sk-token-abcdef")
                .unwrap(),
            "summarize sk-token-abcdef"
        );
    }

    #[test]
    fn egress_block_stops_a_secret_payload_before_dispatch() {
        let fabric = fabric_with_egress(None); // default policy = Block
        let err = fabric
            .egress_process("openrouter", "please use sk-token-abcdef here")
            .unwrap_err();
        assert!(err.contains("egress policy blocked"), "{err}");
        assert!(err.contains("provider/openrouter"), "{err}");
        assert!(
            !err.contains("sk-token-abcdef"),
            "the error never carries the value: {err}"
        );
    }

    #[test]
    fn egress_redact_sanitizes_the_outbound_prompt() {
        use crate::egress::EgressMode;
        let fabric = fabric_with_egress(Some(("openrouter", EgressMode::Redact)));
        let sent = fabric
            .egress_process("openrouter", "use sk-token-abcdef in your answer")
            .unwrap();
        assert!(!sent.contains("sk-token-abcdef"), "{sent}");
        assert!(sent.contains("[REDACTED:provider/openrouter]"), "{sent}");
    }

    #[test]
    fn egress_policy_is_per_provider_at_the_fabric() {
        use crate::egress::EgressMode;
        let fabric = fabric_with_egress(Some(("openrouter", EgressMode::Redact)));
        // openrouter: redacted; groq (unlisted): default Block.
        assert!(fabric
            .egress_process("openrouter", "has sk-token-abcdef")
            .unwrap()
            .contains("[REDACTED:provider/openrouter]"));
        assert!(fabric
            .egress_process("groq", "has sk-token-abcdef")
            .is_err());
    }

    #[tokio::test]
    async fn default_from_env_attaches_security_context_and_blocks_secret_payloads() {
        let env_lock = static_env_lock().lock().expect("env lock poisoned");
        let secret = format!("sk-test-{}", uuid::Uuid::new_v4().simple());
        std::env::set_var("OPENROUTER_API_KEY", &secret);

        let mut fabric = InferenceFabric::default_from_env();
        std::env::remove_var("OPENROUTER_API_KEY");
        drop(env_lock);

        assert!(
            fabric.egress.is_some(),
            "production boot must attach the existing egress gate when secrets are seeded"
        );

        let calls = Arc::new(AtomicUsize::new(0));
        let provider_name = "mock-egress";
        fabric.register_provider(Box::new(CountingProvider::new(
            provider_name,
            Arc::clone(&calls),
        )));
        fabric.register_model(ModelInfo {
            id: "mock-egress/auto".to_string(),
            family: "mock-egress".to_string(),
            provider: provider_name.to_string(),
            latency_ms: 1,
            reliability: 1.0,
            capabilities: vec![ModelCapability::Json, ModelCapability::Tools],
            max_context: 1024,
            tier: Some(CapabilityTier::Auto),
        });

        let clean = fabric
            .complete_tier(
                provider_name,
                CapabilityTier::Auto,
                "summarize the plan",
                false,
            )
            .await
            .expect("clean request must pass");
        assert_eq!(clean, "ok");
        assert_eq!(
            calls.load(Ordering::SeqCst),
            1,
            "provider should be called once"
        );

        let blocked = fabric
            .complete_tier(
                provider_name,
                CapabilityTier::Auto,
                &format!("please use {secret} in the response"),
                false,
            )
            .await
            .expect_err("secret payload must be blocked before provider dispatch");
        assert!(blocked.contains("egress policy blocked"), "{blocked}");
        assert!(!blocked.contains(&secret), "{blocked}");
        assert_eq!(
            calls.load(Ordering::SeqCst),
            1,
            "blocked request must never reach the provider"
        );

        let decision = fabric.last_decision().expect("routing decision recorded");
        let audit = serde_json::to_string(&decision).unwrap();
        assert!(!audit.contains(&secret), "{audit}");
        assert!(
            decision
                .failures
                .iter()
                .any(|f| f.error.contains("egress policy blocked") && !f.error.contains(&secret)),
            "decision must record the block without leaking the secret: {decision:?}"
        );
    }

    fn static_env_lock() -> &'static std::sync::Mutex<()> {
        static LOCK: OnceLock<std::sync::Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| std::sync::Mutex::new(()))
    }
}
