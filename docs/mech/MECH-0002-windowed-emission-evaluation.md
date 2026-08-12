# MECH-0002: Windowed Emission Evaluation

**Status:** Selected — candidate **C2 (retention window with windowed emission)** implemented and validated at implementation level (smoke + 64/256/512 sweep, all PASS); **24 h qualification run deferred** to the final qualification gate (workflow: build → validate continuously → one endurance run once the architecture is complete)
**Date:** 2026-08-12
**Relates to:** [MECH-0001](MECH-0001-snapshot-mechanism-evaluation.md) (Selected — read-model half proven, gate unmet), [ADR-0005](../adr/0005-windowed-observability.md) (Accepted — observability cost must be bounded), [ADR-0006](../adr/0006-persistent-cognitive-state.md) (Design — versioned persistent State contract)
**Scope:** bridge emission path (`services/bridge/src/source.rs`, `services/bridge/src/connection.rs`, `services/bridge/src/server.rs`) and the in-memory read-model retention behind it (`friday-experience::TimelineStore`); consuming projections (console, validation collector)
**Governance:** engineering record, not an ADR. Preserves *why* a particular mechanism is chosen so ADR-0005 stays about the architectural decision. If a better mechanism is found later, this record is revisited — the ADR's decision (observability cost must be bounded) is not reopened.
**Lifecycle:** **Draft** → evaluating candidates → **Proposed** → candidate selected, prototype pending → **Selected** → implemented and validated → **Superseded**. MECH-0002 is currently **Selected** — C2 implemented and validated at implementation level; the endurance run is scheduled for the final qualification gate, not per intermediate change.

## Context — evidence recap

VP-001 run-006 (24 h continuous, 100% coverage, post-C4+C6, git ac0069f) FAILED both decision criteria:

| Criterion | run-005 (old path) | run-006 (C4+C6) | Target |
| --- | --- | --- | --- |
| memory_growth | 11.9 MB/hr | **9.8 MB/hr** | < 1 MB/hr |
| per 1,000 records | 44.5 MB | **36.5 MB** | — |
| rtt_drift | +416.6% (45.4 ms) | **+394.4% (43.0 ms)** | < 10% |

MECH-0001 established that the read-model half is fixed: `dashboard_generation` is flat (mean 0.184 ms, +10.7% drift over 24 h), `timeline_regeneration` is absent. The remaining cost drivers were attributed to "wire emission" (RTT) and "retained point-in-time snapshots" (memory) — two independent residuals.

## New finding (2026-08-12): both residuals trace to the same emission path

Code inspection of the emission path shows the memory residual is **not primarily live retention** — it is the allocator high-water mark left by O(history) transient serialization. The chain:

```
TimelineStore (in-memory, unbounded)
      ↓
tick() → timelines_dto() + ledger_dto()      source.rs:770, 762
      full lists returned from store.timelines()
      ↓
DeltaPublisher::frame() deep-clones both     connection.rs:245-247
      full Vec<DecisionTimelineDto> into every delta frame
      ↓
encode() → FrameSerializer::to_json           connection.rs:371-375
      full frame serialized to String
      ↓
send_frame() → Message::Text(text.clone())    connection.rs:430-431
      String cloned again for the WebSocket message
      ↓
allocator high-water ratchets up              RSS growth
the same tokio::select! loop answers pings    RTT growth
      (connection.rs:652-783)
```

Per new-record delta (~every 13.2 s) the bridge allocates ~2× O(history) deep clones plus a full JSON string, then frees them; the Windows allocator does not return freed memory, so RSS monotonically tracks the peak frame size. The scale is confirmed by MECH-0001's own measurements: **87,594 frames / 216 GB serialized / day** versus ~235 MB/day of RSS growth. The per-1,000-record RSS cost (~36.5 MB) matches the transient serialization volume far better than live retention, which is on the order of a plan clone plus Arc pins per timeline (~KB-scale).

**Consequence:** the memory criterion and the RTT drift are two symptoms of one root cause — the O(history) full-list emission on every new-record delta. This is ADR-0005's own thesis ("two symptoms, one root cause: the cost of observability is unbounded in retained execution history"). Bounding **live retention alone cannot satisfy `< 1 MB/hr`** because the analyzer measures the bridge process RSS, which includes the emission path. Any mechanism chosen to pass the criterion must bound the emitted lists, not just the store.

