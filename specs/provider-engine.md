# Provider Engine Specification

**Status:** 🚧 Spec-first — implement against this file.
**Depends on:** `friday-core` (event bus)
**Crate:** `services/provider` — crate `friday-provider`

## 1. Purpose

FRIDAY talks to multiple LLM providers: local models (Ollama, llama.cpp),
cloud APIs (OpenAI, Anthropic, Google), and future providers. The Provider
Engine abstracts this complexity. It:

- Discovers available providers and models
- Tracks capacity and rate limits
- Selects the best model for each task
- Handles failover and fallback
- Measures quality and cost
- Shields the rest of the system from provider-specific details

This is distinct from the Tool Selector, which chooses *which tools to use*.
The Provider Engine chooses *which model to think with*.

## 2. Separation from Tool Selector

| Component | Decides | Input | Output |
|-----------|---------|-------|--------|
| **Tool Selector** | Which tools to use | Goal + context | Tool list with args |
| **Provider Engine** | Which model to use | Task type + requirements | Provider + model + config |

The Tool Selector says: "Use cargo build, then cargo test."
The Provider Engine says: "Use local Ollama for planning, cloud GPT for code review."

They operate at different levels and should never be confused.

## 3a. Capability Tiers (FRIDAY concepts, not vendor IDs)

Provider configuration never hard-codes vendor model IDs at the call site. Every
model that has a stable FRIDAY purpose is assigned a **`CapabilityTier`**; the
provider maps tiers to concrete model IDs internally. If the vendor renames or
reprices a model, only the provider catalog changes — FRIDAY never notices.

Implemented in `services/intelligence` as `CapabilityTier` + `ModelInfo.tier`:

```text
Reasoning.Free      -> reasoning-free      -> e.g. mindai/macaron-v1-tall
Conversation.Fast   -> conversation-fast   -> e.g. nvidia/nemotron-3-ultra-550b-a55b:free
LongContext.Free    -> long-context-free   -> e.g. inclusionai/ling-3.0-flash:free
Vision.Free         -> vision-free         -> (none registered yet)
Auto                -> auto                -> provider smart routing
```

Contract:

1. `InferenceFabric::resolve_tier(provider, tier) -> Option<&ModelInfo>` resolves
   a FRIDAY descriptor to the concrete registered model.
2. `InferenceFabric::complete_tier(provider, tier, prompt, require_json)` completes
   against a tier, resolving the vendor ID internally.
3. Qualification targets tiers (`OPENGATEWAY_TIER=reasoning-free`), never raw IDs.
4. The planner (frozen) still asks for `Capability` sets; tiers are a provider-side
   resolution layer beneath it and never leak vendor IDs upward.
5. **Routing is auditable**: every routed completion records a `RoutingDecision`
   (`requested_tier`, `routing_mode`, candidates considered with reliability +
   latency scores, `chosen`, `served`, structured `failures`, `timestamp`, and
   `version`). Qualification evidence persists it as `routing_decision` so a
   report answers "which provider/model answered, and why it wasn't the chosen
   one" — quota 429 vs vendor outage vs transient network reset vs capability
   missing are distinguishable. See `specs/inference_fabric.md`.

   **FROZEN (v1) — `RoutingDecision` is immutable.** The shape is stamped
   versioned (v1) with a `timestamp`; changing the shape requires a new ADR and
   a version bump, not an ordinary implementation commit. Mirrors the
   `DecisionRecord` rule: the audit trail is history, so it is never edited —
   everything downstream (analytics, future provider reputation) is a derived
   view over it. The human-readable `change_reason` is derived at
   serialization, never stored.

## 3b. Local routing by capability, not provider name

**Implemented in `services/intelligence`.** Local routing never depends on the provider
*name*. `RoutingMode::Local` selects a model iff its `ModelInfo`
advertises `ModelCapability::Local`; `RoutingMode::Hybrid` adds that same
capability alongside the cloud providers. A provider named `"ollama"` whose model
does not carry the `Local` capability is **not** a local candidate, and a provider
named anything else that *does* carry it is. `ProviderV2Adapter` derives `local`
from `ProviderMetadata.local` — an execution-location flag set by the registering
side (registration knows the endpoint); the adapter never name-matches provider IDs.

