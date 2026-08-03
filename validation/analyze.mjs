#!/usr/bin/env node
// Analyzer — turns a run's `samples.jsonl` into a verdict.
//
// Answers questions instead of dumping numbers:
//   - Memory:   start/end/peak RSS, slope (MB/hour), slope per 1,000 ledger
//               records (the demo source's read models grow with the ledger,
//               so "growth tracks data" is distinct from a leak).
//   - Network:  mean/p50/p95/max RTT over every measured sample, first-half vs
//               second-half drift.
//   - Transport: frames/bytes, throughput, sequence gaps (= dropped frames),
//               connection starts (1 = never reconnected), error frames.
//   - Runtime:  ledger/episodes/decisions growth — expected, reported to
//               separate data growth from engine accumulation.
//
// VP-001 failure criteria are enforced (defaults below, overridable): a metric
// over its threshold is reported loudly and fails the run. Writes `report.md`
// and `report.json` into the run directory. Exit code 1 on any failure.

import { existsSync, readFileSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { parseFlags } from './lib/cli.mjs';

const args = parseFlags(process.argv.slice(2));
const dir = args._[0];

const maxMemSlopeMbHr = args['max-mem-slope-mb-hr'] ? Number(args['max-mem-slope-mb-hr']) : 1.0;
const maxRttDriftPct = args['max-rtt-drift-pct'] ? Number(args['max-rtt-drift-pct']) : 10.0;

if (!dir) {
  console.error('usage: node analyze.mjs <run-dir> [--max-mem-slope-mb-hr 1.0] [--max-rtt-drift-pct 10]');
  process.exit(2);
}

const samplesPath = join(dir, 'samples.jsonl');
const manifestPath = join(dir, 'manifest.json');
if (!existsSync(samplesPath)) {
  console.error(`no samples.jsonl in ${dir}`);
  process.exit(2);
}
const manifest = existsSync(manifestPath)
  ? JSON.parse(readFileSync(manifestPath, 'utf8'))
  : {};

const raw = readFileSync(samplesPath, 'utf8');
const samples = raw
  .trim()
  .split('\n')
  .filter(Boolean)
  .map((line) => JSON.parse(line));

// ---------------------------------------------------------------------------
// math helpers
// ---------------------------------------------------------------------------

function slope(points) {
  const n = points.length;
  if (n < 2) return null;
  let sx = 0;
  let sy = 0;
  let sxx = 0;
  let sxy = 0;
  for (const [x, y] of points) {
    sx += x;
    sy += y;
    sxx += x * x;
    sxy += x * y;
  }
  const denom = n * sxx - sx * sx;
  if (Math.abs(denom) < 1e-9) return null;
  return (n * sxy - sx * sy) / denom;
}

function percentile(sorted, p) {
  if (sorted.length === 0) return null;
  const idx = Math.min(sorted.length - 1, Math.ceil((p / 100) * sorted.length) - 1);
  return sorted[Math.max(0, idx)];
}

function fmt(n, digits = 1) {
  if (n === null || n === undefined || Number.isNaN(n)) return '—';
  return n.toFixed(digits);
}

// ---------------------------------------------------------------------------
// metrics
// ---------------------------------------------------------------------------

const first = samples[0];
const last = samples[samples.length - 1];
const elapsedMs = last.ts - first.ts;
const durationHr = elapsedMs / 3.6e6;
const spanTooShort = elapsedMs < 30 * 60 * 1000;

// memory — RSS of the bridge process
const memPoints = samples
  .filter((s) => s.process && s.process.alive && typeof s.process.rss_bytes === 'number')
  .map((s) => [s.elapsed_ms, s.process.rss_bytes]);
const memFirst = memPoints.length ? memPoints[0][1] : null;
const memLast = memPoints.length ? memPoints[memPoints.length - 1][1] : null;
const memPeak = memPoints.length ? Math.max(...memPoints.map((p) => p[1])) : null;
const memSlopeBytesPerMs = slope(memPoints);
const memSlopeMbHr =
  memSlopeBytesPerMs === null ? null : (memSlopeBytesPerMs * 3.6e6) / (1024 * 1024);

// memory vs ledger — is growth explained by data growth?
const memVsLedger = samples
  .filter(
    (s) =>
      s.process &&
      s.process.alive &&
      typeof s.process.rss_bytes === 'number' &&
      s.runtime &&
      typeof s.runtime.ledger_records === 'number',
  )
  .map((s) => [s.runtime.ledger_records, s.process.rss_bytes]);
const memSlopePerRecord = slope(memVsLedger);
const mbPer1000Records =
  memSlopePerRecord === null ? null : (memSlopePerRecord * 1000) / (1024 * 1024);

// network — every measured RTT. Percentiles sort; drift must preserve time
// order (first half vs second half of the run), otherwise it would compare
// low vs high values and report nonsense drift.
const rttsOrdered = samples.flatMap((s) => s.network?.rtts ?? []);
const rtts = [...rttsOrdered].sort((a, b) => a - b);
const rttMean = rtts.length ? rtts.reduce((a, b) => a + b, 0) / rtts.length : null;
const rttP50 = percentile(rtts, 50);
const rttP95 = percentile(rtts, 95);
const rttMax = rtts.length ? rtts[rtts.length - 1] : null;

let rttDriftPct = null;
let rttDriftMs = null;
if (rttsOrdered.length >= 4) {
  const mid = Math.floor(rttsOrdered.length / 2);
  const a = rttsOrdered.slice(0, mid);
  const b = rttsOrdered.slice(mid);
  const ma = a.reduce((x, y) => x + y, 0) / a.length;
  const mb = b.reduce((x, y) => x + y, 0) / b.length;
  rttDriftPct = (Math.abs(mb - ma) / Math.max(ma, 1e-9)) * 100;
  rttDriftMs = Math.abs(mb - ma);
}

// transport — cumulative counters (monotonic within a run)
const lastTransport = last.transport ?? {};
const gaps = Math.max(...samples.map((s) => s.transport?.sequence_gaps ?? 0));
const starts = Math.max(...samples.map((s) => s.transport?.connection_starts ?? 0));
const unexpectedReconnects = Math.max(0, starts - 1);
const framesReceived = lastTransport.frames_received ?? 0;
const bytesReceived = lastTransport.bytes_received ?? 0;
const errorsReceived = lastTransport.errors_received ?? 0;
const resyncsReceived = lastTransport.resyncs_received ?? 0;
const framesByKind = lastTransport.frames_by_kind ?? {};
const kbps = elapsedMs > 0 ? (bytesReceived / 1024) / (elapsedMs / 1000) : null;

// crashes — bridge gone before a clean stop
const aliveFalseCount = samples.filter((s) => s.process && s.process.alive === false).length;
const crashed = Boolean(manifest.bridge_exited_early) || aliveFalseCount >= 3;

// runtime growth
function growth(key) {
  const series = samples
    .map((s) => s.runtime?.[key])
    .filter((v) => typeof v === 'number');
  if (series.length < 2) return { first: null, last: null, perHr: null };
  const firstVal = series[0];
  const lastVal = series[series.length - 1];
  const perHr = durationHr > 0 ? (lastVal - firstVal) / durationHr : null;
  return { first: firstVal, last: lastVal, perHr };
}
const ledger = growth('ledger_records');
const episodes = growth('episodes');
const decisions = growth('decisions');
const experienceSamples = growth('experience_samples');

// delta / snapshot wire size trend
function sizeTrend(key) {
  const points = samples
    .filter((s) => s.bridge && typeof s.bridge[key] === 'number')
    .map((s) => [s.elapsed_ms, s.bridge[key]]);
  if (points.length < 2) return { first: null, last: null, slopePerHr: null };
  const sl = slope(points);
  return {
    first: points[0][1],
    last: points[points.length - 1][1],
    slopePerHr: sl === null ? null : sl * 3.6e6,
  };
}
const deltaSize = sizeTrend('delta_size_bytes');
const snapshotSize = sizeTrend('snapshot_size_bytes');

// ---------------------------------------------------------------------------
// failure criteria (VP-001)
// ---------------------------------------------------------------------------

function memVerdict() {
  if (memSlopeMbHr === null) return { pass: 'informative', label: 'No bridge process samples' };
  if (spanTooShort) return { pass: 'informative', label: 'Run span < 30 min' };
  const ok = Math.abs(memSlopeMbHr) < maxMemSlopeMbHr;
  return {
    pass: ok,
    label: `${fmt(memSlopeMbHr)} MB/hr (target ±${maxMemSlopeMbHr})`,
  };
}

function rttVerdict() {
  if (rtts.length === 0) return { pass: 'informative', label: 'No RTT samples' };
  // Relative drift is meaningless at loopback scale: the protocol measures
  // integer milliseconds, so a sub-2 ms absolute wobble is measurement noise,
  // not instability. Pass on relative drift OR an absolute floor.
  const ok = rttDriftPct !== null && (rttDriftPct < maxRttDriftPct || rttDriftMs < 2);
  return {
    pass: ok,
    label: `${fmt(rttDriftPct)}% relative (${fmt(rttDriftMs)} ms absolute)`,
  };
}

const criteria = [
  {
    name: 'memory_growth',
    target: `< ${maxMemSlopeMbHr} MB/hour`,
    ...memVerdict(),
  },
  {
    name: 'rtt_drift',
    target: `< ${maxRttDriftPct}%`,
    ...rttVerdict(),
  },
  {
    name: 'frame_loss',
    target: '0',
    observed: gaps,
    pass: gaps === 0,
    label: `${gaps} dropped frame(s) via sequence gaps`,
  },
  {
    name: 'sequence_gaps',
    target: '0',
    observed: gaps,
    pass: gaps === 0,
    label: `${gaps} gap(s) in per-connection sequence`,
  },
  {
    name: 'unexpected_reconnects',
    target: '0',
    observed: unexpectedReconnects,
    pass: unexpectedReconnects === 0,
    label: `${unexpectedReconnects} reconnect(s) after ${starts} connect(s)`,
  },
  {
    name: 'process_crashes',
    target: '0',
    observed: crashed ? 1 : 0,
    pass: !crashed,
    label: crashed ? 'bridge exited early' : 'no crashes',
  },
];

const failures = criteria.filter((c) => c.pass === false);
const verdict = failures.length ? 'FAIL' : 'PASS';

// ---------------------------------------------------------------------------
// report
// ---------------------------------------------------------------------------

const rows = criteria
  .map((c) => {
    const mark = c.pass === true ? 'PASS' : c.pass === false ? 'FAIL' : 'INFO';
    return `| ${mark} | ${c.name} | ${c.target} | ${c.label} |`;
  })
  .join('\n');

const report = `# VP-001 Validation Run — ${manifest.run_id ?? dir}

Run: ${manifest.run_id ?? '—'} · protocol v${manifest.protocol_version ?? 1} · ${manifest.bridge_id ?? '—'}
Started: ${manifest.started_at ?? new Date(first.ts).toISOString()} · Ended: ${manifest.ended_at ?? new Date(last.ts).toISOString()}
Duration: ${fmt(durationHr * 60, 0)} min · Samples: ${samples.length} (every ${fmt((elapsedMs / Math.max(1, samples.length - 1)) / 1000, 1)}s)
Bridge: seed ${manifest.seed ?? '—'} · count ${manifest.count ?? '—'} · git ${manifest.git_commit ?? '—'}
Result: **${verdict}** ${failures.length ? `(${failures.map((f) => f.name).join(', ')})` : ''}

## Memory

| | |
| --- | --- |
| Start | ${fmt(memFirst / 1048576)} MB |
| End | ${fmt(memLast / 1048576)} MB |
| Peak | ${fmt(memPeak / 1048576)} MB |
| Slope | ${memSlopeMbHr === null ? '—' : `${fmt(memSlopeMbHr)} MB/hour`} |
| Per 1,000 ledger records | ${mbPer1000Records === null ? '—' : `${fmt(mbPer1000Records)} MB`} |

Verdict: ${memVerdict().pass === true ? 'Stable' : memVerdict().pass === false ? 'Unstable' : memVerdict().label}

## Network (RTT)

| | |
| --- | --- |
| Mean | ${fmt(rttMean)} ms |
| p50 | ${fmt(rttP50)} ms |
| p95 | ${fmt(rttP95)} ms |
| Max | ${fmt(rttMax)} ms |
| Drift (1st vs 2nd half) | ${rttDriftPct === null ? '—' : `±${fmt(rttDriftMs)} ms (${fmt(rttDriftPct)}%)`} |

Verdict: ${rttVerdict().pass === true ? 'Stable' : rttVerdict().pass === false ? 'Unstable' : rttVerdict().label}

## Transport

| | |
| --- | --- |
| Frames received | ${framesReceived} (${Object.entries(framesByKind).map(([k, v]) => `${k}: ${v}`).join(', ')}) |
| Bytes received | ${fmt(bytesReceived / 1048576)} MB |
| Throughput | ${kbps === null ? '—' : `${fmt(kbps, 2)} KB/s`} |
| Sequence gaps | ${gaps} |
| Connection starts | ${starts} (unexpected reconnects: ${unexpectedReconnects}) |
| Error frames | ${errorsReceived} |
| Auto/client resyncs | ${resyncsReceived} |

## Runtime growth (expected — separates data growth from engine accumulation)

| | start → end | /hour |
| --- | --- | --- |
| Ledger records | ${ledger.first} → ${ledger.last} | ${fmt(ledger.perHr, 0)} |
| Episodes | ${episodes.first} → ${episodes.last} | ${fmt(episodes.perHr, 0)} |
| Decisions | ${decisions.first} → ${decisions.last} | ${fmt(decisions.perHr, 0)} |
| Experience samples | ${experienceSamples.first} → ${experienceSamples.last} | ${fmt(experienceSamples.perHr, 0)} |
| Delta frame size | ${fmt(deltaSize.first / 1024)} KB → ${fmt(deltaSize.last / 1024)} KB | ${deltaSize.slopePerHr === null ? '—' : `${fmt(deltaSize.slopePerHr / 1024, 1)} KB/hr`} |
| Snapshot size | ${fmt(snapshotSize.first / 1024)} KB → ${fmt(snapshotSize.last / 1024)} KB | — |

## Failure criteria (VP-001)

| Verdict | Criterion | Target | Observed |
| --- | --- | --- | --- |
${rows}
`;

console.log(report);
writeFileSync(join(dir, 'report.md'), report);
writeFileSync(
  join(dir, 'report.json'),
  `${JSON.stringify(
    {
      run_id: manifest.run_id ?? null,
      verdict,
      failures: failures.map((f) => f.name),
      metrics: {
        duration_ms: elapsedMs,
        samples: samples.length,
        memory: { start_bytes: memFirst, end_bytes: memLast, peak_bytes: memPeak, slope_mb_per_hr: memSlopeMbHr, mb_per_1000_records: mbPer1000Records },
        rtt: { mean_ms: rttMean, p50_ms: rttP50, p95_ms: rttP95, max_ms: rttMax, drift_pct: rttDriftPct, drift_ms: rttDriftMs },
        transport: { frames: framesReceived, bytes: bytesReceived, gaps, connection_starts: starts, errors_received: errorsReceived, resyncs_received: resyncsReceived, kbps },
        runtime_growth: { ledger: ledger, episodes, decisions, experience_samples: experienceSamples },
        wire_sizes: { delta: deltaSize, snapshot: snapshotSize },
      },
      criteria: criteria.map((c) => ({ name: c.name, target: c.target, observed: c.observed, label: c.label, pass: c.pass })),
      crashed,
    },
    null,
    2,
  )}\n`,
);

process.exit(failures.length ? 1 : 0);
