use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Instant;

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

/// Represents metadata for a model hosted by a specific provider.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelInfo {
    pub id: String,          // e.g. "google/gemini-2.5-flash"
    pub family: String,      // e.g. "gemini-2.5-flash"
    pub provider: String,    // e.g. "openrouter"
    pub latency_ms: u64,
    pub reliability: f64,
    pub capabilities: Vec<ModelCapability>,
    pub max_context: usize,
}

/// Pluggable adapter for communicating with an external API host.
#[async_trait]
pub trait Provider: Send + Sync {
    fn name(&self) -> &str;
    async fn complete(&self, model_id: &str, prompt: &str, require_json: bool) -> Result<String, String>;
    async fn check_health(&self) -> bool;
}

/// Circuit Breaker States
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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

    async fn complete(&self, model_id: &str, prompt: &str, _require_json: bool) -> Result<String, String> {
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
                reqwest::header::HeaderValue::from_static("https://github.com/project-friday/friday"),
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

        let res = client.post(&url)
            .headers(headers)
            .json(&body)
            .send()
            .await
            .map_err(|e| format!("HTTP request failed: {}", e))?;

        if !res.status().is_success() {
            let status = res.status();
            let err_body = res.text().await.unwrap_or_default();
            return Err(format!("Provider {} returned error status {}: {}", self.name, status, err_body));
        }

        let json: serde_json::Value = res.json()
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
        let res = client.get(&url)
            .header(reqwest::header::AUTHORIZATION, format!("Bearer {}", self.api_key))
            .send()
            .await;
        res.is_ok() && res.unwrap().status().is_success()
    }
}

/// Telemetry metrics for each request made through the Inference Fabric.
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

/// Representation of health state persisted locally.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersistedProviderHealth {
    pub name: String,
    pub reliability: f64,
    pub consecutive_failures: usize,
    pub circuit_state: String, // "Closed", "Open", "HalfOpen"
}