## Ground truth: two TimelineStores

The memory-growth blocker concerns the **bridge's** read-model store. FRIDAY's durable history is a different object:

| | Bridge store (`friday_experience::TimelineStore`, services/experience/src/lib.rs:1784) | Planner store (`friday_planner::TimelineStore`, planner/src/experience/timeline.rs:28) |
| --- | --- | --- |
| Backing | in-memory `Vec<DecisionTimeline>` | redb, append-only, durable |
| Consumers | bridge `DemoRuntimeSource` read models + console only | reflection, recorder, miner, replay, experience service |
| Is it the source of truth? | No — observability only | Yes — the Experience Layer's canonical history |

Reflection (`planner/src/experience/reflection.rs`) reads the **redb** store. **Nothing learns from the bridge store.** The qualification fix targets the bridge read-model emission boundary, not the durable Experience history.

## Phase 1 — Evaluation criteria (frozen)

Decided before implementation. "Whichever is easiest to code" is not a criterion.

| Criterion | Weight | Measurement | Instrument |
| --- | --- | --- | --- |
| RSS slope | High | MB/hr (target < 1 MB/hr) | VP-001 analyzer |
| RTT stability | High | % drift (target < 10%) | VP-001 analyzer |
| Bounds live retention | High | retained bridge state constant vs history | code review / soak |
| Bounds transient emission | High | peak frame-size allocation constant vs history | code review / profile |
| Durable-history preservation | High | redb timeline/reflection/patterns untouched | test suite |
| Reflection/Learning semantics | High | planner experience layer semantics unchanged | code review + tests |
| ADR-0006 independence | High | no dependency on the Design-status persistent layer | design review |
| ADR-0003 compatibility | High | point-in-time snapshot guarantee inside the window | equivalence tests |
| Implementation complexity | Medium | estimated effort vs a full rewrite | estimate |
| Wire-compat risk | Medium | frame format unchanged; list semantics documented | protocol tests |

## Phase 2 — Candidate evaluation

### C5. Delta-only projections

Store the incremental delta from each timeline's predecessor instead of a full snapshot; reconstruction folds the deltas.

- Bounds live retention: **no** — every delta is retained; the constant shrinks, the O(history) shape stays.
- Bounds transient emission: **no** — reconstruction still materializes full lists for the wire, so the allocator high-water and RTT head-of-line blocking return.
- Read complexity: **high** — every read is a fold; the fold must be exact; the largest behavioral change across the frozen read-model surface.
- **Verdict: follow-on optimization only** — reduces the per-record constant (a strict improvement, per MECH-0001's "hold — revisit if C4+C6 under-delivers"), but it cannot pass `< 1 MB/hr` by itself.

### C2. Retention window + windowed emission

Keep a bounded horizon of timelines/records in the bridge store; emit the window, not full history. Older history is dropped from the bridge's derived read models — the raw immutable ledger and the durable redb timeline are never deleted (ADR-0005's own first-named candidate).

- Bounds live retention: **yes** — the store holds O(window).
- Bounds transient emission: **yes** — `timelines_dto()`/`ledger_dto()` derive from the windowed store, so the delta frame, its clones, and its JSON encode are all O(window).
- Bounds RTT: **yes (likely)** — encode+send per delta becomes O(window), unblocking the ping/pong path in the shared `tokio::select!` loop.
- Preserves durable history: **yes** — redb untouched; bridge deep-history read model is re-derivable from the ledger at CPU cost.
- ADR-0003 compatibility: **yes inside the window** — retained timelines keep point-in-time snapshot semantics; equivalence suite must still hold over the window.
- Wire semantics: frame format byte-identical; the **meaning of the lists changes** (recent window, not full history) — this is the wire-emission decision MECH-0001 flagged as a separate ADR; the console and validation collector must be told the lists are windowed.
- **Verdict: qualification fix.** The only candidate that can pass `< 1 MB/hr` on a protocol-identical run without new storage machinery, because it bounds live retention and transient emission in one move.

