# ADR-0005: Windowed Observability Read Models

**Status:** Accepted (2026-08-06) — gate passed on VP-001 run-005; implementation and re-validation pending
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

**Run-005 confirms both curves through a clean 24 h.** The first
continuous-observation run with 100% coverage (run-004 was INVALID: the host
slept 9 h 41 m mid-run) reaffirmed linear RSS growth — 14.0 → 202.7 MB (peak
392.0 MB), slope 11.9 MB/hr, 44.5 MB per 1,000 ledger records as the ledger
grew 140 → 6,554 records (~267/hr) — and RTT drift of +416.6% relative
(45.4 ms absolute) with hourly p95 reaching 267 ms. A single 2.7 s
disconnect/reconnect (one resync) was recorded as an operational observation,
non-gating: it did not corrupt the experiment (0 sequence gaps, 100% coverage).

**Two symptoms, one root cause:** the cost of observability is
**unbounded in retained execution history** — in both memory and CPU.

This was the first time the Validation Program produced evidence
contradicting the VP-001 hypothesis ("no accumulation effects beyond the
ledger itself"). Per governance (frozen-contract #8), that evidence is the
gate for an architectural change. The proposal was registered (2026-08-03) in
Proposed status pending a 24-hour run of continuous observation; run-005
(2026-08-06) supplied that evidence and the ADR was accepted (see Gate).

## Decision

**Bound the cost of observability with respect to retained execution
history.**

The objective is deliberately NOT "reduce memory usage" — memory is the
visible symptom of a deeper property: the read models' work and footprint
scale with how much history is retained. The decision is to make that scaling
**bounded and explicit** rather than linear-unbounded.

The proposal intentionally does not prescribe the mechanism. Candidate
implementations (to be evaluated during implementation, behind the frozen
interfaces):

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

## Gate (passed — run-005, 2026-08-06)

The 24 h continuous-observation gate passed on run-005 (coverage 100%, VALID).
VP-001 failure criteria are classed so a runtime anomaly cannot accidentally
overrule the architectural evidence (`validation/analyze.mjs`, ADR-0005
governance):

- **decision** — validates the ADR hypothesis directly (memory growth, RTT
  drift). Gating.
- **integrity** — measurement validity; a failure corrupts the experiment
  (observation coverage, sequence gaps, frame loss, process crashes). Gating.
- **operational** — unrelated runtime anomalies (unexpected reconnects, and
  future host power / network events). Recorded and reported, never gating —
  a transport anomaly only matters when it corrupts the experiment, which is
  what `integrity` detects.

run-005 outcome (verdict FAIL on the two decision criteria):

| Class | Criterion | Target | Observed |
| --- | --- | --- | --- |
| decision | memory_growth | < 1 MB/hr | 11.9 MB/hr (14.0 → 202.7 MB; peak 392.0 MB) |
| decision | rtt_drift | < 10% | 416.6% relative (45.4 ms absolute) |
| integrity | frame_loss | 0 | 0 |
| integrity | sequence_gaps | 0 | 0 |
| integrity | process_crashes | 0 | 0 |
| integrity | observation_coverage | ≥ 99% | 100% (VALID) |
| operational | unexpected_reconnects | 0 | 1 (2.7 s, self-healed; non-gating) |

Both curves continued linearly through a full uninterrupted day, so the
unbounded-in-history hypothesis stands and the decision to bound the cost of
observability is **accepted**. The re-validation gate re-opens: once a bounded
read model is implemented, it must pass the same decision criteria on a fresh
24 h run.
