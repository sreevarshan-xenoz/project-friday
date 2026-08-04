// Observation coverage — how much of the *intended* run was actually observed.
//
// An experiment's verdict is only as good as its observation. A run that
// sleeps, hangs, or loses its collector for hours is not the experiment that
// was designed: "24h wall clock" where the host slept 10h is ~14h observed,
// and that is a different run. This module turns that intuition into numbers
// so ADRs can gate on a single coverage figure.
//
// Definition:
//   expected_duration_ms — the run as designed (manifest `duration_ms`).
//   observed_duration_ms — wall time between the first and last sample, minus
//                          every inter-sample gap that exceeds tolerance
//                          (3x the sample interval, unless overridden). Each
//                          interval contributes at most `tolerance_ms`, so a
//                          9h sleep counts as ~15s, not 9h.
//   coverage_percent    — observed / expected, capped at 100.
//   largest_gap_ms      — longest single inter-sample gap.
//   status              — VALID when coverage >= threshold, else INVALID.
//
// The collector emits samples roughly every `intervalMs`; any delta larger
// than `intervalMs * toleranceFactor` is a hole in the observation, whether
// caused by host sleep, a frozen event loop, or a dropped writer.

export function observationCoverage(samples, { expectedMs, intervalMs, toleranceFactor = 3 }) {
  if (!Array.isArray(samples) || samples.length < 2) {
    return null;
  }
  const ts = samples.map((s) => s.ts);
  const tolerated = Math.max(1, intervalMs * toleranceFactor);

  let observedMs = 0;
  let largestGapMs = 0;
  let gapCount = 0;
  for (let i = 1; i < ts.length; i += 1) {
    const delta = ts[i] - ts[i - 1];
    if (delta > tolerated) {
      gapCount += 1;
      if (delta > largestGapMs) largestGapMs = delta;
    }
    observedMs += Math.min(delta, tolerated);
  }

  const expectedMsNum = Number(expectedMs);
  const coveragePct =
    expectedMsNum > 0 ? Math.min(100, (observedMs / expectedMsNum) * 100) : null;

  return {
    expected_duration_ms: expectedMsNum,
    observed_duration_ms: observedMs,
    coverage_percent: coveragePct === null ? null : Math.round(coveragePct * 10) / 10,
    largest_gap_ms: largestGapMs,
    gap_count: gapCount,
  };
}

export function observationStatus(coverage, thresholdPct) {
  if (!coverage || coverage.coverage_percent === null) return 'UNKNOWN';
  return coverage.coverage_percent >= thresholdPct ? 'VALID' : 'INVALID';
}

export function fmtDurationMs(ms) {
  if (!Number.isFinite(ms)) return '—';
  const s = ms / 1000;
  if (s < 90) return `${Math.round(s)}s`;
  const m = s / 60;
  if (m < 90) return `${Math.round(m)}m`;
  const h = m / 60;
  if (h < 48) return `${(Math.round(h * 10) / 10).toFixed(1)}h`;
  const d = h / 24;
  return `${(Math.round(d * 10) / 10).toFixed(1)}d`;
}
