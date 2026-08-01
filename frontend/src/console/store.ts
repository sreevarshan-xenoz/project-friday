/**
 * FRIDAY Engineering Console — reactive controller.
 *
 * The console is backed by a `ConsoleDataSource`. Today that is the mock;
 * tomorrow it is the runtime bridge. Screens only subscribe via `useConsole`
 * and never care which provider is behind the interface.
 */

import { useSyncExternalStore } from 'react';
import type { DashboardDTO, DecisionRecord, DecisionTimeline, MetricsReport } from './types';
import { MockConsoleDataSource, type ConsoleDataSource } from './dataSource';

export type ConsoleStatus = 'loading' | 'ready' | 'error';

export interface ConsoleSnapshot {
  status: ConsoleStatus;
  error: string | null;
  source: string;
  count: number;
  dashboard: DashboardDTO;
  timelines: DecisionTimeline[];
  metrics: MetricsReport;
  ledger: DecisionRecord[];
}

const EMPTY: ConsoleSnapshot = {
  status: 'loading',
  error: null,
  source: '…',
  count: 0,
  dashboard: {
    mission: '',
    goal_progress: 0,
    phase: '…',
    executive: [],
    world: { files: 0, processes: 0, capabilities: 0, policies: 0, episodes: 0, confidence: 0 },
    recent: [],
  },
  timelines: [],
  metrics: {
    decisions: 0,
    prediction: { decisions: 0, correct_predictions: 0, accuracy: 0, mean_latency_error_ms: 0, mean_cost_error: 0, mean_success_bias: 0 },
    calibration: { decisions_with_calibration: 0, mean_latency_offset: 0, mean_cost_offset: 0, mean_success_offset: 0, mean_confidence_offset: 0, converging: false },
    fallback: { decisions: 0, decisions_with_fallback_chain: 0, fallback_rate: 0, avg_fallback_depth: 0, failures: 0, failure_rate: 0 },
    reputation: { decisions: 0, decisions_with_reputation: 0, coverage: 0, avg_reliability: 0, avg_confidence: 0, total_observations: 0, distinct_operators: 0 },
    experience: { decisions: 0, decisions_with_experience: 0, coverage: 0, distinct_pairs: 0, total_samples: 0, avg_sample_size: 0, avg_success_rate: 0, avg_latency_ms: 0 },
  },
  ledger: [],
};

let snapshot: ConsoleSnapshot = EMPTY;
const listeners = new Set<() => void>();

let source: ConsoleDataSource = new MockConsoleDataSource();
let loading = false;

function notify(): void {
  listeners.forEach((l) => l());
}

export function getConsoleSnapshot(): ConsoleSnapshot {
  return snapshot;
}

export function subscribeConsole(listener: () => void): () => void {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

/** Swap the provider behind the console (mock today, runtime bridge later). */
export function setConsoleSource(next: ConsoleDataSource): void {
  source = next;
  void loadConsole();
}

export async function loadConsole(): Promise<void> {
  if (loading) return;
  loading = true;
  snapshot = { ...EMPTY, status: 'loading', source: source.kind };
  notify();
  try {
    const [dashboard, timelines, metrics, ledger] = await Promise.all([
      source.getDashboard(),
      source.getTimelines(),
      source.getMetrics(),
      source.getLedger(),
    ]);
    snapshot = {
      status: 'ready',
      error: null,
      source: source.kind,
      count: ledger.length,
      dashboard,
      timelines,
      metrics,
      ledger,
    };
  } catch (err) {
    snapshot = { ...EMPTY, status: 'error', error: err instanceof Error ? err.message : String(err), source: source.kind };
  }
  loading = false;
  notify();
}

void loadConsole();

export function useConsole(): ConsoleSnapshot {
  return useSyncExternalStore(subscribeConsole, getConsoleSnapshot);
}
