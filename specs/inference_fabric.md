# Inference Fabric Specification (M5.8)

**Status:** 🚧 Planned / Designing. **Spec-first:** authoritative contract for FRIDAY's Inference Fabric.

The Inference Fabric abstracts LLM providers (e.g. Groq, OpenRouter, Gemini, Ollama, OpenGateway/gitlawb) and maps them to logical model families. The cognitive layer requests a model family under a specific routing policy; the Inference Fabric decides which provider to query and handles automatic rate-limit/JSON-validation failovers.

```
                    Cognitive Engine
                           │
                           ▼
                   [Inference Fabric]
                           │
       ┌───────────┬───────┴───┬───────────┐
       ▼           ▼           ▼           ▼
  OpenRouter      Groq     Gemini      Ollama
```

## Core Models

### `RoutingMode`
Governs how the fabric selects providers and models:
- `FAST`: Prioritizes ultra-low latency providers (e.g., Groq, Cerebras).
- `QUALITY`: Prioritizes high-end models (e.g., Gemini Pro, Mistral Large).
- `FREE`: Prioritizes providers and models requiring no payment cards (e.g., OpenRouter free tier).
- `LOCAL`: Enforces local-only execution (Ollama).
- `HYBRID`: Attempts local Ollama first, falling back to free cloud providers on failure.

### `ModelInfo`
Represents a model instance hosted by a specific provider. `tier` is the FRIDAY
capability descriptor (`CapabilityTier`) when the model has a stable logical role;
callers resolve by tier and never hard-code vendor IDs:
```rust
pub struct ModelInfo {
    pub id: String,            // e.g., "llama-3.3-70b-versatile" (vendor wire ID)
    pub family: String,        // e.g., "llama-3.3-70b" (logical name)
    pub provider: String,      // e.g., "groq"
    pub latency_ms: u64,
    pub reliability: f64,
    pub capabilities: Vec<ModelCapability>, // Json, Tools, Vision, Local, LongContext
    pub max_context: usize,
    pub tier: Option<CapabilityTier>,       // FRIDAY descriptor, e.g. ReasoningFree
}
```
`InferenceFabric::resolve_tier(provider, tier)` maps a FRIDAY descriptor to the
concrete model; `complete_tier(...)` completes against it. Vendor renames are a
provider-catalog change only.

## `Provider` Trait
Pluggable adapter for communicating with an external API host:
```rust
#[async_trait]
pub trait Provider: Send + Sync {
    fn name(&self) -> &str;
    async fn complete(&self, model_id: &str, prompt: &str, require_json: bool) -> Result<String, String>;
    async fn check_health(&self) -> bool;
}
```

## `InferenceFabric` API
Orchestrates model selection, state tracking, and error recovery:
```rust
pub struct InferenceFabric {
    providers: HashMap<String, Box<dyn Provider>>,
    models: Vec<ModelInfo>,
    health_scores: HashMap<String, f64>, // Maps provider to reliability rating
}

impl InferenceFabric {
    pub fn new() -> Self;

    /// Register an external provider adapter.
    pub fn register_provider(&mut self, provider: Box<dyn Provider>);

    /// Register a model configuration mapping to a provider.
    pub fn register_model(&mut self, model: ModelInfo);

    /// Execute a chat/completion prompt with automatic failover and JSON verification
    pub async fn complete(
        &self,
        prompt: &str,
        mode: RoutingMode,
        require_json: bool,
    ) -> Result<(String, ModelInfo), String>;
}
```

## Automatic Failover Execution Loop
When `InferenceFabric::complete` is called:
1. Filter registered models matching the target `RoutingMode` (e.g. if `RoutingMode::Free`, find models with $0.00 pricing).
2. Rank candidates by reliability/latency score.
3. Query the highest ranked model-provider candidate.
4. If the call fails (HTTP 429 rate limit, 503 timeout) OR if the result fails JSON validation (if `require_json` is true):
   - Temporarily degrade the provider's health score.
   - Failover immediately to the next candidate in the ranked list.
   - Retry up to 3 times across different candidates before throwing an error.
