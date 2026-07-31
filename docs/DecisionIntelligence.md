# Decision Intelligence Platform

> **Status:** ✅ **Sprint 5 FROZEN** — all phases 5A–5H implemented and validated.
> The architecture is feature-complete. Future work extends within these
> boundaries rather than reshaping them.
> **Sprint 6 (Observability & Explainability)**: ✅ 6A (Explainability — `ExplanationEngine` +
> `DecisionExplanation`), ✅ 6B (Decision Timeline — `DecisionTimeline` +
> `TimelineBuilder`), ✅ 6C (Metrics — `MetricsEngine`), ✅ 6D (Replay Engine —
> `ReplayEngine`). **Sprint 6 complete.**

## Top Invariant

> **History is immutable. Intelligence is derived.**

- `DecisionRecord` never changes.
- Experience is a deterministic projection of the ledger.
- Reputation is a deterministic projection of experience.
- Calibration influences only future estimates.
- Predictions evolve, observations do not.

## 1. Purpose

The Capability Planner selects providers using
a scored formula: `reliability*w_r + availability*w_a - latency*w_l - cost*w_c
+ policy*w_p + profile_match*w_m + context_fit*w_x`. This is a heuristic — it
computes a single float, loses the dimensionality of the decision, and has no
mechanism to improve from experience.

The Decision Intelligence Platform replaces the scoring formula with a
**closed adaptive loop**:

```
Decision → Record → Aggregate → Learn → Plan Better
```

Every execution produces a structured `DecisionRecord`. Records accumulate into
`Experience` (statistical aggregates per capability/provider/context). Experience
decays into `Reputation` (long-term trust). Both feed into a `CostModel` that
owns all estimation logic. The `CapabilityPlanner` asks the cost model for
estimates; it never computes statistics itself. `PlannerFeedback` adjusts the
cost model, not the planner directly.

## 2. Architecture Overview

The platform is organized into **three layers**. The operational pipeline
performs work; the learning layer derives knowledge from the immutable record;
the observability layer explains, measures, and re-evaluates decisions.
Dependencies flow downward — learning never feeds back into the execution hot
path, and observability never mutates anything.

```
┌────────────────────────────────────────────────────────────────────────┐
│                        EXECUTION LAYER                                 │
│                                                                        │
│   CapabilityRequest → CapabilityPlanner → CostModel.rank()             │
│        → ExecutionPlan → PluginRuntime → PlanOutcome                   │
│        → DecisionLedger (append-only, immutable)                       │
│                                                                        │
│   Answers: "What should we do, and what happened?"                     │
├────────────────────────────────────────────────────────────────────────┤
│                        LEARNING LAYER                                  │
│                                                                        │
│   DecisionLedger → ExperienceEngine → ExperienceStore                  │
│        → ReputationEngine → ReputationStore                            │
│        → PlannerFeedback → CalibrationStore                            │
│                                                                        │
│   Answers: "What did we learn?" — deterministic projections of the     │
│   ledger; calibration adjusts future estimates only.                   │
├────────────────────────────────────────────────────────────────────────┤
│                       OBSERVABILITY LAYER                              │
│                                                                        │
│   TimelineBuilder → DecisionTimeline                                   │
│        ├─ ExplanationEngine → DecisionExplanation   (why)              │
│        ├─ MetricsEngine     → MetricsReport         (how good)         │
│        └─ ReplayEngine      → ReplayReport          (still optimal?)   │
│                                                                        │
│   Answers: "Why? How good? Would we decide the same today?"            │
│   Read-only — never part of the execution or learning path.            │
└────────────────────────────────────────────────────────────────────────┘
```

### 2.1 Operational data flow

```
                CapabilityRequest
                       │
                       ▼
            CapabilityPlanner
                       │
            ┌──────────┼──────────┐
            ▼          ▼          ▼
      Health    Policy      Profile
      Eval      Eval        Eval
                       │
            ┌──────────┼──────────┐
            ▼          ▼          ▼
       Context    CostModel.rank()
       Eval           │
                  ┌───┴───┐
                  ▼       ▼
            Experience  Reputation
            Store       Store
                       │
                       ▼
               ExecutionPlan
               (estimated cost,
                latency, success,
                confidence,
                fallback chain,
                reasoning)
                       │
                       ▼
             ExecutionRuntime
               (follows the plan,
                no decisions)
                       │
                       ▼
              CapabilityResult
                       │
                       ▼
            DecisionLedger
            (append-only,
             immutable)
                       │
                       ▼
               ExperienceEngine
               (batch aggregation)
                       │
                  ┌───┴───┐
                  ▼       ▼
            Experience  Reputation
            Store       Store
                  │       │
                  └───┬───┘
                      ▼
                 PlannerFeedback
                      │
                      ▼
              CostModel.apply_feedback()
```