### C3. `ExperienceSnapshotId → store` lookup

Timelines hold an id; snapshots are fetched from a versioned store at read time.

- Bounds live retention: **yes** — the in-memory store holds current state + lookups.
- Bounds transient emission: **no unless the wire also windows** — fetching every timeline for the full-list delta recreates the O(history) emission cost.
- ADR-0003: **breaks "snapshots, not references" without a versioned store** — the MECH-0001 architectural constraint still binds; the versioned store is the dependency that restores the guarantee.
- ADR-0006 boundary: ADR-0006 is Status: Design with "no implementation decisions made here." Its validation gate requires an interface-impact analysis and its own accepted ADR before any change to the frozen contract surface, plus VP-002-style calibration validation. **C3 cannot proceed until a versioned State contract + storage backend ADR is written and accepted.**
- **Verdict: deferred** — the future-architecture move; not a qualification fix.

## Decision — C2 windowed emission first

**C2 (retention window with windowed emission) is the qualification fix.** Rationale, against the frozen criteria:

- does not touch the canonical redb history or Reflection/Learning semantics;
- does not require ADR-0006;
- bounds both retained bridge state **and** the O(history) serialization that actually drives the RSS slope;
- gives a concrete, measurable path to the `< 1 MB/hr` criterion;
- likely improves RTT because the emitted payload becomes O(window).

**C5 delta-only is a follow-on optimization**, to be applied only if the windowed-emission run leaves the memory slope above target. **C3 is deferred behind ADR-0006.** RTT remains its own blocker with its own evidence trail; the expectation is that C2 flattens both curves because both gate on the same bridge-side emission path — they are measured separately and will likely resolve together. This is not mixing fixes; it is addressing ADR-0005's single root cause.

## Window size is a calibration parameter — not a constant

The objective is not "one hour." The objective is:

```
RSS slope < 1 MB/hr
+
acceptable RTT
+
useful dashboard history
```

The retention horizon (records-based `N` or time-based `T`) is a calibration parameter, swept over:

```
window = N recent records
        ↓
RSS slope
RTT
delta payload size
dashboard usefulness
```

The sweep is evidence-gathering for the wire-emission decision (frame format unchanged; list semantics = recent window), not an architectural guess. No window value is hard-coded in this record.

## Boundary (hard constraint)

```
              Experience Layer
                   │
            canonical redb
                   │
       full durable history 🟢
                   │
         Reflection / Learning
                   │
                   │
         ──────────┼───────────
                   │
              Bridge
                   │
           recent window
                   │
            DTO emission
                   │
             WebSocket
```

**Do not delete or truncate the durable ledger to solve the bridge problem.** That would fix an observability/read-model problem by damaging the cognitive history. The bridge's window is a read-model concern only; the immutable ledger and the durable redb timeline retain everything.

## Phase 3 — Prototype (smallest slice, implemented 2026-08-12)

Smallest implementation that demonstrates the path, behind the frozen interfaces:

1. Bound the bridge `TimelineStore` retention to the window (records or time horizon).
2. Derive `timelines_dto()`/`ledger_dto()` from the windowed store so emission is O(window).
3. Document the list-semantics change (recent window, not full history) for the console and validation collector as the wire-emission decision.
4. Keep the frame format byte-identical (frozen).

### 3b — Implementation evidence (2026-08-12)

Slice delivered (git uncommitted, HEAD 5f96b0a): `--retention-window` CLI (default off) →
`TimelineStore` retention window (windowed eviction of records/timelines/experience, cumulative metrics preserved) →
windowed `ledger_dto()`/`timelines_dto()` → delta/snapshot list semantics = recent window → harness `--window` passthrough.
Durable planner redb store untouched. Frame format byte-identical (protocol v2).

