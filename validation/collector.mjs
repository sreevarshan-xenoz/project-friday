#!/usr/bin/env node
// Collector — the Validation Harness "console client".
//
// Connects to the FRIDAY ReadModelBridge exactly like the Engineering Console:
// receives `hello`, answers with `{"type":"hello_response","capabilities":{"metrics":true}}`,
// waits for the `capabilities` ack, then streams the snapshot and deltas. Every
// `--interval` ms it appends one JSONL sample to `--out` combining:
//
//   bridge    — bridge-wide counters from hello/heartbeats (active connections,
//               reconnects, resyncs, frames/bytes sent, snapshot/delta sizes)
//   runtime   — last-known read-model counts (phase, episodes, ledger records,
//               experience/reputation totals, decisions, prediction accuracy)
//   process   — the bridge process's RSS / CPU rate / handles (`--bridge-pid`)
//   network   — measured RTT from snapshot + heartbeat frames (raw samples kept
//               per interval so the analyzer can compute exact percentiles)
//   transport — frames/bytes received, sequence gaps, connection starts/errors,
//               error frames received, per-kind frame counts
//
// Append-only, never overwrites. Spawned by the Runner; SIGTERM ends the run.

import { createWriteStream } from 'node:fs';
import { parseFlags } from './lib/cli.mjs';
import { sampleProcess } from './lib/proc.mjs';

const args = parseFlags(process.argv.slice(2));
const url = args.url ?? 'ws://127.0.0.1:18433';
const bridgePid = args['bridge-pid'] ? Number(args['bridge-pid']) : null;
const intervalMs = args.interval ? Number(args.interval) : 5000;
const outPath = args.out ?? null;
const tag = args.tag ?? 'collector';

function log(...parts) {
  process.stderr.write(`[${tag}] ${parts.join(' ')}\n`);
}

if (!outPath) {
  log('--out <file> is required');
  process.exit(2);
}

const out = createWriteStream(outPath, { flags: 'a' });
const startedAt = Date.now();

const state = {
  socket: null,
  shuttingDown: false,
  reconnectAttempts: 0,
  lastSampleTs: null,

  lastSequence: null,
  snapshotsReceived: 0,
  framesReceived: 0,
  bytesReceived: 0,
  sequenceGaps: 0,
  connectionStarts: 0,
  connectionErrors: 0,
  resyncsReceived: 0,
  errorsReceived: 0,
  lastError: null,
  framesByKind: {},

  lastBridgeStats: null,
  runtime: null,

  rttWindow: [],
  procPrev: null,
};

function runtimeFromMetrics(m) {
  if (!m) return {};
  return {
    decisions: m.decisions ?? null,
    experience_pairs: m.experience?.distinct_pairs ?? null,
    experience_samples: m.experience?.total_samples ?? null,
    reputation_observations: m.reputation?.total_observations ?? null,
    prediction_accuracy: m.prediction?.accuracy ?? null,
    calibration_converging: m.calibration?.converging ?? null,
  };
}

function deriveRuntime(dashboard, metrics, ledger) {
  return {
    executive_phase: dashboard?.phase ?? null,
    goal_progress: dashboard?.world?.goal_progress ?? null,
    episodes: dashboard?.world?.episodes ?? null,
    ledger_records: ledger?.length ?? null,
    ...runtimeFromMetrics(metrics),
  };
}

function onFrame(msg) {
  const seq = msg.sequence;
  if (typeof seq === 'number') {
    if (state.lastSequence !== null && seq !== state.lastSequence + 1) {
      state.sequenceGaps += 1;
      log('SEQUENCE GAP', state.lastSequence, '->', seq);
    }
    state.lastSequence = seq;
  }
  state.framesReceived += 1;
  state.framesByKind[msg.kind ?? 'unknown'] = (state.framesByKind[msg.kind ?? 'unknown'] ?? 0) + 1;

  switch (msg.kind) {
    case 'hello':
      state.lastBridgeStats = msg.bridge_stats ?? state.lastBridgeStats;
      try {
        state.socket?.send(
          JSON.stringify({ type: 'hello_response', capabilities: { metrics: true } }),
        );
      } catch {
        // connection already closing; nothing to negotiate
      }
      break;
    case 'capabilities':
      break;
    case 'snapshot':
      state.snapshotsReceived += 1;
      if (state.snapshotsReceived > 1) state.resyncsReceived += 1;
      state.runtime = deriveRuntime(msg.dashboard, msg.metrics, msg.ledger);
      if (typeof msg.latency_ms === 'number') state.rttWindow.push(msg.latency_ms);
      break;
    case 'delta':
      if (msg.dashboard) {
        state.runtime = state.runtime ?? {};
        state.runtime.executive_phase = msg.dashboard.phase;
        state.runtime.goal_progress = msg.dashboard.world?.goal_progress ?? null;
        state.runtime.episodes = msg.dashboard.world?.episodes ?? null;
      }
      if (msg.timelines) {
        state.runtime = state.runtime ?? {};
        state.runtime.episodes = msg.timelines.length;
      }
      if (msg.ledger) {
        state.runtime = state.runtime ?? {};
        state.runtime.ledger_records = msg.ledger.length;
      }
      if (msg.metrics) {
        state.runtime = state.runtime ?? {};
        Object.assign(state.runtime, runtimeFromMetrics(msg.metrics));
      }
      break;
    case 'heartbeat':
      if (typeof msg.latency_ms === 'number') state.rttWindow.push(msg.latency_ms);
      state.lastBridgeStats = msg.bridge_stats ?? state.lastBridgeStats;
      break;
    case 'error':
      state.errorsReceived += 1;
      state.lastError = { code: msg.code ?? null, message: msg.message };
      log('ERROR FRAME', msg.code ?? '', msg.message);
      break;
    default:
      log('UNKNOWN KIND', msg.kind);
  }
}