Every component answers exactly one question:

| Component            | Question                                  |
| -------------------- | ----------------------------------------- |
| CapabilityCatalog    | What capabilities exist?                  |
| CapabilityRegistry   | Who provides each capability?             |
| PolicyEngine         | What is allowed?                          |
| CapabilityPlanner    | What should we do?                        |
| CostModel            | What should each plan cost?               |
| ExecutionRuntime     | How do we execute this plan?              |
| DecisionLedger       | What happened?                            |
| ExperienceStore      | What did we learn?                        |
| ReputationStore      | Who has proven reliable over time?        |
| RuntimeHealth        | Is it alive right now?                    |

## 3. Core Components

### 3.1 ExecutionPlan

The output of planning. A plan is **not** a provider ID — it is a structured
description of how to execute, with estimates and alternatives.

```rust
pub struct ExecutionPlan {
    /// The logical capability being executed.
    pub capability: Capability,

    /// The chosen physical operator (plugin implementation).
    pub operator_id: String,

    // ── Estimated cost dimensions ────────────────────────────────────
    pub estimated_latency_ms: u64,
    pub estimated_cost: f64,
    pub estimated_success_probability: f64,
    pub estimated_risk: f64,       // 0.0 (none) to 1.0 (certain failure)
    pub confidence: f64,           // 0.0 (guess) to 1.0 (high certainty)

    // ── Fallback chain ───────────────────────────────────────────────
    /// Alternative plans to try if this one fails, in preference order.
    pub fallback_chain: Vec<ExecutionPlan>,

    // ── Reasoning ────────────────────────────────────────────────────
    pub rationale: PlanningRationale,
}
```

**Rules:**
- An `ExecutionPlan` is **never modified** during execution. If it fails, the
  runtime reports the failure; the planner produces a new plan.
- `fallback_chain` contains fully-specified plans, not just provider IDs. This
  lets the planner pre-compute alternative strategies with their own cost
  estimates.

### 3.2 PlanningRationale

Captures **why** the planner chose this plan over
alternatives, with evidence from each evaluator.

```rust
pub struct PlanningRationale {
    /// All candidates that were evaluated.
    pub candidates: Vec<ScoredCandidate>,

    /// The chosen operator ID.
    pub selected_operator_id: String,

    /// Composite score (derived, not primary).
    pub composite_score: f64,

    /// Human-readable explanation.
    pub explanation: String,

    /// Correlation ID for traceability.
    pub correlation_id: CorrelationId,

    /// Evidence from each evaluator that contributed to the decision.
    pub evaluator_evidence: Vec<EvaluatorSignal>,
}

/// A single evaluator's contribution to the decision.
pub struct EvaluatorSignal {
    pub source: EvaluatorKind,   // Health, Policy, Profile, Context, Experience, Reputation
    pub score: f64,              // Raw score from this evaluator [-1, 1]
    pub confidence: f64,         // How reliable this evaluator's signal is [0, 1]
    pub explanation: String,     // Why this score was assigned
}

pub enum EvaluatorKind {
    Health,
    Policy,
    Profile,
    Context,
    Experience,
    Reputation,
}
```

**Rules:**
- The `composite_score` is a derived convenience value. The planner's actual
  decision logic operates on the dimensional cost estimates in `ExecutionPlan`,
  not on a single score.
- `evaluator_evidence` is append-only. Every evaluator that ran contributes a
  signal, regardless of whether it affected the final decision.

### 3.3 DecisionRecord

The central primitive. Created after every execution. Immutable, append-only.

Estimates live in `ExecutionPlan.plan` and are not duplicated here. The outcome
is bundled into a `PlanOutcome` struct that future multi-step plans can extend.

```rust
/// What actually happened during execution (as opposed to what was planned).
pub struct PlanOutcome {
    pub latency_ms: u64,
    pub cost: f64,
    pub success: bool,
    pub error: Option<String>,
    pub completed_steps: u64,
    pub cancelled: bool,
    pub timeout: bool,
    pub retries: u32,
}

pub struct DecisionRecord {
    pub id: CorrelationId,
    pub timestamp_ms: u64,

    // ── What was requested ───────────────────────────────────────────
    pub capability: Capability,
    pub context: SelectionContext,
    pub parameters: HashMap<String, String>,

    // ── What was planned (includes all estimated_* fields) ───────────
    pub plan: ExecutionPlan,

    // ── What actually happened ───────────────────────────────────────
    pub outcome: PlanOutcome,

    // ── Reflection (populated post-hoc, not at execution time) ───────
    pub reflection: Option<DecisionReflection>,
}

pub struct DecisionReflection {
    /// Was the estimate accurate?
    pub latency_variance: f64,   // positive = slower than estimated
    pub cost_variance: f64,
    pub success_accurate: bool,

    /// What went wrong, if anything.
    pub failure_reason: Option<String>,

    /// Lessons extracted for future planning.
    pub lessons: Vec<String>,
}
```

