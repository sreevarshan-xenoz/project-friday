/**
 * FRIDAY Engineering Console — reactive controller.
 *
 * The console subscribes to a `ConsoleDataSource` and merges streamed
 * `ConsoleDataFrame`s into a single snapshot. Today that source is the mock;
 * tomorrow it is the runtime bridge. Screens only subscribe via `useConsole`
 * and never care which provider is behind the interface.
 */

import { useSyncExternalStore } from 'react';
import type {
  ConsoleDataFrame,
  ConsoleDataSource,
  DashboardDTO,
  DecisionRecord,
  DecisionTimeline,
  MetricsReport,
  RuntimeStatus,
} from './types';
import { MockConsoleDataSource } from './dataSource';

export type ConsoleStatus = 'loading' | 'ready' | 'error';

export interface ConsoleSnapshot {
  status: ConsoleStatus;
  error: string | null;
  runtime: RuntimeStatus;
  count: number;
  dashboard: DashboardDTO;
  timelines: DecisionTimeline[];
  metrics: MetricsReport;
  ledger: DecisionRecord[];
}

const EMPTY_RUNTIME: RuntimeStatus = {
  connected: false,
  bridge: '…',
  latency_ms: null,
  updates_per_sec: 0,
  records: 0,
};

const EMPTY: ConsoleSnapshot = {
  status: 'loading',
  error: null,
  runtime: EMPTY_RUNTIME,
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
let unsubFromSource: (() => void) | null = null;

const arrivals: number[] = [];

function notify(): void {
  listeners.forEach((l) => l());
}

function measuredUpdatesPerSec(): number {
  const now = Date.now();
  while (arrivals.length && now - arrivals[0] > 1000) arrivals.shift();
  return arrivals.length;
}

function publish(frame: ConsoleDataFrame): void {
  arrivals.push(Date.now());
  const count = frame.ledger ? frame.ledger.length : snapshot.count;
  snapshot = {
    ...snapshot,
    status: 'ready',
    error: null,
    dashboard: frame.dashboard ?? snapshot.dashboard,
    timelines: frame.timelines ?? snapshot.timelines,
    metrics: frame.metrics ?? snapshot.metrics,
    ledger: frame.ledger ?? snapshot.ledger,
    count,
    runtime: {
      connected: true,
      bridge: source.kind,
      latency_ms: frame.latency_ms ?? snapshot.runtime.latency_ms,
      updates_per_sec: measuredUpdatesPerSec(),
      records: count,
    },
  };
  notify();
}

export function getConsoleSnapshot(): ConsoleSnapshot {
  return snapshot;
}

export function subscribeConsole(listener: () => void): () => void {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}

/** Swap the provider behind the console (mock today, runtime bridge later). */
export async function setConsoleSource(next: ConsoleDataSource): Promise<void> {
  unsubFromSource?.();
  unsubFromSource = null;
  source = next;
  snapshot = { ...EMPTY, status: 'loading', runtime: { ...EMPTY_RUNTIME, bridge: next.kind } };
  notify();
  try {
    unsubFromSource = source.subscribe(publish);
    await source.connect();
  } catch (err) {
    snapshot = {
      ...EMPTY,
      status: 'error',
      error: err instanceof Error ? err.message : String(err),
      runtime: { ...EMPTY_RUNTIME, bridge: source.kind },
    };
    notify();
  }
}

void setConsoleSource(new MockConsoleDataSource());

export function useConsole(): ConsoleSnapshot {
  return useSyncExternalStore(subscribeConsole, getConsoleSnapshot);
}
