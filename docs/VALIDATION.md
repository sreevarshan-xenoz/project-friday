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

**Status:** ladder in progress — run-002 (2h) = **VP-001 Baseline**; run-003 (8h) done; run-004 (24h) = **INCONCLUSIVE** (host sleep, 9.7h observation gap); run-005 (24h continuous, **first protocol-compliant VP-001 execution**) = **CONFIRMED both curves** → ADR-0005 **ACCEPTED**; bounded read-model mechanism implemented (MECH-0001 C4+C6); **run-006 (re-validation) = FAIL on both decision criteria** (memory 9.8 MB/hr, RTT +394%), all integrity PASS at 100% coverage — alternatives under evaluation

```
Evidence supports:

✓ Transport stability (0 gaps / 0 errors / 0 reconnects through 8h)
✓ Replay stability
✓ Protocol stability
⚠ Read-model memory proportional to retained history
  (~41 MB/1,000 records; per-record cost DECREASING with scale — not a leak)
⚠ RTT degrades with retained history (mean 1.1→12 ms, p95 13→103 ms over 8h)
  — per-frame read-model work is O(history)
✗ Memory target unmet (12.1 MB/hr)
✗ RTT drift target unmet (149.5%)
```

- **Goal:** Can FRIDAY run continuously without degrading?
- **Hypothesis:** The read-model/derived architecture has no accumulation
  effects beyond the ledger itself.
- **Method:** Continuous execution of mixed workloads for fixed durations.
- **Instrument:** `validation/` — unattended runner + collector + analyzer
  (`node validation/friday-validate.mjs --duration 8h --interval 5s`). It
  starts the bridge + console client, appends a JSONL sample every interval,
  and ends with a PASS/FAIL verdict against the VP-001 criteria (memory slope,
  RTT drift, frame loss, sequence gaps, reconnects, crashes). Criteria are
  classed (see the Gate in ADR-0005): **decision** and **integrity** gate the
  verdict; **operational** anomalies (e.g. reconnects) are recorded and
  reported but never gate the run.
- **Metrics:** memory growth, ledger growth, calibration stability, replay
  correctness, prediction drift, latency over time, and — from run-005 onward
  — server-side read-model generation timings (`dashboard_generation_ms`,
  `timeline_regeneration_ms`; bridge writes `TIMING` lines to `bridge.log`,
  analyzer correlates them with the RTT curve as the causal link for
  Finding VP-001-2). Instrument landed in git `0e6df06`+; **run-004 was
  launched before it** and is therefore uninstrumented (no `TIMING` lines).
- **Runs:** 2h ✅ baseline (run-002) → 8h ✅ (run-003) → 24h **continuous**
  observation (run-004: INCONCLUSIVE; run-005: ✅ first protocol-compliant,
  ADR-0005 **ACCEPTED**) → implement bounded read models → re-validate on a
  fresh 24h run (72h/1w only after the architecture question is resolved).
- **Expected Result:** bounded growth; calibration converges and stays within
  clamps; replay results stay consistent; latency flat.
- **Actual Result (run-002 — VP-001 Baseline, 2026-08-03, 120 min, seed
  42/count 140, git 3d72910):** FAIL on `memory_growth` (15.5 MB/hr vs < 1
  MB/hr target); every other criterion PASS (0 sequence gaps, 0 unexpected
  reconnects, 0 error frames, RTT drift ±2 ms). Memory grew 13.9 → 46.3 MB,
  peak 53.8 MB. Growth is **linear, not a leak**: ~2–4 MB per 74-record
  (15-min) bucket, ~52 MB per 1,000 ledger records, and the per-1,000-record
  slope is *not* increasing (first-half ≈ 66 MB/1,000, second-half ≈ 36
  MB/1,000 — warm-up noise, not acceleration — classic leaks accelerate
  20/35/60/95/150, this is ~40/41/39/38/37). Driver: `DemoRuntimeSource`
  regenerates full timelines each cycle and every `DecisionTimeline` carries
  full experience/reputation/calibration store snapshots (frozen Phase 6B
  design — snapshots, not references); experience samples grew 1,960 → 56,359
  (~91/decision). The transport path is healthy: delta frames stay flat (43.4
  → 43.6 KB), snapshot grows to 1,377.5 KB (full-slice read models), 0
  gaps/errors. Evidence: `artifacts/validation/run-002/{samples.jsonl,
  report.md, report.json}` (gitignored).