**Rules:**
- `DecisionRecord` is never mutated after creation.
- Estimates are **not** duplicated — they live in `plan.estimated_*`.
- `reflection` is populated asynchronously by the Experience Engine after the
  execution completes and context is available (not by the execution hot path).
- Records are stored in the `DecisionLedger` — an append-only store.

### 3.4 Experience

Statistical aggregates derived from `DecisionRecord`s. Not raw telemetry —
summarized knowledge.

```rust
/// Key for looking up experience data.
///
/// Designed for hierarchical decomposition. Currently keyed by
/// (Capability, OperatorId), but the key space is explicitly extensible
/// to add context dimensions (Urgency, SecurityLevel, offline mode, etc.)
/// without changing the aggregate type.
#[derive(Debug, Clone, Hash, Eq, PartialEq)]
pub struct ExperienceKey {
    pub capability: Capability,
    pub operator_id: String,
    // Future: pub urgency: Option<Urgency>,
    // Future: pub security_level: Option<SecurityLevel>,
    // Future: pub offline: Option<bool>,
}

pub struct Experience {
    pub key: ExperienceKey,

    /// How many times has this capability been executed on this operator?
    pub sample_size: u64,

    /// Aggregate success metrics.
    pub success_rate: f64,           // weighted by recency
    pub success_count: u64,
    pub failure_count: u64,

    /// Aggregate latency metrics.
    pub average_latency_ms: f64,     // exponential moving average
    pub p50_latency_ms: u64,
    pub p95_latency_ms: u64,
    pub p99_latency_ms: u64,

    /// Recency-weighted aggregates.
    pub last_execution_ms: Option<u64>,
    pub last_failure_ms: Option<u64>,
    pub last_failure_reason: Option<String>,

    /// Consecutive successes/failures (for circuit-breaker-like signals).
    pub consecutive_successes: u64,
    pub consecutive_failures: u64,

    /// Timestamp of the earliest record contributing to this experience.
    pub since_ms: u64,
}
```

**Rules:**
- `Experience` is derived **exclusively** from `DecisionRecord`s. Never from
  in-flight telemetry.
- `Experience` is keyed by `ExperienceKey`. The key can gain new dimensions
  without changing the aggregate struct.
- Latency percentiles are computed from the sliding window, not from all
  history.

### 3.5 Reputation

Long-term trust derived from `Experience` with time decay. Separated from
runtime health.

```rust
pub struct Reputation {
    pub operator_id: String,

    /// Long-term reliability with exponential decay.
    pub reliability: f64,             // 0.0 to 1.0

    /// Confidence in this reputation estimate.
    /// Low when sample_size is small; approaches 1.0 as evidence accumulates.
    pub confidence: f64,              // 0.0 to 1.0

    /// Total observations contributing to reputation.
    pub total_observations: u64,

    /// When the earliest contributing observation was made.
    pub since_ms: u64,

    /// When this reputation was last updated.
    pub last_updated_ms: u64,

    /// Decay factor: how quickly old observations lose weight.
    /// Default: 0.95 (half-life of ~14 observations with uniform weighting).
    pub decay_factor: f64,
}
```

**Rules:**
- `Reputation` depends **only** on `Experience`, never directly on
  `DecisionRecord`s.
- `Reputation` is operator-level, not capability-level. An operator may have
  different experience per capability, but reputation is a cross-capability
  trust signal.
- Time decay ensures that a provider that was perfect six months ago but has
  failed every request this week does not retain a high score.

### 3.6 PlannerFeedback

The signal that closes the adaptive loop. Produced by comparing estimates
against actuals. Feeds into the **CostModel**, never directly into the planner.

```rust
pub struct PlannerFeedback {
    pub capability: Capability,
    pub operator_id: String,

    /// Estimate accuracy.
    pub latency_error_ms: i64,        // estimated - actual (negative = over-estimated)
    pub cost_error: f64,
    pub success_bias: f64,            // 1.0 = always optimistic, -1.0 = always pessimistic

    /// How much to adjust the cost model's estimate for this (capability, operator).
    pub latency_adjustment_ms: f64,
    pub cost_adjustment: f64,
    pub success_adjustment: f64,
    pub confidence_adjustment: f64,

    /// Whether this feedback should cause the cost model to re-evaluate.
    pub trigger_model_update: bool,

    pub timestamp_ms: u64,
}
```