### llama.cpp (llama-server)

Registered behind the same env-gated pattern as OmniRoute — the backend is local
to this machine and may not be running:

- `LLAMACPP_ENABLED=true|1` registers the provider (OpenAI-compatible `/v1`).
- `LLAMACPP_BASE_URL` defaults to `http://localhost:8080/v1` (llama-server default).
- `LLAMACPP_MODEL` labels the served GGUF; defaults to `gemma4:e4b`. The id is a
  label only — llama-server serves whatever `-m` loaded at startup.
- Default model capability set: `Json`, `Tools`, `Local`.

Qualification: `cargo run -p friday-provider-qual -- --provider llamacpp` targets
`RoutingMode::Local` against the default endpoint. Per the ADR-0005 one-variable
discipline, the default is **not** flipped until qualification evidence shows the
local engine beats Ollama for the target workload. See
`docs/mech/MECH-0001-*.md` for how routing-mode decisions are recorded and gated.

## 3c. Qualification profiles vs runtime health

Two layers, **never mixed**:

| Layer | Struct | Who writes it | Lifecycle |
|-------|--------|---------------|-----------|
| Offline benchmark artifact | `QualificationProfile` | `provider-qual` (`qualification.json`) | Immutable; replaced wholesale on re-benchmark |
| Runtime observations | `ProviderHealth` | `InferenceFabric` | Mutable; updated on every request |

`QualificationProfile` is **raw measurements only — no composite `score`**. The
fabric derives `routing_score()` at runtime from profile + health, so scoring
weights can change without invalidating historical benchmark data. Fields the
harness cannot measure yet are `null`, never invented.

```rust
pub struct QualificationProfile {
    pub schema_version: u32,                 // QUALIFICATION_SCHEMA_VERSION = 1
    pub provider_id: String,
    pub model_id: String,
    pub benchmark_config: BenchmarkConfig,   // prompt_set, iterations, ctx, temperature, gpu_layers, batch
    pub generated_at_ms: u64,
    pub ttft_ms: Option<f64>,
    pub tokens_per_second: Option<f64>,
    pub json_accuracy: Option<f64>,
    pub tool_accuracy: Option<f64>,
    pub peak_ram_mb: Option<u32>,
    pub peak_vram_mb: Option<u32>,
    pub benchmark_duration_ms: u64,
}
```

Contract:

1. `provider-qual` writes one `qualification.json` per run, next to the existing
   evidence pack (`report.json`/`metrics.json`/`trace.json`). TTFT and throughput
   are measured by the streaming scenario; host/accelerator memory sampling is not
   yet instrumented and stays `null`.
2. `InferenceFabric::load_qualification_profiles(dir)` recursively ingests every
   `qualification.json` under `dir`, keyed `provider/model`. `QUALIFICATION_DIR`
   points at the directory at startup (`default_from_env`); loading is additive and
   never changes selection. Incompatible `schema_version` is skipped with a
   warning, never guessed at.
3. `routing_score(profile, health)` combines offline quality (50%), speed (30%),
   and live health (20%) with missing measurements falling back to a neutral
   midpoint. Weights are policy in the fabric, not data in the profile.
4. `RoutingDecision` stays FROZEN v1 — profiles and health are runtime consults,
   never audit-trail mutations.

### Ownership + lifetime — one writer per source of truth

Three sources of truth exist; each has exactly one owner and one lifetime, and
must never drift into another owner's lane:

| Source of truth | Owner | Lifetime | Answers |
|-----------------|-------|----------|---------|
| **Capabilities** | the registering side (fabric/`model()`/catalog) | months — until the model/provider changes | "Can I edit this?" → yes, at registration only |
| **`QualificationProfile`** | `provider-qual` | until the next qualification run replaces it | "Should this survive restart?" → yes, it is a file artifact |
| **`ProviderHealth`** | `InferenceFabric` | runtime + persisted snapshots (fabric-owned) | "Who regenerates this?" → the fabric, from live observations |

| Source of truth | Written by | Never written by |
|-----------------|------------|------------------|
| **Capabilities** | manual declaration or discovery | the benchmark harness; the runtime's scoring path |
| **`QualificationProfile`** | `write_qualification` only | the runtime — the fabric only *reads* (`load_qualification_profiles`) |
| **`ProviderHealth`** | the fabric's request path | `provider-qual` — the harness produces measurements, never health state |