async function emitSample() {
  const now = Date.now();
  const rtts = state.rttWindow;
  state.rttWindow = [];

  // Watchdog: if this sample is far later than the interval, the observation
  // has a hole (host sleep / frozen loop / dropped write). Log it so the run
  // knows it became invalid — the manifest coverage block records the final
  // verdict.
  if (state.lastSampleTs !== null) {
    const delta = now - state.lastSampleTs;
    if (delta > intervalMs * 3) {
      log(
        `OBSERVATION GAP expected ${intervalMs / 1000}s actual ${(delta / 1000).toFixed(0)}s`,
      );
    }
  }
  state.lastSampleTs = now;

  let avg = null;
  let peak = null;
  let last = null;
  if (rtts.length > 0) {
    avg = rtts.reduce((a, b) => a + b, 0) / rtts.length;
    peak = Math.max(...rtts);
    last = rtts[rtts.length - 1];
  }

  const proc = await sampleProcess(bridgePid, state.procPrev, intervalMs);
  state.procPrev = proc;

  const sample = {
    ts: now,
    elapsed_ms: now - startedAt,
    bridge: state.lastBridgeStats ? { ...state.lastBridgeStats } : null,
    runtime: state.runtime ? { ...state.runtime } : null,
    process: proc
      ? {
          rss_bytes: proc.rss_bytes,
          cpu_percent: proc.cpu_percent,
          handles: proc.handles,
          alive: proc.alive,
        }
      : null,
    network: {
      rtt_samples: rtts.length,
      avg_rtt_ms: avg,
      peak_rtt_ms: peak,
      last_rtt_ms: last,
      rtts,
    },
    transport: {
      frames_received: state.framesReceived,
      bytes_received: state.bytesReceived,
      frames_by_kind: { ...state.framesByKind },
      sequence_gaps: state.sequenceGaps,
      connection_starts: state.connectionStarts,
      connection_errors: state.connectionErrors,
      resyncs_received: state.resyncsReceived,
      errors_received: state.errorsReceived,
      last_error: state.lastError,
      last_sequence: state.lastSequence,
    },
  };
  out.write(`${JSON.stringify(sample)}\n`);
}

function connect() {
  state.connectionStarts += 1;
  const ws = new WebSocket(url);
  state.socket = ws;

  ws.onopen = () => {
    state.lastSequence = null;
    state.reconnectAttempts = 0;
    log('connected', url);
  };

  ws.onmessage = (ev) => {
    state.bytesReceived += Buffer.byteLength(ev.data, 'utf8');
    let msg;
    try {
      msg = JSON.parse(ev.data);
    } catch {
      state.sequenceGaps += 1;
      log('BAD FRAME (not JSON)');
      return;
    }
    onFrame(msg);
  };

  ws.onerror = () => {
    state.connectionErrors += 1;
  };

  ws.onclose = () => {
    if (state.shuttingDown) return;
    state.reconnectAttempts += 1;
    const delay = Math.min(500 * 2 ** (state.reconnectAttempts - 1), 5000);
    log('connection closed, reconnecting in', delay, 'ms');
    setTimeout(connect, delay);
  };
}

function shutdown(code) {
  if (state.shuttingDown) return;
  state.shuttingDown = true;
  try {
    state.socket?.close();
  } catch {
    // ignore
  }
  out.end(() => process.exit(code));
}

process.on('SIGTERM', () => shutdown(0));
process.on('SIGINT', () => shutdown(0));

connect();
let sampling = false;
const sampleLoop = () => {
  if (sampling) return;
  sampling = true;
  emitSample().finally(() => {
    sampling = false;
  });
};
setTimeout(sampleLoop, 250);
setInterval(sampleLoop, intervalMs);