**Rules:**
- `PlannerFeedback` is derived from `DecisionReflection`, not from raw records.
- `PlannerFeedback` is consumed by the `CostModel`, **not** by the planner.
  The planner never adjusts its own estimates.
- `trigger_model_update` is set when the error exceeds a threshold (e.g.,
  estimated latency was 10x actual, or estimated success was 100% but actual
  was 0%).

### 3.7 CostModel

Owns all estimation logic. The central prediction layer between evaluators and
the planner. It is **stateless** — a pure function over the knowledge stores
(`ExperienceStore`, `ReputationStore`, `CalibrationStore`). The stores own the
aggregates; the model derives estimates from them.

```rust
/// A cost estimate for a single operator on a capability.
pub struct CostEstimate {
    pub operator_id: String,
    pub estimated_latency_ms: u64,
    pub estimated_cost: f64,
    pub estimated_success_probability: f64,
    pub estimated_risk: f64,
    pub confidence: f64,
}

/// Pure derivation over the knowledge stores; owns no state.
pub struct CostModel;

impl CostModel {
    /// Produce a cost estimate for a given operator and capability,
    /// blending Experience (sample-weighted) with Reputation and any
    /// calibration offsets.
    pub fn estimate(
        capability: &Capability,
        operator_id: &str,
        experience_store: &ExperienceStore,
        reputation_store: &ReputationStore,
        calibration: Option<&CalibrationStore>,
    ) -> CostEstimate;

    /// Produce cost estimates for all candidates, ranked by success
    /// probability (desc) then confidence.
    pub fn rank(
        capability: &Capability,
        candidates: &[String],
        experience_store: &ExperienceStore,
        reputation_store: &ReputationStore,
        calibration: Option<&CalibrationStore>,
    ) -> Vec<CostEstimate>;

    /// Incorporate feedback from an execution by mutating the
    /// `CalibrationStore` (the only mutable state in the layer).
    pub fn apply_feedback(feedback: &PlannerFeedback, calibration: &mut CalibrationStore);

    /// Invalidate cached estimates for a (capability, operator) pair
    /// when they have drifted significantly from reality. (Not yet
    /// implemented.)
    pub fn invalidate(capability: &Capability, operator_id: &str);
}
```

**Rules:**
- The `CapabilityPlanner` never computes statistics. It calls
  `CostModel::estimate()` or `CostModel::rank()`.
- The model owns the estimation formula (EMA weighting, decay, calibration
  blending). The stores own the data; nothing else owns statistical logic.
- `apply_feedback()` is asynchronous — never called on the hot path.
- The cost model does **not** make decisions. It produces estimates. The
  planner combines estimates with policy, context, and strategy to choose a
  plan.

## 4. Ownership Boundaries

```
┌──────────────────────────────────────────────────────────────────┐
│                   friday-plugin-runtime                            │
│                                                                    │
│  ┌────────────────────┐  ┌────────────────────┐                   │
│  │  CapabilityPlanner  │  │  ExecutionRuntime   │                   │
│  │  (planning only)    │  │  (execution only)   │                   │
│  └─────────┬──────────┘  └──────────┬─────────┘                   │
│            │                        │                             │
│            ▼                        ▼                             │
│  ┌─────────────────────────────────────────┐                      │
│  │         DecisionLedger                   │                      │
│  │  (append-only, in-memory ring buffer)    │                      │
│  └────────────────┬────────────────────────┘                      │
└───────────────────┼──────────────────────────────────────────────┘
                    │
                    ▼
┌──────────────────────────────────────────────────────────────────┐
│               friday-experience crate                              │
│                                                                    │
│  ┌──────────────────┐  ┌──────────────────┐     ┌──────────────┐ │
│  │  ExperienceEngine │  │  ReputationEngine│     │ Calibration  │ │
│  │  (aggregates      │  │  (derives trust)  │     │  Store       │ │
│  │   DecisionRecords)│  │                   │     │  (offsets)   │ │
│  └────────┬─────────┘  └────────┬─────────┘     └──────┬───────┘ │
│           │                     │                      │         │
│           └─────────┬──────────┘                      │         │
│                     ▼                                  │         │
│          ┌──────────────────┐                          │         │
│          │  PlannerFeedback  │─────────────────────────┘         │
│          └──────────────────┘                                    │
│                                                                    │
│  Read models: CostModel (stateless), ExplanationEngine,           │
│               TimelineBuilder, MetricsEngine, ReplayEngine        │
└──────────────────────────────────────────────────────────────────┘
                    │
                    │  CostModel.estimate()
                    │  CostModel.rank()
                    ▼
          CapabilityPlanner
          (calls CostModel,
           never computes
           statistics)
```

