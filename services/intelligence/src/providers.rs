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

        fabric
    }
}

#[async_trait]
impl ModelClient for InferenceFabric {
    async fn complete(&self, prompt: &str) -> Result<String, String> {
        let candidates = self.select_candidates(self.default_mode);
        if candidates.is_empty() {
            return Err("No active providers configured for this routing mode.".to_string());
        }

        let mut last_error = String::new();
        for (model, provider) in candidates {
            match provider.complete(&model.id, prompt, false).await {
                Ok(response) => {
                    let mut guard = self.health_scores.lock().unwrap();
                    if let Some(score) = guard.get_mut(provider.name()) {
                        *score = (*score + 0.1).min(1.0);
                    }
                    let mut cb_guard = self.circuits.lock().unwrap();
                    if let Some(cb) = cb_guard.get_mut(provider.name()) {
                        cb.record_success();
                    }
                    return Ok(response);
                }
                Err(err) => {
                    tracing::warn!("Provider {} call failed: {}. Trip tracking...", provider.name(), err);
                    let mut guard = self.health_scores.lock().unwrap();
                    if let Some(score) = guard.get_mut(provider.name()) {
                        *score = (*score - 0.2).max(0.0);
                    }
                    let mut cb_guard = self.circuits.lock().unwrap();
                    if let Some(cb) = cb_guard.get_mut(provider.name()) {
                        cb.record_failure();
                    }
                    last_error = err;
                }
            }
        }

        Err(format!("All candidates failed. Last error: {}", last_error))
    }
}
