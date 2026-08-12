# Validation Harness — VP-001

The unattended test rig for the Validation Program (`docs/VALIDATION.md`,
VP-001 — *Long-running cognition*). It starts FRIDAY, watches it for hours or
days, and reports a verdict — no dashboards, no human watching a screen.

```text
8 hours → collect → report → exit
```

Three pieces, one reusable CLI:

```text
friday-validate         Runner   — start bridge, start collector, monitor,
                                   stop cleanly, archive artifacts
collector.mjs           Collector — the console client (WebSocket + negotiation);
                                   every interval appends one JSONL sample
analyze.mjs             Analyzer  — answers questions, checks failure criteria,
                                   writes report.md + report.json, exit code
```

## Run

```bash
node validation/friday-validate.mjs --duration 8h --interval 5s --output artifacts/validation/run-001
```

Short smoke (75 s) first, then scale up: 2 h → 8 h → 24 h → 72 h.

> A run is judged on **hours of continuous observation**, not wall-clock hours.
> If the host sleeps, the experiment does not — a 24 h run with a 10 h sleep is
> ~14 h observed, and the manifest records it as **INVALID** (see
> Observation coverage below). Prevent sleep for the duration of long runs:
> `powercfg /change standby-timeout-ac 0` (and `/dc`) on Windows, equivalent
> on POSIX. This is an experimental control, not an architectural change.

| Option       | Default                        | Meaning |
| ------------ | ------------------------------ | ------- |
| `--duration` | `2h`                           | `90s`, `30m`, `8h`, `1d`, or bare seconds |
| `--interval` | `5s`                           | sample interval |
| `--output`   | `artifacts/validation/run-<ts>` | run artifact directory (created) |
| `--listen`   | `127.0.0.1:18433`              | bridge listen address |
| `--seed`     | `42`                           | bridge RNG seed (same seed ⇒ same ledger) |
| `--count`    | `140`                          | initial ledger records |
| `--window`   | unbounded                      | bridge retention window (records; recent/windowed list semantics, MECH-0002 C2) |
| `--bridge`   | `target/debug/friday-bridge`   | path to the bridge binary |
| `--coverage-threshold` | `99`                  | min observation coverage (%) for a VALID run |
| `--analyze`  | —                              | analyze an existing run directory |
| `--no-analyze` | off                          | skip analysis after a run |
| `--append`   | off                            | allow a non-empty `--output` dir (extend an interrupted run) |