/// Coordinates model selection, state tracking, and error recovery.
pub struct InferenceFabric {
    pub providers: HashMap<String, Box<dyn Provider>>,
    pub models: Vec<ModelInfo>,
    pub health_scores: Arc<Mutex<HashMap<String, f64>>>,
    pub circuits: Arc<Mutex<HashMap<String, CircuitBreaker>>>,
    pub default_mode: RoutingMode,
    pub required_capabilities: Vec<ModelCapability>,
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
        }
    }

    pub fn register_provider(&mut self, provider: Box<dyn Provider>) {
        self.health_scores.lock().unwrap().insert(provider.name().to_string(), 1.0);
        self.circuits.lock().unwrap().insert(provider.name().to_string(), CircuitBreaker::new(30)); // 30s cooldown
        self.providers.insert(provider.name().to_string(), provider);
    }

    pub fn register_model(&mut self, model: ModelInfo) {
        self.models.push(model);
    }

    /// Selects and ranks provider-model candidates based on routing policy and capabilities.
    pub fn select_candidates(&self, mode: RoutingMode) -> Vec<(ModelInfo, &dyn Provider)> {
        let mut candidates = Vec::new();
        let mut circuit_guard = self.circuits.lock().unwrap();

        for model in &self.models {
            // Check capability match
            let has_caps = self.required_capabilities.iter().all(|cap| model.capabilities.contains(cap));
            if !has_caps {
                continue;
            }

            // Check circuit breaker status
            if let Some(cb) = circuit_guard.get_mut(&model.provider) {
                if !cb.allow_request() {
                    continue;
                }
            }

            let matches = match mode {
                RoutingMode::Fast => model.provider == "groq" || model.provider == "cerebras",
                RoutingMode::Quality => model.provider == "gemini" || model.provider == "groq" || model.provider == "cerebras",
                RoutingMode::Free => model.provider == "openrouter",
                RoutingMode::Local => model.provider == "ollama",
                RoutingMode::Hybrid => model.provider == "ollama" || model.provider == "openrouter",
            };

            if matches {
                if let Some(prov) = self.providers.get(&model.provider) {
                    candidates.push((model.clone(), prov.as_ref()));
                }
            }
        }
        
        let scores = self.health_scores.clone();
        candidates.sort_by(|a, b| {
            let guard = scores.lock().unwrap();
            let rel_a = guard.get(a.1.name()).copied().unwrap_or(1.0);
            let rel_b = guard.get(b.1.name()).copied().unwrap_or(1.0);
            
            // Primary sort: reliability descending
            let rel_cmp = rel_b.partial_cmp(&rel_a).unwrap_or(std::cmp::Ordering::Equal);
            if rel_cmp != std::cmp::Ordering::Equal {
                return rel_cmp;
            }
            
            // Secondary sort: latency ascending
            a.0.latency_ms.cmp(&b.0.latency_ms)
        });
        
        candidates
    }

    /// Persists health scores and circuit states to a local file.
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

        let _ = std::fs::create_dir_all(".fabric");
        if let Ok(file) = std::fs::File::create(".fabric/provider_health.json") {
            let _ = serde_json::to_writer_pretty(file, &persisted);
        }
    }

    /// Loads health scores and circuit states from local file if exists.
    pub fn load_health(&self) {
        if let Ok(file) = std::fs::File::open(".fabric/provider_health.json") {
            if let Ok(persisted) = serde_json::from_reader::<_, Vec<PersistedProviderHealth>>(file) {
                let mut scores = self.health_scores.lock().unwrap();
                let mut circuits = self.circuits.lock().unwrap();
                for p in persisted {
                    scores.insert(p.name.clone(), p.reliability);
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
        if let Ok(key) = std::env::var("OPENROUTER_API_KEY") {
            fabric.register_provider(Box::new(OpenAiProvider {
                name: "openrouter".to_string(),
                base_url: "https://openrouter.ai/api/v1".to_string(),
                api_key: key,
            }));
            if !registry_loaded {
                fabric.register_model(ModelInfo {
                    id: "google/gemini-2.5-flash".to_string(),
                    family: "gemini-2.5-flash".to_string(),
                    provider: "openrouter".to_string(),
                    latency_ms: 800,
                    reliability: 1.0,
                    capabilities: vec![ModelCapability::Json, ModelCapability::Tools, ModelCapability::LongContext],
                    max_context: 1_000_000,
                });
                fabric.register_model(ModelInfo {
                    id: "meta-llama/llama-3.3-70b-instruct:free".to_string(),
                    family: "llama-3.3-70b".to_string(),
                    provider: "openrouter".to_string(),
                    latency_ms: 1200,
                    reliability: 1.0,
                    capabilities: vec![ModelCapability::Json, ModelCapability::Tools],
                    max_context: 131072,
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
                    id: "gemini-2.5-flash".to_string(),
                    family: "gemini-2.5-flash".to_string(),
                    provider: "gemini".to_string(),
                    latency_ms: 600,
                    reliability: 1.0,
                    capabilities: vec![ModelCapability::Json, ModelCapability::Tools, ModelCapability::LongContext],
                    max_context: 1_000_000,
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
                capabilities: vec![ModelCapability::Json, ModelCapability::Tools, ModelCapability::Local],
                max_context: 32768,
            });
        }

        fabric.load_health();
        fabric
    }
}

#[async_trait]
impl ModelClient for InferenceFabric {
    async fn complete(&self, prompt: &str) -> Result<String, String> {
        let request_id = uuid::Uuid::new_v4().to_string();
        tracing::info!("[Inference Request {}] Starting completion routing...", request_id);

        let candidates = self.select_candidates(self.default_mode);
        if candidates.is_empty() {
            let err_msg = "No active providers configured for this routing mode.".to_string();
            let metrics = InferenceMetrics {
                request_id: request_id.clone(),
                timestamp: chrono::Utc::now(),
                provider: "none".to_string(),
                model: "none".to_string(),
                latency_ms: 0,
                success: false,
                retry_count: 0,
                error: Some(err_msg.clone()),
            };
            self.log_metrics(&metrics);
            return Err(err_msg);
        }

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
            match provider.complete(&model.id, prompt, false).await {
                Ok(response) => {
                    let latency_ms = start.elapsed().as_millis() as u64;
                    tracing::info!(
                        "[Inference Request {}] Success on candidate {}/{} ({}ms)",
                        request_id,
                        provider.name(),
                        model.id,
                        latency_ms
                    );

                    // Update reliability score
                    {
                        let mut guard = self.health_scores.lock().unwrap();
                        if let Some(score) = guard.get_mut(provider.name()) {
                            *score = (*score + 0.1).min(1.0);
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
                        request_id: request_id.clone(),
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
                    {
                        let mut guard = self.health_scores.lock().unwrap();
                        if let Some(score) = guard.get_mut(provider.name()) {
                            *score = (*score - 0.2).max(0.0);
                        }
                    }

                    // Trip circuit failure
                    {
                        let mut cb_guard = self.circuits.lock().unwrap();
                        if let Some(cb) = cb_guard.get_mut(provider.name()) {
                            cb.record_failure();
                        }
                    }

                    self.persist_health();
                    last_error = err;
                }
            }
        }

        let latency_ms = start.elapsed().as_millis() as u64;
        let final_err = format!("All candidates failed. Last error: {}", last_error);
        let metrics = InferenceMetrics {
            request_id: request_id.clone(),
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
}