### Authoritative update paths

Every state has exactly one authority for each lifecycle operation, so a future
contributor can never accidentally write health from the harness or qualification
data from runtime code:

```text
QualificationProfile   ProviderHealth

Create  -> provider-qual        Create  -> Fabric
Update  -> provider-qual        Update  -> Fabric
Delete  -> provider-qual        Persist -> Fabric
                                Reset   -> Fabric
```

Capabilities have a single path as well: **registration** (declare or discover),
and replacement happens only by re-registering — there is no runtime mutation of
a registered capability set.

Two consequences are enforced here so the layers cannot overlap later:

- **One health representation.** The fabric today persists health as the scalar
  `health_scores` map plus `PersistedProviderHealth` (`providers.rs`). Step 2
  (runtime health metrics) must fold **both** into the single fabric-owned
  `ProviderHealth` struct — never keep three parallel health shapes.
- **Profiles are never re-scored in place.** A `QualificationProfile` is written
  once and read many times. Re-weighting routing is a fabric-side policy change,
  not a profile edit.

### Future: `RoutingPolicy` (strategy, not formula)

`routing_score()` is a placeholder for a strategy so FRIDAY can swap objectives
without touching the fabric:

```rust
trait RoutingPolicy {
    fn score(
        &self,
        qualification: &QualificationProfile,
        health: &ProviderHealth,
        request: &RequestRequirements,
    ) -> f64;
}
```

Guardrail — **the planner expresses constraints, not policies.** The default
routing policy solves an optimization problem over requirements; it does not
pre-pick a policy per request:

```rust
struct RequestRequirements {
    reasoning: Requirement,   // None | Preferred | Required | High
    latency_ms: Option<u32>,  // bound
    offline: Preference,      // Preferred | Required
    budget: Budget,           // Free | Bounded
    tools: Requirement,
}
```

One `RoutingPolicy` (the seeded default, solving over `RequestRequirements`) is
the right shape for nearly all requests. Additional policies only earn a place
when they embody a genuinely different *optimization philosophy* (e.g. an
energy-efficiency objective), not merely a different user request — otherwise you
end up with dozens of near-identical strategies. Do not build any of this before
the constraint-based-routing step; until then `routing_score()` is the single
seeded default.

`ProviderHealth` (see `qualification.rs`): `availability`, moving TTFT / TPS,
`success_rate`, `current_rate_limit`, `last_error`, `circuit_state`. It is the
future replacement signal for the static per-provider `reliability`/`latency_ms`
heuristics; wiring it into `select_candidates` is a later step, not this one.

## 3. Provider Model

```rust
pub struct Provider {
    pub id: String,              // "ollama", "openai", "anthropic", "google"
    pub kind: ProviderKind,      // Local, Cloud, SelfHosted
    pub models: Vec<Model>,
    pub config: ProviderConfig,
    pub status: ProviderStatus,
    pub metrics: ProviderMetrics,
}

pub enum ProviderKind {
    Local,      // Ollama, llama.cpp, vLLM
    Cloud,      // OpenAI, Anthropic, Google
    SelfHosted, // Azure OpenAI, custom endpoints
}

pub struct Model {
    pub id: String,              // "gpt-4o", "claude-sonnet-4-20250514", "llama3.1:8b"
    pub name: String,            // "GPT-4o", "Claude Sonnet 4", "Llama 3.1 8B"
    pub provider_id: String,
    pub capabilities: ModelCapabilities,
    pub limits: ModelLimits,
    pub pricing: Option<ModelPricing>,
    pub quality_score: f64,      // 0..1, updated based on outcomes
}

pub struct ModelCapabilities {
    pub text: bool,
    pub code: bool,
    pub vision: bool,
    pub function_calling: bool,
    pub structured_output: bool,
    pub context_window: u32,     // max tokens
    pub max_output: u32,
}

pub struct ModelLimits {
    pub requests_per_minute: u32,
    pub tokens_per_minute: u32,
    pub daily_requests: u32,
    pub daily_tokens: u64,
}

pub struct ModelPricing {
    pub input_per_1k_tokens: f64,
    pub output_per_1k_tokens: f64,
    pub currency: String,
}
```

