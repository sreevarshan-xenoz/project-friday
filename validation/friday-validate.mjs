#!/usr/bin/env node
// friday-validate — the Validation Harness Runner.
//
// Orchestrates an unattended VP-001 run: starts the bridge, starts the
// Collector (console client), monitors both, stops cleanly, archives
// artifacts, and runs the Analyzer for a verdict.
//
//   node validation/friday-validate.mjs --duration 2h --interval 5s \
//       --output artifacts/validation/run-001
//
// Exit codes: 0 = clean run + analyzer PASS · 1 = crash or analyzer FAIL ·
// 2 = usage/config error. `--analyze <dir>` re-runs the analyzer only.

import { spawn, execFileSync } from 'node:child_process';
import { createWriteStream, existsSync, mkdirSync, readFileSync, readdirSync, statSync, writeFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { parseFlags } from './lib/cli.mjs';
import { parseDuration, formatDuration } from './lib/durations.mjs';
import { observationCoverage, observationStatus, fmtDurationMs } from './lib/observation.mjs';

const here = dirname(fileURLToPath(import.meta.url));
const repoRoot = dirname(here);
const isWin = process.platform === 'win32';

const args = parseFlags(process.argv.slice(2));

function usage() {
  console.log(
    `friday-validate — VP-001 Validation Harness runner

Usage:
  node friday-validate.mjs [options]
  node friday-validate.mjs --analyze <run-dir>

Options:
  --duration <dur>     Run length (300, 90s, 30m, 8h, 1d). Default: 2h
  --interval <dur>     Sample interval. Default: 5s
  --output <dir>       Run artifact directory. Default: artifacts/validation/run-<ts>
  --listen <addr>      Bridge listen address. Default: 127.0.0.1:18433
  --seed <n>           Bridge RNG seed. Default: 42
  --count <n>          Initial ledger records. Default: 140
  --window <n>         Bridge retention window (records; recent/windowed list
                       semantics, MECH-0002 C2). Default: unbounded
  --bridge <path>      Path to the friday-bridge binary
  --log-level <level>  RUST_LOG for the bridge. Default: info
  --coverage-threshold <pct>  Minimum observation coverage (%) for a VALID
                             run. Below it the run is marked INVALID.
                             Default: 99
  --no-analyze         Run without analyzing
  --append             Allow an existing, non-empty --output dir (extend an
                       aborted/interrupted run rather than refuse it)
  --analyze <dir>      Analyze an existing run directory and exit
  --help               Show this help`,
  );
}

function runId() {
  const d = new Date();
  const p = (n) => String(n).padStart(2, '0');
  return `run-${d.getFullYear()}${p(d.getMonth() + 1)}${p(d.getDate())}-${p(d.getHours())}${p(d.getMinutes())}${p(d.getSeconds())}`;
}

function log(...parts) {
  process.stderr.write(`[friday-validate] ${parts.join(' ')}\n`);
}

// --analyze mode -----------------------------------------------------------

if (args.analyze) {
  const code = runAnalyzer(args.analyze);
  process.exitCode = code;
  process.exit(code);
}
if (args.help) {
  usage();
  process.exit(0);
}

// -- config -----------------------------------------------------------------

const durationMs = (() => {
  try {
    return parseDuration(args.duration);
  } catch (e) {
    log(`bad --duration: ${e.message}`);
    process.exit(2);
  }
})();
const intervalMs = (() => {
  try {
    return parseDuration(args.interval, { defaultMs: 5000 });
  } catch (e) {
    log(`bad --interval: ${e.message}`);
    process.exit(2);
  }
})();

const listen = args.listen ?? '127.0.0.1:18433';
const seed = args.seed ? Number(args.seed) : 42;
const count = args.count ? Number(args.count) : 140;
const window = args.window ? Number(args.window) : null;
const bridgeBin =
  args.bridge ?? join(repoRoot, 'target', 'debug', `friday-bridge${isWin ? '.exe' : ''}`);
const logLevel = args['log-level'] ?? 'info';
const coverageThreshold = args['coverage-threshold'] ? Number(args['coverage-threshold']) : 99;
const outDir = resolve(args.output ?? join(repoRoot, 'artifacts', 'validation', runId()));
const collectorPath = join(here, 'collector.mjs');
const analyzePath = join(here, 'analyze.mjs');

if (!existsSync(bridgeBin)) {
  log(`bridge binary not found: ${bridgeBin}`);
  log('build it with: cargo build -p friday-bridge');
  process.exit(2);
}
if (
  existsSync(outDir) &&
  readdirSync(outDir).length > 0 &&
  args.append !== true
) {
  log(`output directory already contains data: ${outDir}`);
  log('refusing to run into a non-empty directory — stale samples would corrupt the measurement.');
  log('use a fresh --output dir, or pass --append to intentionally extend an existing run.');
  process.exit(2);
}
mkdirSync(outDir, { recursive: true });

let gitCommit = null;
try {
  gitCommit = execFileSync('git', ['rev-parse', 'HEAD'], { cwd: repoRoot, encoding: 'utf8' }).trim();
} catch {
  // not a git checkout — record null
}

// -- children ---------------------------------------------------------------

const state = {
  shuttingDown: false,
  bridgeExit: null,
  collectorExit: null,
  startedAt: Date.now(),
  observationGaps: [],
};

const bridgeLog = createWriteStream(join(outDir, 'bridge.log'), { flags: 'a' });
const collectorLog = createWriteStream(join(outDir, 'collector.log'), { flags: 'a' });

const bridgeArgs = ['--listen', listen, '--seed', String(seed), '--count', String(count)];
if (window !== null) bridgeArgs.push('--window', String(window));

const bridge = spawn(bridgeBin, bridgeArgs, {
  cwd: repoRoot,
  env: { ...process.env, RUST_LOG: logLevel },
  stdio: ['ignore', 'pipe', 'pipe'],
});
bridge.stdout.on('data', (d) => bridgeLog.write(d));
bridge.stderr.on('data', (d) => bridgeLog.write(d));
bridge.on('exit', (code, signal) => {
  if (state.shuttingDown) return;
  state.bridgeExit = { code, signal };
  log('BRIDGE EXITED EARLY', JSON.stringify({ code, signal }));
  setTimeout(finish, 100);
});

const collector = spawn(
  process.execPath,
  [
    collectorPath,
    '--url',
    `ws://${listen}`,
    '--bridge-pid',
    String(bridge.pid),
    '--interval',
    String(intervalMs),
    '--out',
    join(outDir, 'samples.jsonl'),
    '--tag',
    'collector',
  ],
  { cwd: here, env: process.env, stdio: ['ignore', 'pipe', 'pipe'] },
);
collector.stdout.on('data', (d) => collectorLog.write(d));
collector.stderr.on('data', (d) => collectorLog.write(d));
collector.on('exit', (code, signal) => {
  if (state.shuttingDown) return;
  state.collectorExit = { code, signal };
  log('COLLECTOR EXITED EARLY', JSON.stringify({ code, signal }));
  setTimeout(finish, 100);
});

log('bridge pid', bridge.pid, `-> ${bridgeBin}`);
log('output', outDir);
log(`running for ${formatDuration(durationMs)} (interval ${formatDuration(intervalMs)})`);

// -- progress + shutdown ----------------------------------------------------

const durationTimer = setTimeout(finish, durationMs);
const progressTimer = setInterval(() => {
  const min = Math.round((Date.now() - state.startedAt) / 60000);
  log(`running ${min}m / ${formatDuration(durationMs)}`);
}, 60000);

// Observation watchdog — a sample must arrive every `intervalMs`. If none has
// (checked against the samples file's mtime), the run is losing observability.
// Host sleep freezes this timer too, so the first tick after wake catches the
// hole; the manifest's coverage block is the authoritative record.
const watchdogTimer = setInterval(() => {
  try {
    const samplesPath = join(outDir, 'samples.jsonl');
    if (!existsSync(samplesPath)) return;
    const mtime = statSync(samplesPath).mtimeMs;
    const gap = Date.now() - mtime;
    if (gap > intervalMs * 3) {
      log(
        `OBSERVATION GAP expected ${formatDuration(intervalMs)} actual ${fmtDurationMs(gap)}`,
      );
      state.observationGaps.push({ at: new Date().toISOString(), gap_ms: gap });
    }
  } catch {
    // best-effort watchdog; the manifest coverage still records the truth
  }
}, Math.max(intervalMs, 5000));

process.on('SIGINT', () => {
  log('SIGINT — stopping');
  finish();
});
process.on('SIGTERM', () => {
  log('SIGTERM — stopping');
  finish();
});

function stopChild(child, timeoutMs) {
  return new Promise((resolve) => {
    if (!child || child.exitCode !== null) return resolve();
    let done = false;
    const t = setTimeout(() => {
      if (!done) {
        done = true;
        try {
          child.kill('SIGKILL');
        } catch {
          // already gone
        }
        resolve();
      }
    }, timeoutMs);
    child.on('exit', () => {
      if (!done) {
        done = true;
        clearTimeout(t);
        resolve();
      }
    });
    try {
      child.kill('SIGTERM');
    } catch {
      resolve();
    }
  });
}

function runAnalyzer(dir) {
  const a = spawn(process.execPath, [analyzePath, dir], { stdio: 'inherit' });
  return new Promise((resolve) => {
    a.on('exit', (code) => resolve(code ?? 1));
  });
}

async function finish() {
  if (state.shuttingDown) return;
  state.shuttingDown = true;
  clearTimeout(durationTimer);
  clearInterval(progressTimer);
  clearInterval(watchdogTimer);

  log('stopping collector');
  await stopChild(collector, 5000);
  log('stopping bridge');
  await stopChild(bridge, 8000);

  const endedAt = new Date();
  const observation = (() => {
    try {
      const samplesPath = join(outDir, 'samples.jsonl');
      if (!existsSync(samplesPath)) return null;
      const samples = readFileSync(samplesPath, 'utf8')
        .trim()
        .split('\n')
        .filter(Boolean)
        .map((line) => JSON.parse(line));
      const cov = observationCoverage(samples, { expectedMs: durationMs, intervalMs });
      return { ...cov, status: observationStatus(cov, coverageThreshold), threshold_percent: coverageThreshold };
    } catch {
      return null;
    }
  })();
  const manifest = {
    run_id: basenameOf(outDir),
    protocol_version: 2,
    bridge_id: 'friday-bridge-demo',
    listen,
    seed,
    count,
    bridge_binary: bridgeBin,
    duration_ms: durationMs,
    interval_ms: intervalMs,
    coverage_threshold_percent: coverageThreshold,
    started_at: new Date(state.startedAt).toISOString(),
    ended_at: endedAt.toISOString(),
    elapsed_ms: endedAt.getTime() - state.startedAt,
    observation,
    observation_gaps_watchdog: state.observationGaps,
    bridge_exited_early: Boolean(state.bridgeExit),
    bridge_exit: state.bridgeExit,
    collector_exit: state.collectorExit,
    git_commit: gitCommit,
    files: ['bridge.log', 'collector.log', 'samples.jsonl'],
  };
  writeFileSync(join(outDir, 'manifest.json'), `${JSON.stringify(manifest, null, 2)}\n`);
  log('manifest written', join(outDir, 'manifest.json'));
  if (observation && observation.status !== 'VALID') {
    log(
      `OBSERVATION INVALID: ${observation.coverage_percent}% coverage (< ${coverageThreshold}%), largest gap ${fmtDurationMs(observation.largest_gap_ms)}`,
    );
  }

  const crashed = Boolean(state.bridgeExit);
  if (args['no-analyze']) {
    log(crashed ? 'RESULT: FAIL (bridge crashed)' : 'RESULT: run complete (no analysis requested)');
    process.exitCode = crashed ? 1 : 0;
    return;
  }
  log('analyzing');
  const code = await runAnalyzer(outDir);
  log(crashed ? 'RESULT: FAIL (bridge crashed)' : `RESULT: analyzer exit ${code}`);
  process.exitCode = crashed || code !== 0 ? 1 : 0;
  log('report', join(outDir, 'report.md'));
}

function basenameOf(p) {
  return p.split(/[\\/]/).filter(Boolean).pop() ?? p;
}
