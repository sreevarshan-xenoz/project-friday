# FRIDAY Validation Program

The architecture is frozen (see `docs/adr/`). This document is the scientific
protocol for proving it works — not another architecture document.

The question has changed:

> Before: *Can we build this?*
> Now: **Does this architecture actually work?**

## Operating rule

> **No architectural changes without evidence from the Validation Program.**

1. Architecture is frozen.
2. Algorithms compete within their stable interfaces.
3. Validation produces evidence.
4. Only evidence justifies architectural evolution.
5. If the architecture changes, it gets a new ADR.

## Experiment protocol

Every validation program records the same eight fields so results are
comparable over time:

```
Goal
Hypothesis
Method
Metrics
Dataset
Expected Result
Actual Result
Decision
```

`Expected Result` is written before the run. `Actual Result` and `Decision`
are filled in afterwards. A run that surprises us is a finding, not a failure.

## Validation programs

### VP-001 — Long-running cognition

**Status:** runs started (2h complete — FAIL: memory_growth)

- **Goal:** Can FRIDAY run continuously without degrading?
- **Hypothesis:** The read-model/derived architecture has no accumulation
  effects beyond the ledger itself.
- **Method:** Continuous execution of mixed workloads for fixed durations.
- **Instrument:** `validation/` — unattended runner + collector + analyzer
  (`node validation/friday-validate.mjs --duration 8h --interval 5s`). It
  starts the bridge + console client, appends a JSONL sample every interval,
  and ends with a PASS/FAIL verdict against the VP-001 criteria (memory slope,
  RTT drift, frame loss, sequence gaps, reconnects, crashes).
- **Metrics:** memory growth, ledger growth, calibration stability, replay
  correctness, prediction drift, latency over time.
- **Runs:** 2 hours (done) → 8 hours → 24 hours → 72 hours → 1 week.
- **Expected Result:** bounded growth; calibration converges and stays within
  clamps; replay results stay consistent; latency flat.
- **Actual Result (run-002, 2026-08-03, 120 min, seed 42/count 140, git
  3d72910):** FAIL on `memory_growth` (15.5 MB/hr vs < 1 MB/hr target); every
  other criterion PASS (0 sequence gaps, 0 unexpected reconnects, 0 error
  frames, RTT drift ±2 ms). Memory grew 13.9 → 46.3 MB, peak 53.8 MB. Growth
  is **linear, not a leak**: ~2–4 MB per 74-record (15-min) bucket, ~52 MB per
  1,000 ledger records, and the per-1,000-record slope is *not* increasing
  (first-half ≈ 66 MB/1,000, second-half ≈ 36 MB/1,000 — warm-up noise, not
  acceleration). Driver: `DemoRuntimeSource` regenerates full timelines each
  cycle and every `DecisionTimeline` carries full experience/reputation/
  calibration store snapshots (frozen Phase 6B design — snapshots, not
  references); experience samples grew 1,960 → 56,359 (~91/decision). The
  transport path is healthy: delta frames stay flat (43.4 → 43.6 KB), snapshot
  grows to 1,377.5 KB (full-slice read models), 0 gaps/errors. Evidence:
  `artifacts/validation/run-002/{samples.jsonl, report.md, report.json}`
  (gitignored).
- **Decision (Finding VP-001-1):** bridge memory is unbounded *linear* data
  growth — the read models hold everything forever. At the demo's ~300
  decisions/hr that projects to ~370 MB at 24 h and multi-GB at 1 week, so
  long runs are not viable without a retention policy. Not an ADR yet — the
  call is the user's: (a) accept + bound run lengths, (b) raise an ADR for
  read-model retention/windowing (touches the frozen TimelineBuilder/store
  design), or (c) continue the ladder (8 h → 24 h) to separate the memory
  signal from any other long-run degradations first.

### VP-002 — Prediction quality

**Status:** proposed

- **Goal:** Quantify *how wrong* the CostModel is, not whether it works.
- **Hypothesis:** Estimation error converges (shrinks) as experience
  accumulates.
- **Method:** Record `estimated` vs `actual` for every execution; build the
  distribution of error.
- **Metrics:** error distributions for latency, cost, success probability,
  confidence; convergence over time.
- **Expected Result:** mean absolute error decreasing, variance narrowing,
  success bias approaching zero.

### VP-003 — Replay correctness

**Status:** proposed

- **Goal:** Turn replay into a regression suite over decision history.
- **Hypothesis:** Replay reflects real learning — decisions change only when
  evidence changed.
- **Method:** Randomly sample historical decisions, replay them against the
  current stores, aggregate the report.
- **Metrics:** decision unchanged %, operator changed %, prediction
  improvement %, explanation changed %.
- **Expected Result:** high stability on stable evidence; change tracks
  experience/reputation/calibration movement.

### VP-004 — Scaling

**Status:** first run complete (finding fixed)

- **Goal:** Benchmark the *architecture*, not execution — how the read models
  behave as the ledger grows.
- **Hypothesis:** Timeline build, replay, and metrics generation are linear in
  ledger size with bounded constant factors.
- **Method:** Generate ledgers of increasing size and measure read-model
  operations; produce growth curves, not just averages.
- **Datasets:** 10 / 100 / 1,000 / 10,000 / 100,000 `DecisionRecord`s.
- **Metrics:** timeline build time, replay time, metrics generation, memory
  usage, ledger size, experience update speed.
- **Expected Result:** linear growth curves with no super-linear surprises.
- **Actual Result (run 1):** all read models linear with sub-linear per-record
  cost (fixed overhead dominates at small n). One super-linear surprise found
  and fixed: `TimelineBuilder::build_all()` was O(n²) — now O(n), no interface
  change (evidence: `benchmarks/VP-004/README.md`, `benchmarks/VP-004/results/`).
  49 tests pass, clippy clean.
- **Decision:** accept linear-growth hypothesis; keep benchmark as regression
  guard (rerun on any change to read models). Replays must remain sampled —
  full replay of every timeline is the only legitimately super-linear
  operation and is out of scope.

### VP-005 — Algorithm competitions

**Status:** proposed

- **Goal:** Compete learning algorithms behind the frozen `CostModel`
  interface.
- **Hypothesis:** The interface is sufficient — any estimator that fits it can
  be evaluated without touching the rest of the system.
- **Method:** Replay historical ledgers under each estimator
  (CostModel V1 → V2 → Bayesian → ML → hybrid); compare outcomes.
- **Metrics:** VP-002 + VP-003 metrics per estimator.
- **Expected Result:** interchangeable estimators; results comparable; no
  interface changes required.

## Status ledger

| VP    | Program                 | Status    | Last run | Result link |
| ----- | ----------------------- | --------- | -------- | ----------- |
| VP-001 | Long-running cognition  | 2h run (FAIL: memory) | 2026-08-03 | [run-002](..\artifacts\validation\run-002\report.md) |
| VP-002 | Prediction quality      | proposed  | —        | —           |
| VP-003 | Replay correctness      | proposed  | —        | —           |
| VP-004 | Scaling                 | first run | 2026-08-01 | [results](../benchmarks/VP-004/README.md) |
| VP-005 | Algorithm competitions  | proposed  | —        | —           |

## Evidence → ADR gate

An architectural change may only proceed via a new ADR, and that ADR must cite
the VP evidence that motivated it. Governance summary:

- **Architecture:** frozen — changing any contract requires a new ADR.
- **Algorithms:** compete within stable interfaces (no ADR needed).
- **Evidence:** produced by this program.
- **Evolution:** only evidence from this program can justify an architecture
  change.
