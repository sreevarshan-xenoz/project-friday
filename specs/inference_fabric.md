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

### Routing audit trail (`RoutingDecision`)

Every routed completion records why routing did what it did, so qualification
evidence is auditable (which vendor model answered, and why it wasn't the
chosen one):

**FROZEN (v1) — `RoutingDecision` is immutable.** The shape is versioned with a
`timestamp`; changing it requires a new ADR and a version bump, not an ordinary
implementation commit. Mirrors the `DecisionRecord` rule: the audit trail is
history and is never edited — everything downstream (analytics, future provider
reputation) is a derived view over it.

| Field | Type | Meaning |
|---|---|---|
| `requested_tier` | `Option<CapabilityTier>` | Some for tier routing; None = mode-based |
| `routing_mode` | `String` | Fast/Quality/Free/Local/Hybrid |
| `candidates` | `Vec<CandidateConsideration>` | in routing order, with scores |
| `chosen` | `Option<(String, String)>` | highest-ranked candidate (provider, model) |
| `served` | `Option<(String, String)>` | what actually answered, if anything |
| `failures` | `Vec<CandidateFailure>` | per-candidate errors, in attempt order; empty when served == chosen |
| `timestamp` | `String` (RFC 3339) | when the decision was recorded |
| `version` | `u32` | contract version; currently `1` |

```rust
pub struct RoutingDecision {
    pub requested_tier: Option<CapabilityTier>,
    pub routing_mode: String,
    pub candidates: Vec<CandidateConsideration>,
    pub chosen: Option<(String, String)>,
    pub served: Option<(String, String)>,
    pub failures: Vec<CandidateFailure>,
    pub timestamp: String, // RFC 3339
    pub version: u32,
}

pub struct CandidateConsideration {
    pub provider: String,
    pub model: String,
    pub reliability: f64, // provider reliability (primary sort key)
    pub latency_ms: u64,  // model latency (secondary sort key)
}

pub struct CandidateFailure {
    pub provider: String,
    pub model: String,
    pub error: String, // e.g. quota 429, reset
}
```

`InferenceFabric::last_decision()` returns the most recent trail;
`last_served()` derives the `(provider, model)` from it for backwards
compatibility. Set on both success and failure (the failure trail keeps every
candidate error so a run is debuggable). Qualification evidence persists it as
`routing_decision` alongside `served_model`; the human-readable `change_reason`
is derived from `failures` at serialization time, never stored.

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