**Key boundary rules:**
- `friday-plugin-runtime` owns the execution hot path and the decision ledger.
- `friday-experience` owns learning, aggregation, cost estimation, calibration,
  and the observability read models (timeline, explanation, metrics, replay).
- The runtime never depends on the experience crate. The experience crate
  depends on `DecisionRecord` (from the runtime crate).
- `RuntimeHealth` stays in the runtime crate. It contains only live operational
  state (availability, current status, current latency). It does not contain
  long-term aggregates.
- `ReputationStore` lives in the experience crate. It does not contain live
  state. It contains only decayed historical aggregates.
- `PlannerFeedback` is consumed by the `CostModel`, **not** by the planner.
  The planner never adjusts its own estimates.
- The `CapabilityPlanner` calls `CostModel.estimate()` or `CostModel.rank()`
  to get estimates. It never computes statistics itself.

## 5. Data Flow

### Execution path (hot path — no learning)

```
CapabilityRequest
       │
       ▼
CapabilityPlanner.plan()
       │  (calls CostModel.rank())
       ▼
ExecutionPlan
       │
       ▼
ExecutionRuntime.execute(plan)
       │
       ▼
CapabilityResult
       │
       ▼
DecisionLedger.append(DecisionRecord)
```

### Learning path (cold path — asynchronous, non-blocking)

```
DecisionLedger
       │ (batch read)
       ▼
ExperienceEngine.update()
       │
       ▼
ExperienceStore
       │
       ▼
ReputationStore.update()
       │
       ▼
PlannerFeedback
       │
       ▼
CostModel.apply_feedback()
```

### Planner evaluation path (decision time)

```
SelectionContext + Capability
       │
       ▼
CapabilityPlanner.plan()
       │
       ├── HealthEvaluator.evaluate()  → RuntimeHealth (live)
       ├── PolicyEvaluator.evaluate()  → PolicyEngine (rules)
       ├── ProfileEvaluator.evaluate() → ProviderProfile (static)
       ├── ContextEvaluator.evaluate() → SelectionContext (request)
       └── CostModel.rank()            → CostEstimate[] (experience + reputation + decay)
       │
       ▼
ExecutionPlan (with evaluator evidence)
```

Note: The planner does not call Experience and Reputation evaluators directly.
It calls `CostModel.rank()` which internally consults `ExperienceStore` and
`ReputationStore` to produce dimensional cost estimates.

## 6. Lifecycle

### 6.1 DecisionRecord lifecycle

```
1. Created  — after execution completes, before returning to caller
2. Stored   — appended to DecisionLedger (in-memory ring buffer)
3. Read     — consumed by ExperienceEngine (batch, async)
4. Archived — eventually summarized and removed from hot storage
```

### 6.2 Experience lifecycle

```
1. Update   — ExperienceEngine reads new DecisionRecords
2. Aggregate — compute new sliding-window statistics
3. Store    — overwrite previous Experience for (capability, operator)
4. Prune    — remove Experience with sample_size below threshold
```

### 6.3 Reputation lifecycle

```
1. Compute  — derived from Experience on every Experience update
2. Decay    — apply time decay to old observations
3. Store    — overwrite previous reputation
4. Prune    — remove operators that have not been used in extended period
```

## 7. Planner Feedback Loop

The feedback loop has four stages:

### Stage 1: Record

Every execution produces a `DecisionRecord` containing both estimates and
actuals. This is mandatory and synchronous — the hot path always records.

### Stage 2: Reflect

`DecisionReflection` is computed asynchronously. It compares estimates against
actuals and extracts:

- `latency_variance = actual_latency - estimated_latency`
- `cost_variance = actual_cost - estimated_cost`
- `success_accurate = (estimated_success > 0.5) == actual_success`
- Failure reason and lessons (if failed)

### Stage 3: Aggregate

The `ExperienceEngine` processes new `DecisionReflection`s and updates
`Experience` aggregates. This is a batch operation, not per-execution.

### Stage 4: Adjust

When the aggregation reveals a significant drift (e.g., `latency_variance` is
consistently positive for a given capability/operator), `PlannerFeedback` is
produced and consumed by the `CostModel`. The cost model adjusts its internal
estimate for that `(capability, operator)` pair.

