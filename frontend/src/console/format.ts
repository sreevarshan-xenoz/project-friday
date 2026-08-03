import type { ExplanationReason } from './types';

export function fmtPct(v: number, digits = 1): string {
  return `${(v * 100).toFixed(digits)}%`;
}

export function fmtMs(v: number): string {
  if (v >= 1000) return `${(v / 1000).toFixed(2)} s`;
  return `${Math.round(v)} ms`;
}

export function fmtCost(v: number): string {
  return `$${v.toFixed(4)}`;
}

export function fmtTime(ts: number): string {
  return new Date(ts).toLocaleTimeString([], { hour12: false });
}

export function fmtDateTime(ts: number): string {
  return new Date(ts).toLocaleString([], { hour12: false });
}

export function fmtSigned(v: number, digits = 1): string {
  const s = v > 0 ? '+' : '';
  return `${s}${v.toFixed(digits)}`;
}

export function fmtBytes(v: number): string {
  if (v < 1024) return `${v} B`;
  if (v < 1024 * 1024) return `${(v / 1024).toFixed(1)} KB`;
  return `${(v / (1024 * 1024)).toFixed(2)} MB`;
}

export function reasonLabel(r: ExplanationReason): string {
  switch (r) {
    case 'HighestCompositeScore':
      return 'Highest composite score';
    case 'PolicyConstraint':
      return 'Policy constraint';
    case 'HigherReliability':
      return 'Higher reliability';
    case 'BetterHistoricalLatency':
      return 'Better historical latency';
    case 'CalibrationAdjustment':
      return 'Calibration adjustment';
    case 'FallbackSelection':
      return 'Fallback selection';
  }
}
