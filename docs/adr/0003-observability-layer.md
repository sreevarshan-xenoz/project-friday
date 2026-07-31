# ADR-0003: Observability Layer (Sprint 6)

**Status:** Accepted
**Date:** 2026-07-31
**Scope:** `friday-experience` (read models)

## Context

After Sprint 5, FRIDAY had an adaptive loop but no way to answer three
questions about it: **why** was a decision made, **how good** is the system
at predicting, and **would we choose differently today?** Instrumenting the
execution hot path directly would couple observability to execution and slow
the system. Storing pre-computed metrics would go stale.

The requirement was to make every adaptive decision explainable, measurable,
and replayable — without those capabilities becoming part of execution or
learning.

## Decision

Add a third, orthogonal **Observability** layer of pure read models in
`friday-experience`:

- `TimelineBuilder` → `DecisionTimeline` — a point-in-time **snapshot** of one
  full decision (plan, prediction, outcome, knowledge-store snapshots, derived
  feedback, explanation). Snapshots, not references: valid even if live stores
  change later.
- `ExplanationEngine` → `DecisionExplanation` — which operator, why, why not
  the others. Structured, machine-readable `ExplanationReason`s; the
  human-readable narrative is a rendering, never parsed.
- `MetricsEngine` → `MetricsReport` — stateless derivations over
  `&[DecisionTimeline]` (prediction accuracy, calibration convergence,
  fallback, reputation, experience). Derived, never stored.
- `ReplayEngine` → `ReplayReport` — re-evaluates a historical decision against
  current knowledge. A **comparison between two evaluations**
  (`historical` / `replay` / `differences`), never a rewrite of history.

Naming: `*Engine` computes derived values; `*Builder` assembles composite
snapshots.

## Consequences

- **Positive:** Full auditability without interfering with the execution or
  learning path.
- **Positive:** Every read model is a pure projection — re-derivable, testable,
  and safe to expose.
- **Positive:** `ReplayReport` provides a regression signal
  (`decision_still_optimal`) for measuring whether learning improves or
  destabilizes decisions.
- **Negative:** Read models duplicate some logic conceptually; they must stay
  in lock-step with the projections they read (mitigated by building on the
  existing read models rather than recomputing independently).
- **Negative:** The observability API surface is larger than the learning core
  and is still maturing (see ADR-0004).

## Alternatives Considered

- **Instrument the execution path.** Rejected: couples observability to the
  hot path; adds cost and failure modes to execution.
- **Store derived metrics/explanation eagerly.** Rejected: goes stale; the
  whole point is explaining against *current* knowledge.
- **Replay as re-execution.** Rejected: replay is a read-only evaluation, not
  a second run.