- **Actual Result (run-003, 2026-08-03, 480 min, same seed/count, git
  c0a147b):** FAIL on `memory_growth` (12.1 MB/hr) AND `rtt_drift` (±5.7 ms,
  149.5%); transport still clean (0 gaps, 0 reconnects, 0 errors, delta frames
  flat 43.4 → 42.1 KB). Memory 14.0 → 113.3 MB, peak 155.8 MB; **per-1,000-
  record cost fell to 41.0 MB (from 52.0 in run-002) — per-record cost
  decreases with scale, the clearest "not a leak" signal yet**. New signal
  **Finding VP-001-2**: RTT degrades monotonically with retained history —
  hourly mean 1.1 → 3.3 → 4.7 → 6.3 → 8.0 → 8.8 → 9.7 → 12.0 ms and hourly
  p95 13 → 31 → 48 → 59 → 76 → 87 → 96 → 103 ms as the ledger grew 437 →
  2500 (~295/hr). Same root cause as VP-001-1: the source's `regenerate()`
  (every 12 s cycle) and `dashboard()` (every emission) iterate ALL timelines,
  so per-frame CPU is O(retained history); the bridge's main loop delays pong
  replies during that work → RTT inflates. The collector is constant-work
  (async, no blocking), so this is bridge-side head-of-line blocking, not a
  measurement artifact. Ledger 140 → 2500, experience samples 1,960 → 650,864
  (81,115/hr). Evidence: `artifacts/validation/run-003/` (gitignored).
- **Actual Result (run-004, 2026-08-03→04, 24h intended, seed 42/count 140):
  INCONCLUSIVE — not a FRIDAY finding.** The host entered sleep state for
  9h 41m (2026-08-03 19:35 → 2026-08-04 05:17 UTC) plus a 6.7 min hiccup
  after wake, so the experiment lost observability for ~40% of its intended
  duration. The bridge never crashed, disconnected, or produced a sequence
  gap (`connection_starts=1`, `connection_errors=0`, `sequence_gaps=0` in
  every observed sample) — the run clock stayed continuous and the collector
  resumed on wake. Observed segments (h 0–1.9 and h 11.6–24) are continuous
  and consistent with the hypothesis (linear RSS 13.9 → 184.5 MB at 42.3
  MB/1,000 records — matching run-003's 41.0; RTT mean 14.6 ms tracking the
  ledger 140 → 4,150), but the criterion was **24 hours of continuous
  observation**, and 14.2h observed (59.2% coverage) is not the experiment
  that was designed. Per the coverage gate this run is INVALID regardless of
  its curves.

  **Reason for INCONCLUSIVE:** the validation protocol evolved during this
  run. The observation-coverage criterion (and its watchdogs) was introduced
  *after* run-004 exposed host-sleep blind spots — this run was not executed
  under the protocol it would later be judged by. run-004 is therefore
  archived as INCONCLUSIVE, not rejected for a defect: the experiment itself
  revealed a weakness in the validation framework, and the framework was
  improved before making any architectural decision.

  Evidence: `artifacts/validation/run-004/` (gitignored). Mitigation for
  run-005: sleep prevention for the run window + observation coverage and
  watchdogs in the harness (`validation/`, instrumented this entry).
- **Actual Result (run-005, 2026-08-05→06, 1440 min continuous, seed 42/count
  140, git 87fc9ff5):** the first fully protocol-compliant 24 h run —
  observation coverage **100% VALID** (largest gap 0 s; 17,245 samples at
  5.0 s). Verdict FAIL on the two **decision** criteria only: `memory_growth`
  11.9 MB/hr (14.0 → 202.7 MB, peak 392.0 MB, 44.5 MB/1,000 records; ledger
  140 → 6,554, ~267/hr) and `rtt_drift` +416.6% relative (45.4 ms absolute,
  hourly p95 reaching 267 ms) — both curves held **linearly** through the full
  day, confirming VP-001-1 and VP-001-2. Every **integrity** criterion PASS
  (0 frame loss, 0 sequence gaps, 0 crashes, 100% coverage; 87,067 frames, 0
  errors). One **operational** observation, non-gating: a single 2.7 s
  disconnect/reconnect after 2 connects (1 resync, 0 gaps) — recorded, not a
  defect; it did not corrupt the experiment (coverage stayed 100%).