**Feedback is not applied during execution.** The `CostModel` is updated
asynchronously. The planner always calls `CostModel.estimate()` — it never
holds stale estimates. This ensures that the execution hot path is never
blocked by learning and the planner always sees the latest cost model without
managing state.

## 8. Future Extensions

These are explicitly **not** part of Sprint 5 but are anticipated by the
architecture:

### 8.1 Context-specific experience

Experience today is keyed by `(Capability, OperatorId)`. A future extension
would add context dimensions:

```
(Capability, OperatorId, Urgency, SecurityLevel) → Experience
```

This allows the planner to answer: "How does this operator perform under
critical urgency?" vs "How does it perform in normal conditions?"

### 8.2 Plan caching

Repeated identical capabilities can skip planning:

```
1. Lookup cache: (Capability, SelectionContext hash) → CachedPlan
2. If cached and still valid (estimates match recent actuals): use cached plan
3. If cached but estimates have drifted: invalidate, re-plan
4. If not cached: plan, cache result
```

### 8.3 Replanning on failure

```
ExecutionPlan → Runtime → Failure → PlannerFeedback → CostModel
                                                         │
                                                         ▼
                                                  (adjusted estimates)
                                                         │
                                                         ▼
                                                  CapabilityPlanner
                                                         │
                                                         ▼
                                                  ExecutionPlan (fallback)
                                                         │
                                                         ▼
                                                  Runtime → Success
```

This is not retry logic. Retry says "try again with the same plan." Replanning
says "the assumptions behind the first plan were wrong — build a new one."

### 8.4 Exploration

A future planner might occasionally choose a non-optimal provider to gather
experience data (exploration vs exploitation). This requires the planner to
understand uncertainty, not just expected value.

### 8.5 Cross-session persistence

Today the DecisionLedger is in-memory. A future extension would persist
DecisionRecords to disk, allowing experience and reputation to survive process
restarts.

## 9. API Stability Rules

The runtime crate (`friday-plugin-runtime`) will be divided into three
visibility tiers when the API is frozen:

### Tier 1: Public SDK (stable, breaking changes require major version bump)

```rust
pub trait Plugin                    // unchanged since Sprint 1
pub struct CapabilityRequest        // stable
pub struct CapabilityResult         // stable
pub struct Capability               // stable
pub struct PluginDescriptor         // stable
pub enum RuntimeError               // stable
```

### Tier 2: Advanced API (stable with caution, breaking changes documented)

```rust
pub struct SelectionContext         // new in Sprint 4
pub struct ProviderProfile          // new in Sprint 4
pub trait TelemetryObserver         // new in Sprint 3
pub struct CorrelationId            // new in Sprint 3
pub struct ExecutionPlan            // new in Sprint 5
pub struct PlanningRationale        // replaces SelectionRationale
pub struct DecisionRecord           // new in Sprint 5
pub struct PlanOutcome              // new in Sprint 5
```

### Tier 3: Internal (no stability guarantee, renamed or removed at any time)

```rust
pub(crate) struct CapabilityPlanner
pub(crate) struct CapabilityRegistry    // stable interface, internal impl
pub(crate) struct CapabilityCatalog     // stable interface, internal impl
pub(crate) struct RuntimeHealth
pub(crate) mod evaluators              // all evaluators
pub(crate) mod policy                  // all policy engines
```

### friday-experience visibility

The experience crate is the public learning + observability surface for
integration authors (dashboards, monitoring, evaluations). Its types are
therefore **public by design**, not `pub(crate)`:

- **Knowledge stores** — `ExperienceStore`, `ReputationStore`,
  `CalibrationStore` (callers read snapshots and apply feedback).
- **Engines** — `ExperienceEngine`, `ReputationEngine` (learning),
  `ExplanationEngine`, `MetricsEngine`, `ReplayEngine` (observability).
- **Read-model types** — `CostEstimate`, `DecisionTimeline`,
  `DecisionExplanation` + factor types + `ExplanationReason`,
  `MetricsReport` + metric structs, `ReplayReport` + snapshot/difference
  types.
- **Stateless `CostModel`** — estimation entry point used by the planner.

No internal helpers are exposed; everything `pub` is a deliberate read model
or store. The crate has no consumers yet, so breaking changes are cheap — but
the surface above is the intended stable boundary.

Every public type carries an intended stability level (mirrored in the crate
root docs of `services/experience/src/lib.rs`):

| Level        | Meaning                                                            |
| ------------ | ------------------------------------------------------------------ |
| `stable`     | Safe to depend on; breaking changes require a documented migration. |
| `experimental` | Intentionally public (integration surface) but still maturing;     |
|              | breaking changes are expected without notice.                      |