## 4. Provider Discovery

### 4.1 Local providers

Scan for running local LLM servers:

```rust
pub async fn discover_local() -> Vec<Provider> {
    let mut providers = vec![];

    // Ollama
    if let Some(ollama) = check_ollama("http://localhost:11434").await {
        providers.push(ollama);
    }

    // llama.cpp server
    if let Some(llamacpp) = check_llamacpp("http://localhost:8080").await {
        providers.push(llamacpp);
    }

    // vLLM
    if let Some(vllm) = check_vllm("http://localhost:8000").await {
        providers.push(vllm);
    }

    providers
}
```

### 4.2 Cloud providers

Configured via `~/.friday/providers.json` or environment variables:

```json
{
  "providers": [
    {
      "id": "openai",
      "kind": "cloud",
      "api_key_env": "OPENAI_API_KEY",
      "base_url": "https://api.openai.com/v1",
      "models": ["gpt-4o", "gpt-4o-mini", "o1"]
    },
    {
      "id": "anthropic",
      "kind": "cloud",
      "api_key_env": "ANTHROPIC_API_KEY",
      "base_url": "https://api.anthropic.com",
      "models": ["claude-sonnet-4-20250514", "claude-haiku-4-20250514"]
    }
  ]
}
```

### 4.3 Self-hosted providers

Azure OpenAI, custom endpoints, or FRIDAY's own hosted inference.

## 5. Model Selection

The Provider Engine selects models based on task requirements:

```rust
pub struct TaskRequirements {
    pub task_type: TaskType,        // planning, coding, reasoning, vision
    pub max_latency_ms: Option<u64>,
    pub max_cost: Option<f64>,
    pub requires_vision: bool,
    pub requires_function_calling: bool,
    pub context_length: u32,
    pub quality_threshold: f64,     // minimum quality score
}

pub enum TaskType {
    Planning,     // high-level goal decomposition
    Coding,       // code generation, review, refactoring
    Reasoning,    // complex analysis, multi-step logic
    Vision,       // image understanding, OCR
    Embedding,    // text embedding for search
    Conversation, // back-and-forth dialogue
}
```

Selection algorithm:

```
1. Filter models by capability requirements (vision, function calling, etc.)
2. Filter by context window >= required context length
3. Filter by quality score >= threshold
4. Rank by: quality_score * (1 / cost) * (1 / latency)
5. Check capacity: is rate limit available?
6. Return top-ranked model with available capacity
7. If no model meets requirements, fallback to best available
```

## 6. Capacity Tracking

```rust
pub struct CapacityTracker {
    pub provider_id: String,
    pub model_id: String,
    pub requests_used: u32,
    pub tokens_used: u64,
    pub window_start: DateTime<Utc>,
    pub limits: ModelLimits,
}

impl CapacityTracker {
    pub fn can_handle(&self, estimated_tokens: u32) -> bool;
    pub fn record_usage(&mut self, input_tokens: u32, output_tokens: u32);
    pub fn reset_window(&mut self);
    pub fn estimated_wait_ms(&self) -> u64;  // time until capacity available
}
```

## 7. Quality tracking

Models are scored based on outcomes:

```rust
pub struct QualityScore {
    pub model_id: String,
    pub success_rate: f64,         // % of successful completions
    pub average_latency_ms: f64,
    pub user_satisfaction: f64,    // from explicit feedback
    pub task_accuracy: f64,        // did the output solve the problem?
    pub overall: f64,              // weighted composite
}
```

Quality scores are updated after each task:
- Success: +0.01 (capped at 1.0)
- Failure: -0.02
- User thumbs up: +0.05
- User thumbs down: -0.10

Scores are persisted and used for model selection.

## 8. Failover and fallback

When a provider fails:

```
Primary: OpenAI GPT-4o
  ↓ timeout / rate limit / error
Fallback 1: Anthropic Claude Sonnet 4
  ↓ timeout / rate limit / error
Fallback 2: Local Ollama llama3.1:8b
  ↓ all providers failed
Escalate to user: "All providers unavailable, retry later?"
```

