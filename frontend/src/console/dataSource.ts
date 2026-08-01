/**
 * FRIDAY Engineering Console — data sources.
 *
 * React never talks to FRIDAY directly. It consumes the frozen read models
 * through the `ConsoleDataSource` interface:
 *
 *   ConsoleDataSource
 *   ├── MockConsoleDataSource    (synthetic ledger + live simulated ticks)
 *   └── RuntimeConsoleDataSource (Week 2: ReadModelBridge -> WebSocket)
 *
 * Both providers publish the same typed `ConsoleFrame` protocol — hello,
 * snapshot, delta, heartbeat, error — each carrying metadata (sequence,
 * timestamp, source, protocol, bridge). The mock advances the executive cycle
 * on a timer so the whole pipeline is visible before the bridge exists.
 *
 * Replace the provider, never the screens.
 */
import {
  CONSOLE_PROTOCOL_VERSION,
  type ConsoleDataSource,
  type ConsoleFrame,
  type ConsoleFrameKind,
  type DashboardDTO,
  type DecisionRecord,
  type DecisionTimeline,
  type DeltaFrame,
  type ExecutivePhaseName,
  type HeartbeatFrame,
  type MetricsReport,
  type SnapshotFrame,
} from './types';
import { computeMetrics, executiveCycle, generateLedger, worldState } from './mock';

const TICK_MS = 1200;
const HEARTBEAT_EVERY = 3;
const BRIDGE_ID = 'mock-friday';

const PHASE_ORDER: ExecutivePhaseName[] = [
  'Observe',
  'Attend',
  'Plan',
  'Simulate',
  'Execute',
  'Verify',
  'Reflect',
  'Consolidate',
  'Learn',
  'Idle',
];

export class MockConsoleDataSource implements ConsoleDataSource {
  readonly kind = 'mock';

  private readonly timelines: DecisionTimeline[];
  private readonly metrics: MetricsReport;
  private readonly ledger: DecisionRecord[];

  private listeners = new Set<(frame: ConsoleFrame) => void>();
  private timer: ReturnType<typeof setInterval> | null = null;
  private progress = 0;
  private tickCount = 0;
  private seq = 0;

  constructor(count = 140, seed = 42) {
    this.timelines = generateLedger(count, seed);
    this.metrics = computeMetrics(this.timelines);
    this.ledger = this.timelines.map((t) => t.record);
  }

  async connect(): Promise<void> {
    this.emit(this.frame('hello'));
    this.emit(this.snapshot());
    if (this.timer == null) {
      this.timer = setInterval(() => this.tick(), TICK_MS);
    }
  }

  disconnect(): void {
    if (this.timer != null) {
      clearInterval(this.timer);
      this.timer = null;
    }
    this.listeners.clear();
  }

  subscribe(listener: (frame: ConsoleFrame) => void): () => void {
    this.listeners.add(listener);
    return () => {
      this.listeners.delete(listener);
    };
  }

  async getDashboard(): Promise<DashboardDTO> {
    return this.buildDashboard();
  }

  async getTimelines(): Promise<DecisionTimeline[]> {
    return this.timelines;
  }

  async getMetrics(): Promise<MetricsReport> {
    return this.metrics;
  }

  async getLedger(): Promise<DecisionRecord[]> {
    return this.ledger;
  }

  private frame(kind: ConsoleFrameKind, extra: Record<string, unknown> = {}): ConsoleFrame {
    this.seq += 1;
    return {
      kind,
      sequence: this.seq,
      timestamp: Date.now(),
      source: this.kind,
      protocol: CONSOLE_PROTOCOL_VERSION,
      bridge: BRIDGE_ID,
      ...extra,
    } as ConsoleFrame;
  }

  private snapshot(): SnapshotFrame {
    return this.frame('snapshot', {
      dashboard: this.buildDashboard(),
      timelines: this.timelines,
      metrics: this.metrics,
      ledger: this.ledger,
      latency_ms: this.simulatedLatency(8, 14),
    }) as SnapshotFrame;
  }

  private tick(): void {
    this.progress = (this.progress + 1) % (PHASE_ORDER.length + 1);
    this.tickCount += 1;
    if (this.tickCount % HEARTBEAT_EVERY === 0) {
      this.emit(this.frame('heartbeat', { latency_ms: this.simulatedLatency(5, 16) }) as HeartbeatFrame);
      return;
    }
    this.emit(this.frame('delta', { dashboard: this.buildDashboard(), latency_ms: this.simulatedLatency(6, 18) }) as DeltaFrame);
  }

  private emit(frame: ConsoleFrame): void {
    this.listeners.forEach((l) => l(frame));
  }

  private buildDashboard(): DashboardDTO {
    return {
      mission: 'Mission K-731 · Plan and execute the next objective safely',
      goal_progress: 0.72,
      phase: 'EXECUTING',
      executive: executiveCycle(PHASE_ORDER.slice(0, this.progress)),
      world: worldState(this.timelines),
      recent: this.timelines.slice(-6).reverse(),
    };
  }

  private simulatedLatency(min: number, max: number): number {
    return min + Math.random() * (max - min);
  }
}

/**
 * Week 2: connects to the real runtime through the ReadModelBridge. Until the
 * bridge exists this refuses to serve data instead of guessing.
 */
export class RuntimeConsoleDataSource implements ConsoleDataSource {
  readonly kind = 'runtime';

  private static notReady(): never {
    throw new Error('RuntimeConsoleDataSource: the Week 2 ReadModelBridge is not connected yet.');
  }

  async connect(): Promise<void> {
    return RuntimeConsoleDataSource.notReady();
  }

  disconnect(): void {}

  subscribe(_listener: (frame: ConsoleFrame) => void): () => void {
    return () => {};
  }

  async getDashboard(): Promise<DashboardDTO> {
    return RuntimeConsoleDataSource.notReady();
  }

  async getTimelines(): Promise<DecisionTimeline[]> {
    return RuntimeConsoleDataSource.notReady();
  }

  async getMetrics(): Promise<MetricsReport> {
    return RuntimeConsoleDataSource.notReady();
  }

  async getLedger(): Promise<DecisionRecord[]> {
    return RuntimeConsoleDataSource.notReady();
  }
}
