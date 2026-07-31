# ADR-0002: Learning Layer (Sprint 5)

**Status:** Accepted
**Date:** 2026-07-31
**Scope:** `friday-plugin-runtime` (contracts, ledger, planner), `friday-experience`

## Context

The Capability Planner selected providers with a heuristic scoring formula
(`reliability·w_r + availability·w_a − latency·w_l − cost·w_c + ...`) that
produced a single float, lost the dimensionality of the decision, and had no
mechanism to improve from experience.

FRIDAY needed a closed adaptive loop: decisions must be recorded, aggregated
into knowledge, and used to make better future decisions — without letting
learning corrupt what actually happened.

## Decision

Adopt the learning layer with these invariants:

- Every execution produces an **immutable `DecisionRecord`**, appended to an
  append-only `DecisionLedger`.
- `Experience` is a **deterministic projection** of the ledger (per
  capability/operator aggregates). `Reputation` is a deterministic projection
  of experience (time-decayed trust). Neither is derived from raw telemetry
  or in-flight state.
- `CostModel` is **stateless**: a pure function over the knowledge stores.
  The stores own the data; the model owns the estimation formula.
- `PlannerFeedback` is consumed by the **CostModel** (via the
  `CalibrationStore`), never by the planner directly. Calibration affects
  only future estimates.
- Fallback traversal only **executes pre-existing plans** from the immutable
  fallback chain — the runtime never generates plans.
- One-way dependency: `friday-experience` depends on `friday-plugin-runtime`;
  never the reverse.

The top invariant: **"History is immutable. Intelligence is derived."**

## Consequences

- **Positive:** Predictions evolve; observations do not. Learning cannot
  retroactively rewrite history.
- **Positive:** Deterministic projections are re-derivable and testable.
- **Positive:** No backward dependencies — the hot path is never blocked by
  learning.
- **Negative:** Knowledge is bounded by what the projections capture; richer
  context requires extending the key space, not mutating existing records.
- **Negative:** `CostModel::invalidate` is not yet implemented; cache
  invalidation remains a known gap.

## Alternatives Considered

- **Mutate records in place during learning.** Rejected: destroys
  auditability and the ability to re-derive intelligence.
- **Let the planner adapt its own estimates.** Rejected: couples planning and
  learning, and lets the planner drift away from observed reality.
- **Keep the heuristic score as the source of truth.** Rejected: no learning;
  loses dimensionality of the decision.
