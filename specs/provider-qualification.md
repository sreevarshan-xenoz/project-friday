# Provider Qualification — Phase 1.5 + M5.2

**Status:** 🟡 Under Qualification (Phase 1.5, M5.2 benchmarks live)
**Owner:** Reliability Engineering
**Crate under test:** `friday-intelligence` (Inference Fabric + providers)
**Rule:** Evidence-based. No feature expansion, no new architecture. We *prove* a
provider before it is trusted, exactly like every other subsystem.

This spec is the qualification plan for LLM providers. It does not change core
contracts, the `Provider` trait, or the Inference Fabric's routing. It adds a
**corpus**, a **failure-injection catalog**, and **provider-specific metrics** so
that OmniRoute — and every future provider — must pass the same pipeline Windows,
Filesystem, and Git already do.

## Why providers, and why now

OmniRoute was integrated in Phase 1 as a drop-in `Provider`. It must not skip
straight into production. FRIDAY already qualifies every other subsystem; a new
provider gets no free pass. The pipeline is:

```
Integrate → Qualify → Generalize → Expand
```

Phase 1 integrated. Phase 1.5 qualified the baseline. **M5.2 (Provider Capability
Layer)** then landed the `ProviderV2` capability-driven trait — and, per the
evidence-first rule, *every new V2 feature auto-creates a new qualification
benchmark*. Streaming and Dynamic Discovery are now live benchmarks; the
capability API did not land as architecture alone — it landed with proof.

V1 (`Provider`) is intentionally **kept** until every provider is migrated to V2
and green; the `ProviderV2Adapter` wraps any V1 provider with conservative,
inferred capabilities so the V2 surface exists immediately without rewrites.

## Scope — provider behaviors FRIDAY depends on

The corpus tests the provider itself (not the model behind it):

- **Connectivity** — gateway reachable via `/models` health probe.
- **Completion** — a trivial prompt returns a non-empty response.
- **JSON** — a strict-JSON prompt returns parseable, schema-valid JSON.
- **Long Context** — a long-context prompt returns a complete, untruncated
  structured answer (proves the full context was handled).
- **Failure Handling** (mock-only) — injected timeout / 429 / 500 / malformed /
  disconnect; the **Inference Fabric** must react: circuit breaker opens, health
  score degrades, metrics recorded.
- **Streaming** (M5.2) — the V2 provider's `stream()` emits incremental
  `CompletionChunk`s; the harness reassembles them without loss and verifies a
  finish reason is observed. Proves token-level streaming works before any UI
  consumes it.
- **Dynamic Discovery** (M5.2) — the V2 provider's `list_models()` returns a
  non-empty, well-shaped catalog of `ModelDescriptor`s. This is the source of
  truth for routing-by-capability, so it must be well-formed.

## Failure injection catalog (the real value)

Injected at the harness level against a mock provider (we cannot inject faults
into a live gateway). Each must be detected and handled by the Fabric:

| # | Injection      | What it proves                                                        |
|---|----------------|-----------------------------------------------------------------------|
| 1 | Timeout        | Fabric records the failure and degrades health.                       |
| 2 | HTTP 429       | Rate-limit error is treated as a provider failure, not a success.     |
| 3 | HTTP 500       | Server error trips the circuit breaker.                               |
| 4 | Malformed JSON | Broken upstream body is surfaced as an error, not silent garbage.     |
| 5 | Disconnect     | Dropped connection is handled gracefully; metrics recorded.           |

These findings feed issues, never silence.

## Provider-specific qualification metrics

Distinct from the global four; precise so they cannot be reinterpreted:

- **Availability** = reachable health probes / connectivity attempts.
- **Completion Success** = non-empty completions / completion attempts.
- **JSON Correctness** = schema-valid JSON responses / JSON attempts.
- **Context Handling** = complete (untruncated) long-context answers / attempts.
- **Failure Handling** = injected failures the Fabric reacted to correctly / injections.
- **Tool Support** = `1.0` when the V2 provider declares `tool_calling` (observed
  by the Streaming / Dynamic Discovery scenarios); `0.0` when it declares false;
  `n/a` when no V2 surface exists (provider not yet migrated to `ProviderV2`).
- **Streaming Reliability** = incremental, loss-free streams observed / streaming
  attempts (M5.2 `ProviderV2::stream`).
- **Dynamic Discovery** = non-empty, well-shaped model catalogs / discovery attempts
  (M5.2 `ProviderV2::list_models`).
- **Mean Latency** = mean ms from request issued to response observed.

## Qualification bar (🟡 → 🟢)

Overall progress = mean of the available metrics, each required to clear its
individual bar (Availability / Completion / JSON / Context ≥ 0.99, Failure
Handling ≥ 0.85). **🟢 Qualified** when overall ≥ 0.85 **sustained** over the
trailing window (e.g. repeated mock + real runs). `Tool Support`, `Streaming
Reliability`, and `Dynamic Discovery` enter the bar now that M5.2's `ProviderV2`
surface exists; they remain `n/a` only for providers still on the V1 trait.

The Resilience Dashboard shows the current sprint on one line:

```
Current Qualification: Provider  ████████░░ 78%
```

## Trust criterion (definition of done)

When OmniRoute is 🟢, you should genuinely trust FRIDAY to route an unsupervised
plan through it, and to survive its disappearance (failover to Ollama / direct
providers) without the user noticing.

## Acceptance criteria (Phase 1.5 exit → 🟢)

- [ ] Provider Qualification Corpus implemented and runnable (`--mock` and real).
- [ ] All 5 failure injections exercise the Fabric and produce an issue (none ignored).
- [ ] The nine provider metrics computed and reported; dashboard shows the sprint line.
- [ ] Overall Provider Qualification ≥ 0.85 with every individual bar met, sustained.
- [ ] M5.2 benchmarks live: **Streaming** and **Dynamic Discovery** exercises run
      against both the mock `MockProviderV2` and the real OmniRoute V2 surface.
- [ ] At least the malformed-JSON and disconnect findings have issues filed; fixes
      land or are explicitly deferred with reason.

## Relationship to other specs

- Does **not** modify `inference_fabric.md` (Fabric contract is frozen this phase).
- Follows `windows-qualification.md` (same evidence-pack discipline, same
  candidate-graduation gate).
- Progress is tracked per `engineering/qualification.md` (trust, not intelligence).
- Unblocks `inference_fabric.md` Phase 2 (Provider Capability API), which lands
  only after this qualification produces evidence.