- **Stable** — the Sprint 5 learning core: `ExperienceStore`,
  `ExperienceEngine`, `ReputationStore`, `ReputationEngine`,
  `CalibrationStore`, `CalibrationOffsets`, `CostModel` (`invalidate` not yet
  implemented).
- **Experimental** — the Sprint 6 observability layer: `ExplanationEngine`,
  `DecisionExplanation` + factors + `ExplanationReason`, `DecisionTimeline`,
  `TimelineBuilder`, `MetricsEngine` + metric structs + `MetricsReport`,
  `ReplayEngine` + `ReplayReport` + snapshot/difference types.

### Stability rules

1. **Plugin authors** should only depend on Tier 1 types.
2. **Integration authors** (dashboards, monitoring) may depend on Tier 2.
3. **Internal refactors** must never break Tier 1.
4. **Tier 2** breaking changes must be documented in a changelog.
5. **Tier 3** can be changed without notice.

## Appendix: Terminology Migration

The renames below are **complete**. This table exists to decode older
references (issues, history, old code). New code uses the right-hand column.

| Current (Sprint 1-4)          | Final                     | Rationale                                  |
| ----------------------------- | --------------------- | ------------------------------------------- |
| CapabilityResolver            | CapabilityPlanner     | It plans, not just resolves                 |
| resolve_and_execute()         | plan_and_execute()    | Two-phase: plan then execute                |
| PluginRuntime                 | ExecutionRuntime      | Runtime executes, does not plan             |
| SelectionRationale            | PlanningRationale     | Evidence from planning, not selection       |
| TelemetryStore                | DecisionLedger        | Stores decisions, not raw events            |
| TelemetryEvent                | (removed)             | Replaced by DecisionRecord                  |
| TelemetryCollector            | DecisionLedger        | Append-only decision store                 |
| HealthMonitor                 | RuntimeHealth         | Live state only, not long-term aggregates   |
| ScoringWeights                | (removed)             | Replaced by dimensional cost estimates      |
| ScoredCandidate               | (removed)             | Replaced by evaluator evidence              |
| PluginHealth                  | RuntimeHealth         | Live operational state only                 |
| (did not exist)               | ExperienceEngine      | Aggregates DecisionRecords                  |
| (did not exist)               | ExperienceStore       | Statistical aggregates per (cap, op)        |
| (did not exist)               | ReputationStore       | Long-term trust with time decay             |
| (did not exist)               | CostModel             | Estimation + feedback (owns all statistics) |
| (did not exist)               | PlannerFeedback       | Closed-loop signal to cost model            |
| (did not exist)               | PlanOutcome           | Bundled execution result                    |
| (did not exist)               | ExecutionPlan         | First-class plan type with fallback chain   |
| (did not exist)               | DecisionReflection    | Post-hoc analysis of execution              |
| (did not exist)               | EvaluatorSignal       | Typed evaluator output                      |
| (did not exist)               | CostEstimate          | Dimensional cost output from CostModel      |
| (did not exist)               | ExperienceKey         | Hierarchical key for experience lookup      |

## 10. Observability & Explainability (Sprint 6)

Sprint 6 adds a second, orthogonal pipeline. The operational pipeline performs
work; the observability pipeline explains it. They never interfere.

**Naming convention:** read models that *compute* derived values are
`*Engine` (`ExplanationEngine`, `MetricsEngine`, `ReplayEngine` — plus the
learning-layer `ExperienceEngine`/`ReputationEngine`); a read model that
*assembles* a composite snapshot is a `*Builder` (`TimelineBuilder`).

```
          ┌─────────────────────── Execution Layer ───────────────────────┐
          │   Plan → Predict → Execute → Record                           │
          │        │            │            │                            │
          └────────┼────────────┼────────────┼────────────────────────────┘
                   ▼            ▼            ▼
          ┌─────────────────────── Learning Layer ────────────────────────┐
          │   Experience → Reputation → Calibrate (affects future only)   │
          └───────────────────────────────────────────────────────────────┘
                                 │
                                 ▼
          ┌────────────────────── Observability Layer (read-only) ─────────┐
          │   TimelineBuilder → DecisionTimeline                          │
          │        ├─ ExplanationEngine  → DecisionExplanation   (why)    │
          │        ├─ MetricsEngine      → MetricsReport         (how)    │
          │        └─ ReplayEngine       → ReplayReport    (still optimal?)│
          └───────────────────────────────────────────────────────────────┘
```

### 6A — Explainability (✅ complete)

- `DecisionExplanation` — which operator, why, why not the others. Pure read
  over immutable artifacts. Owns no state, mutates nothing.
