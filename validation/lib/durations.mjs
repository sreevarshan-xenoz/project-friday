// Duration parsing for the Validation Harness CLIs.
// Accepts bare seconds (`300`, `90`), `90s`, `30m`, `8h`, `1d`, `500ms`.

const UNITS = { ms: 1, s: 1000, m: 60000, h: 3600000, d: 86400000 };

export function parseDuration(input, { defaultMs = 2 * 3600000 } = {}) {
  if (input === undefined || input === true) return defaultMs;
  const s = String(input).trim().toLowerCase();
  const m = s.match(/^(\d+(?:\.\d+)?)(ms|s|m|h|d)?$/);
  if (!m) throw new Error(`invalid duration: ${input}`);
  const unit = m[2] ?? 's';
  return Math.round(parseFloat(m[1]) * UNITS[unit]);
}

export function formatDuration(ms) {
  if (ms < 1000) return `${ms}ms`;
  const s = ms / 1000;
  if (s < 60) return `${s}s`;
  const m = s / 60;
  if (m < 60) return `${Math.floor(m)}m ${Math.round(s % 60)}s`;
  const h = m / 60;
  return `${Math.floor(h)}h ${Math.round(m % 60)}m`;
}