Exit codes: `0` clean + analyzer PASS · `1` crash, analyzer FAIL on any
**decision**/**integrity** criterion, or INVALID observation coverage · `2`
usage error. An **operational** anomaly (e.g. an unexpected reconnect) is
recorded and reported, never gating.
`Ctrl-C` stops cleanly and still analyzes what was captured.

## Output layout

```
artifacts/validation/run-001/
  samples.jsonl    append-only data, one JSON line per sample
  bridge.log       bridge stdout/stderr
  collector.log    collector diagnostics (OBSERVATION GAP lines from watchdog)
  manifest.json    config + observation coverage + timings + exit codes + git commit
  report.md        human verdict (printed at end of the run)
  report.json      machine verdict (criteria + metrics)
```

Each sample captures `bridge` counters, `runtime` read-model counts,
`process` RSS/CPU/handles, `network` RTT (raw samples kept for exact
percentiles), and `transport` (frames/bytes, sequence gaps, connection
starts/errors, error frames, per-kind counts).

The runner **refuses a non-empty `--output` dir** unless `--append` is passed:
a rerun into a stale directory would append new samples onto an aborted run's
data and corrupt the measurement. Every run should land in a fresh directory
(or pass `--append` deliberately, e.g. to extend an interrupted run).

## Observation coverage

The manifest records how much of the intended run was actually observed:

```json
"observation": {
  "expected_duration_ms": 86400000,
  "observed_duration_ms": 51500000,
  "coverage_percent": 59.6,
  "largest_gap_ms": 34890910,
  "gap_count": 2,
  "status": "INVALID",
  "threshold_percent": 99
}
```

- `observed_duration_ms` = span between the first and last sample minus every
  inter-sample gap longer than 3× the interval. A 9 h sleep counts as ~15 s,
  not 9 h.
- `coverage_percent` = observed / intended, capped at 100.
- `status` = **VALID** iff coverage ≥ `--coverage-threshold` (default 99),
  else **INVALID** — the run is not the experiment that was designed.

Two watchdogs detect holes while the run is live:

- **Collector** — if a sample is emitted more than 3× the interval after the
  previous one, it logs `OBSERVATION GAP expected 5s actual 34877s`.
- **Runner** — every tick it checks the samples file's mtime; a stall is
  logged and recorded in `observation_gaps_watchdog`. Host sleep freezes this
  timer too, so the first tick after wake catches the hole; the manifest
  coverage block is the authoritative record.

A hole in the observation does **not** say FRIDAY failed — it says the
experiment lost observability, so the metrics describe a partial run and the
verdict is INVALID regardless of the curves.

## Failure criteria (VP-001)

Decided **before** the run; the analyzer prints each as PASS/FAIL/INFO.
Criteria are classed (governance in the Gate of `docs/adr/0005-windowed-observability.md`):

- **decision** — validates the ADR hypothesis directly (memory growth, RTT
  drift). Gates the verdict and exit code.
- **integrity** — measurement validity; a failure corrupts the experiment
  (frame loss, sequence gaps, process crashes, observation coverage). Gates
  the verdict and exit code.
- **operational** — unrelated runtime anomalies (unexpected reconnects, and
  future host power / network events). Recorded in `report.json`
  `operational_observations` and shown in the report, but **never** gate — a
  transport anomaly only matters when it corrupts the experiment, which is
  what `integrity` detects.

| Class         | Metric                | Target          |
| ------------- | --------------------- | --------------- |
| decision      | Memory growth         | < 1 MB/hour     |
| decision      | RTT drift             | < 10%           |
| integrity     | Frame loss            | 0 (sequence gaps) |
| integrity     | Sequence gaps         | 0              |
| operational   | Unexpected reconnects | 0 (recorded, non-gating) |
| integrity     | Process crashes       | 0              |
| integrity     | Observation coverage  | ≥ 99%          |

Overrides: `--max-mem-slope-mb-hr 1.0 --max-rtt-drift-pct 10 --coverage-threshold 99`.

Two report signals beyond the table:

- **Memory per 1,000 ledger records** — the demo runtime's read models grow
  with the ledger (they are derived in-memory), so some RSS growth is expected
  data growth, not a leak. The analyzer separates slope from slope-per-record.
- **Delta frame size trend** — the additive protocol ships full replacement
  slices, so delta size grows with the ledger; the trend is evidence for later
  transport work (stages 3–4), not a failure.

One causal section beyond the table:

- **Read-model generation timings** — the bridge times `dashboard_generation`
  and `timeline_regeneration` internally and writes `TIMING <name> <ms>
  <epoch-ms>` lines to its stderr (captured into `bridge.log` by the runner;
  deliberately **off the wire**, so the frozen frame format stays
  byte-identical). The analyzer parses them and reports count / mean / p95 /
  first-vs-second-half drift per name — the causal link for RTT drift: if
  generation time grows with retained history, per-frame CPU is O(history).
  Not a criterion. Instrumented in `git 0e6df06`+; runs launched before that
  have no `TIMING` lines and the section shows `—`.

## Reuse

Same runner + collector for later programs: change the data the Collector
samples (the `runtime` section is the only protocol-specific part), not the
orchestration. No dependencies — plain Node, Node 24 global `WebSocket`.
Process metrics are full on Windows (RSS/CPU/handles) and RSS-only on POSIX.

Known limitation: metrics are capped at the collector's own connection
(per-connection sequence stream). Server-wide counters still capture
connections/resyncs for cross-client behavior.
