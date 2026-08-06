# MECH-0001: Snapshot Mechanism Evaluation

**Status:** Selected — Phase 3 (prototype) complete: equivalence gate green, benchmark recorded; Phase 4 (re-validation) pending
**Date:** 2026-08-06
**Relates to:** [ADR-0005](../adr/0005-windowed-observability.md) (Accepted) — Windowed Observability Read Models
**Scope:** `friday-experience` read models (`DecisionTimeline`, `TimelineBuilder`, `TimelineStore`), bridge `DemoRuntimeSource` (incremental store; `regenerate` removed)
**Governance:** engineering record, not an ADR. Preserves *why* a particular mechanism was chosen so ADR-0005 stays about the architectural decision. If a better mechanism is found later, this record is revisited — the ADR's decision (observability cost must be bounded) is not reopened.
**Lifecycle:** **Draft** → evaluating candidates / prototype pending → **Selected** → mechanism chosen and implemented → **Superseded** → replaced by a later mechanism. MECH-0001 is currently **Selected** (prototype + equivalence + benchmarks done; Phase 4 re-validation tracked as ADR-0005 validation, not as this mechanism's lifecycle).

## Context — evidence recap

VP-001 run-005 (24 h continuous, 100% coverage) confirmed two O(retained history) curves:

- **RSS:** 14.0 → 202.7 MB, slope 11.9 MB/hr, 44.5 MB per 1,000 ledger records (peak 392.0 MB).
- **RTT:** +416.6% relative drift (45.4 ms absolute), hourly p95 reaching 267 ms.

Both trace to the same read-model path:

- Every cycle `DemoRuntimeSource::regenerate()` (removed by this mechanism) rebuilt the experience and reputation stores from the **full ledger**, then `TimelineBuilder::build_all()` (`lib.rs:1174`) reconstructed **all** timelines; each timeline cloned its `plan` / `outcome` / `record` / `experience` / `reputation` / `calibration` / `explanation`.
- Every emission `dashboard()` re-iterates **all** timelines (policy-factor count, confidence sum, recent) — O(history) per frame.
- The bridge main loop serializes this work ahead of pong replies, so RTT inflates via head-of-line blocking. The collector is constant-work; this is not a measurement artifact.

## Phase 1 — Evaluation criteria (frozen)

Decided before any code. "Whichever is easiest to code" is not a criterion.

| Criterion | Weight | Measurement | Instrument |
| --- | --- | --- | --- |
| RSS growth reduction | High | MB/hr (target < 1 MB/hr) | VP-001 analyzer |
| RTT stability | High | % drift (target < 10%) | VP-001 analyzer |
| Read complexity | High | O(?) per read | code review / profile |
| Write complexity | Medium | additional work per append | profile |
| Storage independence | High | no storage-specific types / assumptions | code review |
| ADR-0003 compatibility | High | timeline stays a point-in-time snapshot; pure projection | code review + equivalence tests |
| ADR-0006 compatibility | High | uses only the `State` contract surface | design review |
| Implementation complexity | Medium | estimated effort vs a `regenerate()` rewrite | estimate |
| Migration risk | Medium | data compatibility; existing runs/tests unaffected | test suite |

ADR-0003 compatibility is a hard constraint from the ADR-0005 decision: "read models remain pure projections, 'history is immutable,' and explanation/metrics stay valid against current knowledge."

## Phase 2 — Candidate evaluation

### C2. Retention window

- What it is: keep timelines/snapshots for a bounded horizon; older history compressed or dropped from derived models (the raw immutable ledger is never deleted).
- Pros: simple; low risk; RSS bounded by construction.
- Cons: does **not** bound per-frame CPU within the window (RTT still grows while inside it); throws away in-memory explain/replay of old decisions (re-derivable from the ledger at CPU cost); mitigates the symptom without changing the O(history) cost shape.
- **Verdict: fallback only** — an acceptable interim cap, not the answer.

### Architectural constraint: ADR-0003 forces ownership over references

The strongest single reason the ID-based candidate (C3) is **deferred, not rejected**, is an architectural constraint rather than an implementation detail:

```
ADR-0003
    ↓
Timeline owns immutable point-in-time snapshots ("snapshots, not references")
    ↓
ExperienceSnapshotId (timeline holds an id, fetches the snapshot from a store at read time)
    ↓
Only valid if the backing store is versioned — otherwise the timeline no longer
owns a point-in-time snapshot: a later store update would change what the
timeline "sees"
    ↓
ADR-0006 (versioned, storage-independent persistent layer)
```

C3's read-model shape is exactly right for ADR-0006's persistent layer, but it cannot be used for this in-memory fix without first building the versioned store it depends on. C4 (`Arc<Snapshot>`) delivers the same ownership guarantee in memory today, which is why C4 is taken and C3 is deferred.

### C3. `ExperienceSnapshotId → store` lookup

- What it is: the timeline holds an id; the snapshot is fetched from a (versioned) store at read time.
- Pros: small per-timeline footprint; fits a storage abstraction; scales.
- Cons: **breaks ADR-0003's "snapshots, not references"** — a live-store lookup means the timeline no longer owns a point-in-time snapshot (a versioned store could restore the guarantee, but that is a bigger change); adds read latency and more lookups; the dominant cost (Experience versions) is deferred to a store, not removed.
- **Verdict: deferred — strong candidate for the future persistent layer (ADR-0006), not for this in-memory fix.** The dependency chain that makes a non-versioned id-lookup violate ADR-0003 is spelled out in the [architectural constraint](#architectural-constraint-adr-0003-forces-ownership-over-references) above; C4 achieves the same guarantee in memory, so C3 waits for the versioned store.

### C4. `Arc<ExperienceSnapshot>` sharing

- What it is: timeline snapshot fields become `Arc<...>`; timelines share immutable values instead of cloning them.
- Pros: tiny diff; **semantics-preserving** — an immutable `Arc` *is* a point-in-time snapshot (live store updates create a new value; old timelines keep theirs), so ADR-0003 is preserved; eliminates repeated deep clones (allocation CPU) of the dominant field; storage-agnostic.
- Cons: steady-state RSS can still grow O(history) when values diverge per key each decision (the Arc keeps every version alive); does not change `regenerate()`/`dashboard()` re-iteration, so the RTT curve is unaffected on its own.
- **Verdict: take** — the memory/clone half of the fix.

### C5. Delta-only projections

- What it is: the timeline keeps only the incremental delta from its predecessor; reconstruction folds the deltas.
- Pros: better asymptotics (O(delta) per append); less copying.
- Cons: more bookkeeping; correctness harder (the fold must be exact); every read becomes a fold → read complexity rises; the largest behavioral change across the frozen read-model surface.
- **Verdict: hold** — revisit only if the combined fix (C4 + C6) under-delivers on RSS.

### C6. Lazy / incremental regeneration

- What it is: replace full `regenerate()` with append-time incremental update — one timeline per new decision, only the affected experience/reputation entries updated; `dashboard()` consumes cached/running aggregates instead of re-summing all timelines.
- Pros: **directly attacks the observed O(history) per-frame behavior** (both the cycle rebuild and the per-emission re-iteration); matches the evidence — each append is a small delta.
- Cons: introduces incremental state (cached aggregates must be invalidated correctly); `MetricsEngine::summarize` is stateless by design, so running aggregates must be proven equivalent to `summarize(all timelines)`.
- **Verdict: investigate first** — the half of the fix that flattens RTT.

### C7. Combined path — recommended prototype

The evidence shows two orthogonal costs; no single candidate fixes both:

- RTT ← full `regenerate()` + full `dashboard()` re-iteration → **C6 incremental**
- RSS ← per-timeline full clones → **C4 Arc sharing** (+ drop redundant clones where safe)

```
Ledger append (one record)
        ↓
Append one timeline (Arc-shared snapshots)      ← C4 + C6
        ↓
Update only the affected experience/reputation  ← C6
        ↓
Recompute only the metrics that changed         ← C6 (running aggregates)
        ↓
dashboard() reads cached aggregates             ← C6
        ↓
State (ADR-0006 builds on top, storage-independent)
```

Storage independence: the mechanism uses no storage-specific types — pure in-memory read-model mechanics — so it stays compatible with ADR-0006's storage-independent `State` contract. The `ExperienceSnapshotId → store` idea is deferred to ADR-0006's implementation, where a versioned store can provide the same point-in-time guarantee the in-memory `Arc` provides today.

## Implementation decision — point-in-time snapshot semantics (canonical)

> **Point-in-time snapshot semantics are adopted as the canonical behavior because they are consistent with ADR-0003, the existing API documentation, and immutable snapshot semantics. Final-state replay is treated as an implementation artifact of the previous full-regeneration algorithm and is intentionally removed.**

Rationale, in short:

- ADR-0003 requires the timeline to own an immutable, point-in-time snapshot that survives store changes (proven by `timeline_is_a_snapshot_that_survives_store_changes`).
- The API documents "Experience snapshot ... at build time."
- The previous `build_all()` (rebuild-everything) projected the **final** store value into every timeline: for a given (capability, operator) key, `A → 0.78, B → 0.78, C → 0.78`. That is not history — it is the present projected backwards, and it destroys "what did the system believe when this decision was made?"
- Implementation disagreed with both the architectural contract and the API docs; the implementation is not the specification.
- A genuinely incremental append (C6) *naturally* produces point-in-time snapshots, so the optimized path and the corrected reference agree on the same semantics.

Correction to the equivalence gate (was "`build_all()` byte-identical"):

```
incremental == build_all_pti()
```

where `build_all_pti()` is a pure point-in-time replay of the ledger (one record at a time, snapshotting the stores after each). This validates the optimized implementation against the same semantics it implements, not against behavior already determined to be incorrect.

**Semantic equality** for the equivalence test — `must match`: `DecisionRecord`, `Experience`, `Reputation`, `Calibration`, `Outcome`, `Plan`. `may differ`: `Reputation.last_updated_ms` (wall-clock by design — deterministic projections promise reproducibility on derived values, not timestamps). The equality helper documents this split.

**Regression test (executable spec)**: ledger `A → experience 10, B → experience 20, C → experience 30` must yield `Timeline A == 10, Timeline B == 20, Timeline C == 30`. The old final-state implementation fails this immediately; the point-in-time implementation passes.

## Phase 3 — Prototype (complete)

Smallest implementation that demonstrates the path, behind the frozen interfaces:

1. `TimelineBuilder` gained `build_all_pti()` (`lib.rs:1198`) — a pure point-in-time replay of the ledger (one record at a time, snapshotting the stores after each). `build_all()` (`lib.rs:1174`) is kept as-is: its contract is "project the given stores onto every record" (relied on by existing metrics tests), and the runtime no longer uses it for live state.
2. `DecisionTimeline` snapshot fields are `Arc`-shared (`record` shared with the ledger, `experience`/`reputation`/`calibration` shared with the stores); `ReputationEngine::update_operator` updates only the affected operator. Equivalence tests assert `incremental == build_all_pti()` under the semantic-equality rule above.
3. `TimelineStore` (`lib.rs:1789`) + `RunningMetrics` (`lib.rs:1513`) maintain persistent stores + running aggregates; `dashboard()` reads them (O(1) per frame instead of O(history)).
4. Regression test `timeline_snapshots_reflect_state_at_decision_time` (`lib.rs:3170`, A→10, B→20, C→30); incremental-vs-pti (`lib.rs:3199`); `running_metrics_matches_summarize` (`lib.rs:3242`); wire-DTO equivalence suite (`bridge/tests/equivalence.rs`); scaling benchmark (`lib.rs:3300`, `#[ignore]`d).
5. Measured against the run-005 baseline (below).

### Measured results (release build, global counting allocator)

`#[ignore]`d scaling benchmark `incremental_vs_full_rebuild_scaling`: the old demo flow (per-append `ExperienceEngine::update` + `ReputationEngine::update` + `build_all` + `summarize`, replayed into the same ledger/stores) vs the incremental `TimelineStore` (one append each), wall time + allocation count:

| n | per-append rebuild (ms) | incremental (ms) | time ratio | rebuild allocs | incremental allocs | alloc ratio |
| --- | --- | --- | --- | --- | --- | --- |
| 2,000 | 5,515 | 6.0 | 919x | 56,500,289 | 64,378 | 878x |
| 4,000 | 23,690 | 12.4 | 1,909x | 225,805,642 | 128,779 | 1,753x |
| 8,000 | 102,332 | 25.6 | 3,994x | 902,820,350 | 257,580 | 3,505x |

The ratio grows superlinearly with n (919 → 1,909 → 3,994): the old per-append path is O(n²) in both time and allocations, the incremental path is O(n) (6.0 → 12.4 → 25.6 ms ≈ 2x per 2x of records). Incremental allocations per append are constant (~32 per record: one timeline, one record Arc clone, one reputation update, one metrics tick).

Equivalent state was asserted in the same run (final timeline count and metric totals match across both paths), and the wire-DTO equivalence suite (`bridge/tests/equivalence.rs`) additionally asserts `incremental == build_all_pti()` at the DTO level.

**Confirmed scope boundary (measured 2026-08-06, not an open question):** the wire protocol sends the **full owned timeline + ledger lists on every new-record delta**, and the connection task serializes + writes that frame in the same `tokio::select!` loop that answers pings (so pong RTT includes the O(history) frame encode+write). The console (`mergeSlices`) and the validation collector both require full lists (replace semantics, `.length` used for counts). Therefore after the read-model work is flattened, the RTT residual is **wire emission**, which is protocol-level and out of scope for this read-model mechanism. Mitigations (writer-task decoupling from the pong path, or append-only delta semantics) are separate decisions — an ADR-0005 threats note / future ADR.

Gate: proceed to full migration only if a 2 h soak shows the RSS curve flat and `incremental == build_all_pti()` holds. RTT is reported honestly with the wire-emission residual separated from read-model cost.

## Phase 4 — Re-validation (protocol-identical)

After migration, re-run VP-001 with the **same** protocol as run-005: 24 h continuous, seed 42 / count 140, interval 5 s, coverage ≥ 99%, same thresholds, same analyzer, same governance. Decision criteria (memory growth < 1 MB/hr, RTT drift < 10%) must PASS. INV-001 (reconnect) and timing capture are tracked independently and do not gate this run.

## Log

- 2026-08-06 — Phase 1 criteria frozen; Phase 2 evaluation written; Phase 3 prototype pending.
- 2026-08-06 — Added architectural-constraint section (ADR-0003 forces ownership over references → why C3 is deferred, not rejected); adopted Draft/Selected/Superseded lifecycle; Phase 3 prototype started.
- 2026-08-06 — Adopted point-in-time snapshot semantics as canonical (final-state replay = artifact of the previous full-regeneration algorithm, intentionally removed); equivalence gate redefined as `incremental == build_all_pti()` with explicit semantic equality (must-match vs `last_updated_ms` may-differ); regression test `timeline_snapshots_reflect_state_at_decision_time` added to the prototype plan.
- 2026-08-06 — Prototype complete: `DecisionTimeline` Arc-snapshot fields, `ReputationEngine::update_operator`, `TimelineStore` + `RunningMetrics`, `build_all_pti()`, `DemoRuntimeSource` switched to the incremental store (`regenerate` removed), wire-DTO Arc handling + `bridge/tests/equivalence.rs`, Arc-shared `DecisionLedger`. Equivalence gate green across all suites.
- 2026-08-06 — Scaling benchmark recorded (release, counting allocator): old per-append rebuild O(n²) vs incremental O(n); at n=8,000 the incremental path is ~4,000x faster and ~3,500x fewer allocations. Benchmark measures the real old flow (per-append full regeneration), not a single cold rebuild. Mechanism promoted to **Selected**; Phase 4 re-validation tracked as ADR-0005 validation.
