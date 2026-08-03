// Cross-platform process sampling for the Validation Harness.
//
// Reports RSS (working set), cumulative CPU seconds, and handle count for a
// single OS process. CPU percent is derived from the delta between two
// consecutive samples so it is an interval rate, not an instantaneous value.
//
// IMPORTANT: sampling is ASYNC (non-blocking). The collector's event loop
// also answers WebSocket pings; a blocking sample would delay pong replies
// and inflate the RTT the bridge measures (a measurement artifact the smoke
// run caught: 5s-interval PowerShell blocks produced ~270ms "RTT" spikes).
//
// Windows: one PowerShell call per sample (WorkingSet64/CPU/HandleCount).
// POSIX: `ps -o rss=,pcpu=` for RSS; CPU-seconds and handles are not available
// without extra privileges, so cpu_percent is reported as null there.

import { execFile } from 'node:child_process';
import { promisify } from 'node:util';

const execFileAsync = promisify(execFile);
const DEAD = { alive: false, rss_bytes: null, cpu_seconds: null, handles: null };

async function sampleWindows(pid) {
  const cmd =
    `$p = Get-Process -Id ${pid} -ErrorAction SilentlyContinue; ` +
    `if ($null -eq $p) { '{}' } else { ` +
    `[pscustomobject]@{ rss=[int64]$p.WorkingSet64; cpu=[double]$p.CPU; handles=[int]$p.HandleCount } | ConvertTo-Json -Compress }`;
  const { stdout } = await execFileAsync(
    'powershell.exe',
    ['-NoProfile', '-NonInteractive', '-ExecutionPolicy', 'Bypass', '-Command', cmd],
    { windowsHide: true, maxBuffer: 65536 },
  );
  const j = JSON.parse(stdout.trim() || '{}');
  if (j && typeof j.rss === 'number' && j.rss >= 0) {
    return {
      alive: true,
      rss_bytes: j.rss,
      cpu_seconds: typeof j.cpu === 'number' ? j.cpu : null,
      handles: typeof j.handles === 'number' ? j.handles : null,
    };
  }
  return DEAD;
}

async function samplePosix(pid) {
  const { stdout } = await execFileAsync('ps', ['-o', 'rss=,pcpu=', '-p', String(pid)], {
    maxBuffer: 65536,
  });
  const m = stdout.match(/^\s*(\d+)\s+[\d.]+\s*$/m);
  if (m) {
    return {
      alive: true,
      rss_bytes: parseInt(m[1], 10) * 1024,
      cpu_seconds: null,
      handles: null,
    };
  }
  return DEAD;
}

/**
 * Sample one process. `prev` is the previous return value (used for the CPU
 * rate); `elapsedMs` is the wall-clock time between the two samples.
 */
export async function sampleProcess(pid, prev, elapsedMs) {
  if (pid === null || pid === undefined || Number.isNaN(pid)) {
    return { ...DEAD, cpu_percent: null };
  }
  let s;
  try {
    s = process.platform === 'win32' ? await sampleWindows(pid) : await samplePosix(pid);
  } catch {
    s = DEAD;
  }

  let cpu_percent = null;
  if (
    s.alive &&
    prev &&
    prev.alive &&
    typeof s.cpu_seconds === 'number' &&
    typeof prev.cpu_seconds === 'number' &&
    elapsedMs > 0
  ) {
    const dt = s.cpu_seconds - prev.cpu_seconds;
    if (dt >= 0) cpu_percent = Math.round((dt / (elapsedMs / 1000)) * 100 * 10) / 10;
  }
  return { ...s, cpu_percent };
}
