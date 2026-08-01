/**
 * FRIDAY Engineering Console — reactive controller.
 *
 * The console subscribes to a `ConsoleDataSource` and merges the typed
 * `ConsoleFrame` stream into a single snapshot. Frames are event-sourced:
 * hello announces the bridge, snapshot seeds the full read models, deltas
 * merge changes, heartbeats carry round-trip latency, errors surface bridge
 * problems. Sequence gaps are counted as dropped frames.
 *
 * Today the source is the mock; tomorrow it is the runtime bridge. Screens
 * only subscribe via `useConsole` and never care which provider is behind the
 * interface.
 */

import { useSyncExternalStore } from 'react';
import type {
  ConsoleDataSource,
  ConsoleFrame,
  DashboardDTO,
  DecisionRecord,
  DecisionTimeline,
  MetricsReport,
  RuntimeStatus,
  SnapshotFrame,
  DeltaFrame,
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
  protocol: 0,
  frames: 0,
  dropped: 0,
  fps: 0,
  round_trip_ms: null,
  records: 0,
  last_error: null,
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
let lastSeq: number | null = null;
let dropped = 0;

function notify(): void {
  listeners.forEach((l) => l());
}

function measuredFps(): number {
  const now = Date.now();
  while (arrivals.length && now - arrivals[0] > 1000) arrivals.shift();
  return arrivals.length;
}

function mergeSlices(frame: SnapshotFrame | DeltaFrame): void {
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
      ...snapshot.runtime,
      connected: true,
      round_trip_ms: frame.latency_ms ?? snapshot.runtime.round_trip_ms,
      frames: lastSeq ?? snapshot.runtime.frames,
      dropped,
      fps: measuredFps(),
      records: count,
    },
  };
}

function publish(frame: ConsoleFrame): void {
  arrivals.push(Date.now());
  if (lastSeq != null && frame.sequence > lastSeq) {
    dropped += frame.sequence - lastSeq - 1;
  }
  lastSeq = frame.sequence;

  switch (frame.kind) {
    case 'hello':
      snapshot = {
        ...snapshot,
        runtime: {
          ...snapshot.runtime,
          connected: true,
          bridge: frame.bridge,
          protocol: frame.protocol,
          last_error: null,
        },
      };
      break;
    case 'snapshot':
    case 'delta':
      mergeSlices(frame);
      break;
    case 'heartbeat':
      snapshot = {
        ...snapshot,
        runtime: {
          ...snapshot.runtime,
          connected: true,
          round_trip_ms: frame.latency_ms ?? snapshot.runtime.round_trip_ms,
          frames: lastSeq ?? snapshot.runtime.frames,
          dropped,
          fps: measuredFps(),
        },
      };
      break;
    case 'error':
      snapshot = {
        ...snapshot,
        runtime: {
          ...snapshot.runtime,
          connected: true,
          frames: lastSeq ?? snapshot.runtime.frames,
          last_error: frame.message,
        },
      };
      break;
  }
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
  arrivals.length = 0;
  lastSeq = null;
  dropped = 0;
  snapshot = {
    ...EMPTY,
    status: 'loading',
    runtime: { ...EMPTY_RUNTIME, bridge: next.kind },
  };
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