| Evidence | Setup | Key readings |
| --- | --- | --- |
| `cargo test -p friday-experience -p friday-bridge` | 24 bridge integration + 3 protocol_compat + 1 equivalence + 55 experience unit | all PASS; new `retention_window_evicts_oldest…`, `demo_source_retention_window_emits_recent_lists_and_keeps_metrics_cumulative` |
| smoke-window-32 (75 s) | window 32, seeded history 140 (> window) | ledger **32→32** (plateau while 5 records appended); RSS 18.9→19.9 MB; wire 4.3 MB/75 s ≈ **5 GB/day vs run-006 216 GB/day**; RTT 0.0 ms; 0 gaps/crashes; dashboard_generation 0.178 ms |
| sweep-w64-clean (10 m) | window 64, history 140 (> window) | ledger **64→64** while episodes 140→184 (window slid, list pinned); RSS 18.8→19.5 MB, peak 21.1; wire 48.2 MB; RTT 0.0/0.0 ms (0.0% drift); 0 gaps/crashes; coverage VALID |
| sweep-w256 (10 m) | window 256, history 140 (< window, no truncation) | ledger 140→184; RSS 19.2→21.3 MB, peak 28.1; wire 89.2 MB; RTT mean 0.1 ms, drift ±0.0 ms abs; 0 gaps/crashes; coverage VALID |
| sweep-w512 (10 m) | window 512, history 140 (< window, no truncation) | ledger 140→184; RSS 19.4→21.5 MB, peak 26.8; wire 89.2 MB; RTT mean 0.1 ms, drift ±0.0 ms abs; 0 gaps/crashes; coverage VALID |

Readings: payload and wire volume scale with the window (w64 48.2 MB vs w256/512 89.2 MB per 10 m; snapshot 655 KB @64 vs 1376 KB @140), and plateau when history exceeds the window (smoke + w64). RSS steady-state ~19–21 MB flat across N (10-min slopes 6–13 MB/hr are warm-up artifacts; the "per 1,000 ledger records" denominator now plateaus). RTT sub-ms with ≤0.1 ms absolute drift. Bridge correctness clean (0 gaps/errors/reconnects/crashes/resyncs). Durable history unregressed (episodes/decisions/experience samples cumulative at consistent rates; planner store untouched; equivalence green). **C2 accepted as an implementation-level fix.**

Order of verification (per the qualification discipline — repro/soak before the long run):

1. **Bounded smoke test (75 s)** — ✅ window bounds hold, protocol tests pass, equivalence green over the window.
2. **Calibration sweep (64 / 256 / 512 × ~10 min)** — ✅ accepted above; window size remains a calibration parameter for the final build. A 2 h soak and the **protocol-identical 24 h VP-001 run** (seed 42, count 140, interval 5 s, coverage ≥ 99%, same thresholds, same analyzer; decision criteria memory `< 1 MB/hr` and RTT `< 10%`) are **deferred to the final qualification gate**, once C2, RTT work, CC-11, the remaining blockers, and final integrations are complete.

ADR-0005 is updated only when the decision threshold is reached on a protocol-identical run; the final mechanism choice is recorded here (MECH-0001 lifecycle: **Superseded** by MECH-0002 once C2 is selected-and-validated).

## Log

- 2026-08-12 — Evaluation written. New finding: the RSS slope is driven by the O(history) full-list emission path (deep clone → JSON encode → WebSocket text clone), confirmed against MECH-0001's 216 GB/day serialization churn. Two-store ground truth established (bridge in-memory observability store vs planner redb durable history). Candidates evaluated: C5 (follow-on), C2 (qualification fix), C3 (deferred behind ADR-0006). C2 selected with window size as a calibration parameter and the durable-ledger boundary recorded. Prototype pending.
- 2026-08-12 — Prototype implemented (smallest slice): `--retention-window` CLI, `TimelineStore` retention window, windowed DTO emission, harness `--window` passthrough. Validated: hermetic tests green (24 bridge integration, 3 protocol_compat, 1 equivalence, 55 experience unit); 75 s smoke (window 32) and 64/256/512 × 10 m sweep all PASS — bounded window (ledger pinned at N while episodes grow), wire volume scales with N and plateaus past it, RSS flat ~19–21 MB, RTT sub-ms, bridge correctness clean, durable history unregressed. **C2 accepted as an implementation-level fix.** Workflow decision (user, 2026-08-12): no per-change 24 h runs — the endurance run (2 h soak + protocol-identical 24 h VP-001) moves to the final qualification gate after C2, RTT work, CC-11, remaining blockers, and final integrations are complete.
