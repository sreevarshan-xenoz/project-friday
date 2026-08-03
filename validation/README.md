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

| Option       | Default                        | Meaning |
| ------------ | ------------------------------ | ------- |
| `--duration` | `2h`                           | `90s`, `30m`, `8h`, `1d`, or bare seconds |
| `--interval` | `5s`                           | sample interval |
| `--output`   | `artifacts/validation/run-<ts>` | run artifact directory (created) |
| `--listen`   | `127.0.0.1:18433`              | bridge listen address |
| `--seed`     | `42`                           | bridge RNG seed (same seed ⇒ same ledger) |
| `--count`    | `140`                          | initial ledger records |
| `--bridge`   | `target/debug/friday-bridge`   | path to the bridge binary |
| `--analyze`  | —                              | analyze an existing run directory |
| `--no-analyze` | off                          | skip analysis after a run |

Exit codes: `0` clean + analyzer PASS · `1` crash or analyzer FAIL · `2` usage error.
`Ctrl-C` stops cleanly and still analyzes what was captured.

## Output layout

```
artifacts/validation/run-001/
  samples.jsonl    append-only data, one JSON line per sample
  bridge.log       bridge stdout/stderr
  collector.log    collector diagnostics
  manifest.json    config + timings + exit codes + git commit
  report.md        human verdict (printed at end of the run)
  report.json      machine verdict (criteria + metrics)
```

Each sample captures `bridge` counters, `runtime` read-model counts,
`process` RSS/CPU/handles, `network` RTT (raw samples kept for exact
percentiles), and `transport` (frames/bytes, sequence gaps, connection
starts/errors, error frames, per-kind counts).

## Failure criteria (VP-001)

Decided **before** the run; the analyzer prints each as PASS/FAIL/INFO.

| Metric                | Target          |
| --------------------- | --------------- |
| Memory growth         | < 1 MB/hour     |
| RTT drift             | < 10%           |
| Frame loss            | 0 (sequence gaps) |
| Sequence gaps         | 0              |
| Unexpected reconnects | 0              |
| Process crashes       | 0              |

Overrides: `--max-mem-slope-mb-hr 1.0 --max-rtt-drift-pct 10`.

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