Failover rules:
1. Try primary provider
2. On failure, try next available provider
3. If all cloud providers fail, try local
4. If everything fails, report to user
5. Record failure for future model selection

## 9. Cost tracking

```rust
pub struct CostTracker {
    pub daily_cost: f64,
    pub monthly_cost: f64,
    pub cost_by_provider: HashMap<String, f64>,
    pub cost_by_model: HashMap<String, f64>,
    pub budget_daily: Option<f64>,
    pub budget_monthly: Option<f64>,
}

impl CostTracker {
    pub fn record_cost(&mut self, model_id: &str, input_tokens: u32, output_tokens: u32);
    pub fn can_afford(&self, estimated_cost: f64) -> bool;
    pub fn remaining_budget(&self) -> f64;
}
```

If a task would exceed the budget, the Provider Engine:
1. Warns the user
2. Suggests a cheaper model
3. Waits for approval before proceeding

## 10. Provider health monitoring

```rust
pub struct ProviderHealth {
    pub provider_id: String,
    pub status: HealthStatus,      // Healthy, Degraded, Unhealthy, Unknown
    pub latency_ms: f64,           // average response time
    pub error_rate: f64,           // % of failed requests
    pub uptime_pct: f64,           // % of time available
    pub last_check: DateTime<Utc>,
    pub last_error: Option<String>,
}
```

Health checks run every 60 seconds:
1. Ping provider endpoint
2. Make a minimal completion request
3. Record latency and success
4. Update health status
5. Emit `provider.health.updated` event

## 11. Events

| Event | Payload | Producer |
|-------|---------|----------|
| `provider.discovered` | `{ provider_id, kind, models }` | ProviderEngine |
| `provider.connected` | `{ provider_id }` | ProviderEngine |
| `provider.disconnected` | `{ provider_id, reason }` | ProviderEngine |
| `provider.health.updated` | `{ provider_id, status, latency_ms }` | ProviderEngine |
| `provider.model.selected` | `{ provider_id, model_id, task_type }` | ProviderEngine |
| `provider.model.failed` | `{ provider_id, model_id, error }` | ProviderEngine |
| `provider.failover` | `{ from_provider, to_provider, reason }` | ProviderEngine |
| `provider.capacity.warning` | `{ provider_id, model_id, remaining }` | ProviderEngine |
| `provider.budget.warning` | `{ remaining_daily, remaining_monthly }` | ProviderEngine |
| `provider.cost.recorded` | `{ model_id, cost, daily_total }` | ProviderEngine |
| `provider.quality.updated` | `{ model_id, old_score, new_score }` | ProviderEngine |

## 12. Metrics

```rust
pub struct ProviderMetrics {
    pub total_requests: u64,
    pub total_tokens: u64,
    pub total_cost: f64,
    pub average_latency_ms: f64,
    pub success_rate: f64,
    pub failover_count: u64,
    pub requests_by_provider: HashMap<String, u64>,
    pub requests_by_model: HashMap<String, u64>,
    pub cost_by_provider: HashMap<String, f64>,
}
```

## 13. Acceptance criteria

- [ ] `friday-provider` builds clean (clippy/fmt/tests)
- [ ] Local providers (Ollama) are discovered automatically
- [ ] Cloud providers are configured via `providers.json`
- [ ] Model selection respects task requirements (vision, function calling, etc.)
- [ ] Capacity tracking prevents rate limit violations
- [ ] Quality scores update based on outcomes
- [ ] Failover works: primary → secondary → local → user
- [ ] Cost tracking stays under daily/monthly budgets
- [ ] Health monitoring detects unhealthy providers
- [ ] The Tool Selector never sees provider details — only tool selection
- [ ] Test: select model for "code review" task → returns appropriate model with capacity

## 14. What this unlocks

- **Multi-provider support:** FRIDAY uses the best model for each task
- **Cost optimization:** cheaper models for simple tasks, powerful for complex
- **Reliability:** failover ensures FRIDAY always has a working model
- **Capacity awareness:** never hits rate limits unexpectedly
- **Quality tracking:** models improve over time based on outcomes
- **User control:** budget limits, provider preferences, model selection
- **Local-first:** uses local models when possible, cloud when needed
