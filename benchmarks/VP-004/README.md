# VP-004 — Scaling (Evidence)

Experiment record for VP-004 per `docs/VALIDATION.md`.

- **Goal:** Benchmark the *architecture*, not execution — how the read models
  behave as the ledger grows.
- **Hypothesis:** Timeline build, replay, and metrics generation are linear in
  ledger size with bounded constant factors.
- **Method:** Generate deterministic synthetic ledgers of increasing size and
  measure each read-model operation; produce growth curves, not just averages.
- **Datasets:** 10 / 100 / 1,000 / 10,000 / 100,000 `DecisionRecord`s.
- **Metrics:** ledger append, experience update, reputation update, timeline
  build, metrics generation, replay (sampled subset).

## How to run

```
cargo run -p friday-experience --example vp004
```

Writes one JSON result file per dataset size into `results/` and prints a
console table with per-record linearity ratios (≈1.0 = linear).

## Result — run 1 (2026-08-01)

| records | append | experience | reputation | timeline | metrics | replay/tl |
| ------: | -----: | ---------: | ---------: | -------: | ------: | --------: |
| 10      | 208 µs | 177 µs     | 72 µs      | 306 µs   | 36 µs   | 22.1 µs   |
| 100     | 255 µs | 106 µs     | 10 µs      | 1,192 µs | 151 µs  | 18.8 µs   |
| 1,000   | 1,611 µs| 1,757 µs  | 41 µs      | 9,885 µs | 810 µs  | 10.2 µs   |
| 10,000  | 16,890 µs| 10,315 µs | 23 µs     | 113,780 µs | 8,635 µs | 11.2 µs |
| 100,000 | 218,913 µs| 107,546 µs | 25 µs    | 939,916 µs | 93,131 µs | 12.7 µs |

Per-record ratios (large/small, ≈1.0 linear):

| append | timeline | metrics | replay/tl |
| -----: | -------: | ------: | --------: |
| 0.105  | 0.307    | 0.259   | 0.573     |

Per-record cost *decreases* with size for every read model (constant fixed
overhead dominates at small n). No super-linear growth. 100k timeline build
completes in ~0.94 s in an unoptimized debug build.

## Findings & decisions

### Finding VP-004-1 — `TimelineBuilder::build_all()` was O(n²)

First run timed out at 100k records. Root cause: `build_all()` collected every
`CorrelationId`, then called `build(cid)` per id; `build()` scans the ledger
with `iter().find(...)` → O(n²) total.

**Decision:** Algorithmic improvement within the frozen interface (method
signature unchanged): `build_all()` now indexes the ledger once into a
`HashMap<CorrelationId, &DecisionRecord>` and reconstructs each timeline from
its record. This is an algorithm change behind a stable interface — no ADR
required. Verified: 49 experience tests pass, clippy clean.

This is exactly the class of hidden super-linear surprise VP-004 was designed
to detect, and the evidence gate worked as intended.

## Evidence files

- `results/10.json` … `results/100000.json` — per-size measurements (data, not
  screenshots).
- Reference: `docs/VALIDATION.md` status ledger.