- **Gate outcome (2026-08-06):** run-005 satisfied the 24 h
  continuous-observation gate (coverage ≥ 99%). ADR-0005 flipped **Proposed →
  Accepted**. The criteria governance split (decision / integrity /
  operational) is documented in the ADR Gate and enforced by
  `validation/analyze.mjs` — the reconnect no longer fails the run, it is
  recorded as an operational observation.
- **Actual Result (run-006, 2026-08-06→07, 1440 min continuous, seed 42/count
  140, git ac0069f1158):** re-validation after the bounded read-model
  mechanism (MECH-0001 C4 `Arc`-shared snapshots + C6 incremental
  `TimelineStore`; `regenerate` removed). Observation coverage **100% VALID**
  (largest gap 0 s; 17,245 samples at 5.0 s). Transport: 87,594 frames,
  216.0 GB received (2,441.7 KB/s), 0 sequence gaps, 1 connection start, 0
  unexpected reconnects, 0 error frames, 0 resyncs. Verdict FAIL on the two
  **decision** criteria only: `memory_growth` 9.8 MB/hr (14.4 → 246.6 MB, peak
  370.8 MB, 36.5 MB/1,000 records; ledger 140 → 6,597, ~269/hr; experience
  samples 1,050 → 2,271,254, ~94,596/hr) and `rtt_drift` +394.4% relative
  (43.0 ms absolute; mean 32.4 ms, p50 0 ms, p95 264 ms, max 1,801 ms). Every
  **integrity** criterion PASS (0 frame loss, 0 sequence gaps, 0 crashes, 100%
  coverage). Read-model generation timings: `dashboard_generation` 64,580
  samples, mean 0.184 ms, p95 0.501 ms, first half 0.175 ms → second half
  0.193 ms (+10.7% drift); `timeline_regeneration` absent (no `TIMING` lines —
  no per-cycle rebuild). Operational observations: none. Evidence:
  `artifacts/validation/run-006/` (gitignored). Analysis in MECH-0001 Phase 4.
- **Decision (Finding VP-001-1 + VP-001-2, path: (c) then (b)):** the 8h run
  answered the ladder's questions — slope stays linear (yes, per-record cost
  is *falling*), no allocator/fragmentation blowup (RSS tracks history
  linearly), but **serialization/frame CPU cost DOES grow with history**
  (RTT). Two symptoms, one root cause: **read-model work and memory are both
  proportional to retained history**. That is exactly what a windowed
  observability design addresses. Continue the ladder to 24h **of continuous
  observation** (run-004 was INCONCLUSIVE on the coverage gate; run-005 is the
  deciding run) to confirm both curves hold at ~7,200 records and no new
  failure mode appears; then open **ADR-0005 — Windowed Observability Read
  Models** (NOT "fix a memory leak" — there is no leak). Architectural options
  noted, not yet evaluated: (1) timeline → `ExperienceSnapshotId` → store
  lookup, (2) timeline → `Arc<ExperienceSnapshot>`, (3) timeline keeps only
  the delta. Evaluation happens only after the 24h run, per Evidence → ADR →
  Architecture change. **run-005 (2026-08-06) satisfied the
  continuous-observation requirement** — both curves held linearly through a
  full 24 h at 100% coverage — and **ADR-0005 flipped Proposed → Accepted**.

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
| VP-001 | Long-running cognition  | baseline + 8h + 24h ×3 (run-004 INCONCLUSIVE, host sleep; run-005 CONFIRMED → ADR-0005 accepted; run-006 FAIL on decision criteria, integrity clean) | 2026-08-07 | [run-002 baseline](..\artifacts\validation\run-002\report.md) · [run-003](..\artifacts\validation\run-003\report.md) · [run-004](..\artifacts\validation\run-004\report.md) · [run-005](..\artifacts\validation\run-005\report.md) · [run-006](..\artifacts\validation\run-006\report.md) |
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
