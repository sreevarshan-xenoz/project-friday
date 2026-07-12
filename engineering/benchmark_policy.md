# Benchmark Policy

The benchmark is the project's honesty instrument. Its job is to **describe
reality**, not to be a reward function that clever code can game.

## Invariants

- **Raw MTBF** counts *every* interruption, including ones recovery resolved.
  Recovery is never folded into MTBF.
- Recovery is reported as a *separate* number (Recovery Effectiveness), so a
  recovered failure is visible as both "user succeeded" and "system failed."
- No single metric may be optimized in isolation to hide the others. The four
  headline metrics each describe a different property:
  - Raw MTBF → **Stability**
  - First-Pass Success → **Robustness**
  - Recovery Effectiveness → **Resilience**
  - User Success Rate → **User Experience**
- Mean estimated **Recovery Cost** (ms) is reported per subsystem.

## Benchmark integrity

- The benchmark **definition is never changed to make scores look better.** If a
  metric is wrong, fix the *measurement*, not the *target*. A capability is not
  "qualified" by lowering the bar.
- **Every new capability/subsystem ships with benchmarks and chaos scenarios.**
  No subsystem is merged without the tasks and failure injections that exercise it.
- Runs are comparable: a fixed task set plus committed `benchmarks/<date>.json`
  snapshots make changes auditable run-over-run.

## Regression policy

A **regression** is any of:

- Chaos Raw MTBF drops below the trailing 7-run mean by more than the agreed
  tolerance, **or**
- a subsystem's recovery coverage drops below its prior value, **or**
- any qualification-bar metric falls below target.

On regression, the offending change is **blocked** (or flagged for explicit
sign-off) until root cause is identified. Automated Regression Detection +
Root-Cause Hint (offending commit + reason) is a planned tooling layer; until then
the policy is enforced manually in review.

## Historical snapshots

- Each run writes `benchmarks/<date>.json` (Normal/Chaos Raw MTBF, User Success,
  plus the learned `ConfidenceModel`).
- `run_day(tasks, prior)` seeds confidence from the previous day, so learning
  compounds across runs and progress is auditable.

## Qualification bar

A subsystem is **Qualified** at recovery coverage ≥ 85%. Project qualification
target: Raw MTBF > 100, First-Pass > 90%, Recovery Effectiveness > 85%,
User Success > 99%.

## Future: Regression Detection + Root Cause Hint

Today the benchmark produces numbers. Next, add a layer:

```
Benchmark → Regression Detection → Root Cause Hint
```

Example: Chaos MTBF `10 → 7` with reason `Git cleanup regression` and the
offending commit `abc123`. This turns the benchmark from a scoreboard into a
diagnostic, and automates the regression policy above.
