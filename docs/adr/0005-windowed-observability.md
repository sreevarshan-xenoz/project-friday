# ADR-0005: Windowed Observability Read Models (Proposal)

**Status:** Proposed — pending VP-001 run-004 (24h) evidence
**Date:** 2026-08-03
**Scope:** `friday-experience` read models (`DecisionTimeline`,
`TimelineBuilder`, store snapshots); consuming projections in the
ReadModelBridge and any long-running host
**Modifies/Supersedes:** ADR-0003 (Observability Layer) — snapshot semantics;
ADR-0001 core freeze item 4 ("History is immutable. Intelligence is derived.")
remains intact

## Context

VP-001 (long-running cognition) is producing two independent, reinforcing
measurements over retained execution history:

1. **Memory is proportional to retained history.** Each `DecisionTimeline`
   carries full snapshots of the experience/reputation/calibration stores at
   the decision's point in time (ADR-0003's "snapshots, not references"
   design). Run-002: 13.9 → 46.3 MB (52 MB per 1,000 ledger records).
   Run-003: 14.0 → 113.3 MB, and the per-1,000-record cost **fell** to 41.0 MB
   as history grew — per-record cost decreases with scale, ruling out a
   classic (accelerating) leak.
2. **CPU per update is proportional to retained history.** The source's
   `regenerate()` (every cycle) and `dashboard()` (every emission) iterate all
   timelines, so per-frame work grows with history. Run-003 showed RTT
   increasing monotonically — hourly mean 1.1 → 12.0 ms, hourly p95 13 → 103
   ms — as the ledger grew 437 → 2,500 records (~295/hr). The collector is
   constant-work (async, non-blocking), so the inflation is bridge-side
   head-of-line blocking during read-model work, not a measurement artifact.

**Two symptoms, one root cause:** the cost of observability is
**unbounded in retained execution history** — in both memory and CPU.

This is the first time the Validation Program has produced evidence
contradicting the VP-001 hypothesis ("no accumulation effects beyond the
ledger itself"). Per governance (frozen-contract #8), that evidence is the
gate for an architectural change. The proposal is registered **now**, in
Proposed status, and will be decided only after the 24-hour run confirms or
refutes the curves.

## Decision (Proposed — not yet made)

**Bound the cost of observability with respect to retained execution
history.**

The objective is deliberately NOT "reduce memory usage" — memory is the
visible symptom of a deeper property: the read models' work and footprint
scale with how much history is retained. The decision is to make that scaling
**bounded and explicit** rather than linear-unbounded.

The proposal intentionally does not prescribe the mechanism. Candidate
implementations (to be evaluated after the 24-hour run, not today):

- **Retention window** — timelines/snapshots kept for a bounded horizon; older
  history compressed or dropped from derived models (the raw immutable ledger
  is never deleted).
- **`ExperienceSnapshotId` → store lookup** — timelines hold an id instead of
  the snapshot value.
- **`Arc<ExperienceSnapshot>`** — share the snapshot rather than clone it.
- **Delta-only timelines** — timeline keeps only the incremental delta from
  its predecessor.
- **Lazy / incremental materialization** — `dashboard()` and `MetricsEngine`
  recompute incrementally (or on demand) instead of full re-iteration every
  emission/cycle.

Any chosen mechanism must preserve ADR-0003's guarantees: read models remain
pure projections, "history is immutable," and explanation/metrics stay valid
against current knowledge.

## Consequences (expected, not yet measured)

- **Positive:** memory and per-frame CPU become bounded by a retention window
  instead of total runtime; RTT stops degrading with history.
- **Positive:** 24 h / 72 h / 1 week runs become feasible (currently projected
  ~370 MB at 24 h, multi-GB at 1 week).
- **Positive:** VP-001 can then pass its memory criterion, restoring the
  clean evidence ledger.
- **Negative:** a bounded-history read model may lose the ability to explain
  or replay very old decisions from in-memory state — mitigation: the raw
  ledger is immutable and retained, so deep history remains re-derivable
  (at CPU cost) rather than gone.
- **Negative:** windows/compaction add retention policy and lifecycle
  machinery to read models that are currently stateless projections.
- **Negative:** touches a frozen contract surface (ADR-0003 snapshot
  semantics), which is exactly why this requires an ADR and its own
  re-validation.

## Alternatives Considered

- **Do nothing; accept unbounded growth.** Rejected — the evidence makes
  multi-day/week runs non-viable, and RTT degradation is a real latency
  regression, not a cosmetic one.
- **Change the VP-001 criterion to accept data growth.** Rejected by the
  architecture review board — the criterion is fixed and evidence-based;
  redefining success on the first contradictory result would weaken the
  program.
- **Optimize the demo fixture only.** Rejected — the growth is a property of
  the frozen read-model design, not the demo; a real long-running runtime
  would exhibit the same curve.
- **Pre-compute/store metrics eagerly.** Rejected (as in ADR-0003) — goes
  stale; observability must stay derivable from current knowledge.

## Gate

Status flips from Proposed to Accepted/Rejected only after VP-001 run-004
(24 h) confirms: continued linear RSS, continued RTT growth tracking history,
no crashes/reconnects/sequence gaps. If run-004 contradicts the curve (RSS or
RTT flattening), the proposal is revisited against the new evidence.
