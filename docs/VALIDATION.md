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

**Status:** proposed

- **Goal:** Can FRIDAY run continuously without degrading?
- **Hypothesis:** The read-model/derived architecture has no accumulation
  effects beyond the ledger itself.
- **Method:** Continuous execution of mixed workloads for fixed durations.
- **Metrics:** memory growth, ledger growth, calibration stability, replay
  correctness, prediction drift, latency over time.
- **Runs:** 24 hours → 72 hours → 1 week.
- **Expected Result:** bounded growth; calibration converges and stays within
  clamps; replay results stay consistent; latency flat.

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

**Status:** proposed

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
| VP-001 | Long-running cognition  | proposed  | —        | —           |
| VP-002 | Prediction quality      | proposed  | —        | —           |
| VP-003 | Replay correctness      | proposed  | —        | —           |
| VP-004 | Scaling                 | proposed  | —        | —           |
| VP-005 | Algorithm competitions  | proposed  | —        | —           |

## Evidence → ADR gate

An architectural change may only proceed via a new ADR, and that ADR must cite
the VP evidence that motivated it. Governance summary:

- **Architecture:** frozen — changing any contract requires a new ADR.
- **Algorithms:** compete within stable interfaces (no ADR needed).
- **Evidence:** produced by this program.
- **Evolution:** only evidence from this program can justify an architecture
  change.
