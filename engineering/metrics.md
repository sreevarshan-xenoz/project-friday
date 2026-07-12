# Metrics

The resilience metrics, and the one we are building toward.

## Current four metrics (each a distinct property)

| Metric                   | Property         | Meaning                                            |
| ------------------------ | ---------------- | -------------------------------------------------- |
| Raw MTBF                 | Stability        | Every interruption counts; recovery ignored.       |
| First-Pass Success       | Robustness       | Succeeded with *no* recovery.                      |
| Recovery Effectiveness   | Resilience       | Of attempts, fraction that resolved.               |
| User Success Rate        | User Experience  | User ultimately got it (incl. recovered).          |

## Precise definitions

Let `N` = total tasks, `I` = interruptions (every failure event, recovery ignored),
`R` = recovery attempts, `R_ok` = attempts that resolved, `F0` = tasks completed
with zero recovery, `U` = tasks where predicted success **or** recovered.

- **Raw MTBF (Stability)** = `N / I`. Every interruption counts. Units: tasks per
  interruption. This is the honest, unfiltered failure rate.
- **First-Pass Success (Robustness)** = `F0 / N`. Success with *no* recovery.
- **Recovery Effectiveness (Resilience)** = `R_ok / R` (defined only when `R > 0`).
  Of attempts, the fraction that resolved.
- **User Success Rate (User Experience)** = `U / N`. Did the user ultimately get it,
  including via recovery.
- **Recovery Cost** = mean `cost_ms()` of *resolved* recoveries, per subsystem.
- **Qualified** = recovery coverage (`R_ok / R`) ≥ 0.85 **sustained** over the
  window.

No single metric may be optimized to hide the others; together they tell the whole
story (see `benchmark_policy.md`).

## Future: Trust Score (~6 months)

The number users actually care about is not MTBF, Recovery, or Success in
isolation. It is **Trust**:

```
Trust =
  0.30 * Stability      (Raw MTBF)
+ 0.25 * Robustness     (First-Pass)
+ 0.20 * Resilience     (Recovery Effectiveness)
+ 0.15 * User Success
+ 0.10 * Predictability (variance / consistency over time)
```

Not exposed internally yet. Eventually this answers the only question that
matters to a daily user: *"Can I trust FRIDAY?"*
