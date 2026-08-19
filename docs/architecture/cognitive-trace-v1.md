# FRIDAY Cognitive Trace v1 — FROZEN

**Status: FROZEN (2026-08-19).** This is the v1 cognitive-observability contract.
After the 5A–5E milestone, the semantic meaning of every type below is locked.
Future FRIDAY components implement this contract; they do not redefine it.

The golden test that enforces this contract is
`friday-agent-runtime/tests/director.rs::full_cognitive_episode_trace`
(Phase 5E). It reconstructs one full episode and asserts identity, topology,
causal ordering, real model/tool telemetry, failure semantics, and parallel
execution parentage.

---

## Core model

- **Trace** — a `TraceId` identifies one cognitive episode (one user goal's
  complete lifecycle). All spans in the episode share that `trace_id`.
- **Span** — a `SpanId` identifies one operation inside the episode.
- **Parent** — `parent_span_id` identifies the causal operation that spawned
  the current operation. `None` marks a root span.
- **Event** — an `EventId` is an optional causality link between events
  (distinct from span parentage).

## Identity types

```
TraceId
SpanId
EventId
SessionId   // aggregates multiple goals
GoalId      // one user request
```

## Propagation (the boundary contract)

Any asynchronous or module boundary that *continues* a cognitive episode must
propagate the full `TraceContext`:

```
TraceContext {
    trace_id,
    parent_span_id,   // the upstream span to attach under — NOT just trace_id
    session_id,
    goal_id,
}
```

Propagating only `trace_id` would create two correlated-but-separate trace
trees. Threading the whole `TraceContext` (with `parent_span_id`) yields one
causal episode. The live boundary type is `TraceContext`; the in-module span
factory is `CognitiveTraceContext`.

## Recording types

```
CognitiveTrace           // one immutable span record
CognitiveTracer          // lightweight span-emission interface
TraceCollector           // receives spans for aggregation/telemetry
InMemoryTraceCollector    // test collector
NoopTraceCollector        // disabled telemetry
```

FRIDAY's semantic trace is the **source of truth**. Exporters (e.g.
OpenTelemetry, Phase 6A) are *export targets*, never the origin.

## Execution semantics

```
AgentStep    // what one worker did (capability/tool or model call)
StepKind     // Model | Tool  — explicit reasoning-vs-tool discriminator
ModelUsage   // model/provider/tokens_in/tokens_out/cost for model steps
```

`AgentStep` describes **what the agent did**. `CognitiveTrace` describes **how
that action fits into the cognitive episode**. Keep these worlds separate:
do **not** add trace-linkage fields (`trace_id`/`span_id`/`parent_span_id`)
to `AgentStep`.

## Structural invariants (must hold for every episode)

```
one root per cognitive episode
unique span IDs
valid parent_span IDs (no dangling parents)
no orphan spans
same trace_id throughout the episode
child spans preserve causality
```

## Verification invariant

```
Agent success  ≠  Verification success
```

A failed step propagates failure through the trace; it is never silently
upgraded to verification success. `verification_gate` reports the *gate*
outcome, which can be `Failure` even when the agent ran.

## New-trace rule

A new `TraceId` is created **only** when a genuinely new cognitive episode
begins. Autonomous background work that continues an existing goal reuses the
existing `trace_id` (and propagates `TraceContext` across the boundary).

## Model telemetry

When a provider does not supply information, leave the field `None`. Never
fabricate `model`, `provider`, `tokens_in`, `tokens_out`, or `cost_usd`.

---

## Export adapters (Phase 6A — sanctioned, contract-preserving)

The frozen semantic trace is the source of truth. OpenTelemetry is an
**export target**, never a re-source. The adapter lives in the
`friday-observability` crate:

- `export_span(tracer, &CognitiveTrace)` — projects one `CognitiveTrace` into
  an OpenTelemetry span, preserving trace/span IDs, timing, status, and
  FRIDAY-specific fields as `friday.*` attributes.
- `OtelTraceCollector<T>` — implements `friday_core::trace::TraceCollector`, so
  it drops into any existing `Arc<dyn TraceCollector>` slot without touching
  the frozen types.
- ID mapping: `TraceId` (128-bit Uuid) → OTel 128-bit `TraceId`; `SpanId`
  (128-bit Uuid) → OTel 64-bit `SpanId` (low 8 bytes). Parentage is expressed
  as an OTel `Link` to the parent `SpanContext` (OpenTelemetry 0.27 has no
  `parent_span_id` builder field; Link-based linkage keeps asynchronous
  cross-module parenting intact).

Real OTLP/SDK wiring is a later, equally thin addition — it supplies the
concrete `Tracer` to `OtelTraceCollector` and changes nothing else.

## Production status (honest, as of freeze)

| Capability                                  | Status |
|---------------------------------------------|--------|
| Trace schema supports model spans           | ✅     |
| Tests prove `StepKind::Model` → `llm_call`  | ✅     |
| Production driver emits `Model` steps        | 🟡     |

The production `SkillAgentDriver` (`services/action-runtime`) records each
capability execution as a `Tool` step (`tool_call`). It does **not yet** emit
`Model` steps for the model invocations it makes. Mark the last row green only
once a real production model invocation produces a `StepKind::Model` step.

## Legacy trace systems (audit — KEEP, do not migrate)

These are **distinct** models that predate / sit beside v1 and are **not**
duplicating the cognitive-observability schema. They remain in use:

- `services/goal-execution/src/trace.rs` — `TraceNode` / `TraceNodeKind` /
  the executive **decision-record audit trail** (Investigation, Planning,
  Evaluation, Execution, Verification, Recovery, Reflection, Learning,
  WorldStateChange). This is the autonomy cycle's reasoning history, a
  different axis from the cross-module span tree. NOTE: it also names a type
  `CognitiveTrace` (a `Vec<TraceNode>`); this is unrelated to
  `friday_core::trace::CognitiveTrace` (this frozen contract). Recommended
  follow-up: rename the goal-execution type to avoid the collision.
- `e2e/src/trace.rs` — `ExecutionTrace` / `TraceReplayEngine`. **Test
  infrastructure** for the vertical-slice harness: captures raw LLM I/O
  (prompt, raw + parsed response) and execution outcomes for replay. The v1
  contract intentionally does *not* capture raw prompts/responses, so this
  stays.

Neither should be deleted or merged into v1.