- Composed factors, each mapping to one architectural layer:
  `CandidateExplanation`, `ExperienceFactor`, `ReputationFactor`,
  `CalibrationFactor`.
- `ExplanationReason` — structured, machine-readable reasons
  (`HighestCompositeScore`, `PolicyConstraint`, `HigherReliability`,
  `BetterHistoricalLatency`, `CalibrationAdjustment`, `FallbackSelection`).
  The human-readable `narrative` is a rendering of these reasons; localization
  and filtering never parse free text.
- `ExplanationEngine::explain(&ExecutionPlan, &ExperienceStore,
  &ReputationStore, &CalibrationStore) -> DecisionExplanation`. Calibration is
  only surfaced when offsets are nonzero — a "Calibration: none" line on every
  decision would be noise.

### 6B — Decision Timeline (✅ complete)

- `DecisionTimeline` — a point-in-time **snapshot** of one full decision:
  `CorrelationId`, reconstructed `CapabilityRequest`, `ExecutionPlan`,
  prediction `CostEstimate`, `PlanOutcome`, the immutable `DecisionRecord`,
  `Experience`/`Reputation`/`Calibration` snapshots, derived `PlannerFeedback`,
  and the `DecisionExplanation`.
- Snapshots, not references: the timeline remains valid even if the live stores
  change after reconstruction.
- `TimelineBuilder` reads the `DecisionLedger`, `ExperienceStore`,
  `ReputationStore`, `CalibrationStore`, and the `ExplanationEngine` — reconstruction
  lives in one place.
- `build(&CorrelationId)` matches the record whose plan carries that request id
  (`plan.rationale.correlation_id`). `build_all()` reconstructs the whole ledger
  in order. Unknown ids return `None`.

### 6C — Metrics & Observability (✅ complete)

- `MetricsEngine` — a **stateless** derivation layer over
  `&[DecisionTimeline]` from `build_all()`. Owns no state, mutates nothing;
  metrics are derived, never stored.
- `PredictionAccuracy` — success prediction accuracy, mean |latency error|,
  mean |cost error|, mean success bias (positive = overconfident).
- `CalibrationMetrics` — mean absolute offsets (latency/cost/success/
  confidence) plus a **convergence** signal: magnitude shrank from the first
  half of decisions to the second (≥ 2 calibrated decisions required).
- `FallbackMetrics` — fraction of plans that carried a fallback chain,
  average fallback depth, failure count and rate.
- `ReputationMetrics` — coverage, avg reliability/confidence, total
  observations, distinct operators.
- `ExperienceMetrics` — coverage, distinct (capability, operator) pairs,
  total samples, avg sample size / success rate / latency.
- `MetricsEngine::summarize()` aggregates all five into a `MetricsReport`.
  Empty input yields all-zero metrics (no panics).

```
TimelineBuilder → Vec<DecisionTimeline> → MetricsEngine → MetricsReport
```

### 6D — Replay Engine (✅ complete)

- `ReplayEngine::replay(&DecisionTimeline, &ExperienceStore, &ReputationStore,
  &CalibrationStore) -> ReplayReport` — answers **"what would happen now?"**
  instead of reconstructing history from scratch. Builds on the existing read
  models (Timeline, ExplanationEngine, CostModel); owns no state.
- Ranks the historical plan's own candidates through today's `CostModel` and
  compares against the historical selection.
- `ReplayReport` is a **comparison between two evaluations** — never a rewrite
  of history. It is structured as immutable `historical`, current `replay`,
  and a `differences` section:
  - `historical` — the immutable snapshot: `plan`, `prediction`,
    `explanation`, `outcome`
  - `replay` — today's evaluation of the same decision: `plan`,
    `prediction` (today's estimate for the historical operator),
    `explanation`, and `ranked_candidates` (today's full ranking)
  - `differences`:
    - `operator_changed` — historical vs today's top-ranked operator
    - `decision_still_optimal` — regression signal, equal to
      `!operator_changed`: would the current planner still make the same
      decision? Enables measuring % historical decisions still optimal, how
      often learning changes selection, and whether calibration improves or
      destabilizes decisions
    - `prediction_delta` — latency / cost / success / confidence deltas
      between `replay.prediction` and `historical.prediction`
    - `reputation_delta` — current reliability vs historical snapshot
    - `calibration_delta` — current |latency offset| vs historical snapshot
    - `explanation_delta` — added/removed `ExplanationReason`s (recomputed
      via the Explainer)
- `replay_timestamp_ms` marks when the replay ran.

```
DecisionTimeline → ReplayEngine → ReplayReport (historical + replay + differences)
```

