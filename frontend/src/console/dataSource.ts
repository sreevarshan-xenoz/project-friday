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
 * Week 2: connects to the real runtime through the ReadModelBridge over
 * WebSocket. Each bridge text frame is a typed `ConsoleFrame` (hello,
 * snapshot, delta, heartbeat, error); we forward them straight to subscribers
 * and merge the payload slices locally so the one-shot `get*` reads work too.
 * The bridge owns the protocol — this client never constructs read models.
 */
const RUNTIME_BRIDGE_URL = 'ws://127.0.0.1:18433';

interface LatestSlices {
  dashboard: DashboardDTO | null;
  timelines: DecisionTimeline[] | null;
  metrics: MetricsReport | null;
  ledger: DecisionRecord[] | null;
  latency_ms: number | null;
}

export class RuntimeConsoleDataSource implements ConsoleDataSource {
  readonly kind = 'runtime';

  private readonly url: string;
  private ws: WebSocket | null = null;
  private listeners = new Set<(frame: ConsoleFrame) => void>();
  private handshake: Promise<void> | null = null;
  private clientSeq = 0;
  private intentionallyClosed = false;
  private reconnectTimer: ReturnType<typeof setTimeout> | null = null;
  private reconnectDelay = 500;
  private latest: LatestSlices = {
    dashboard: null,
    timelines: null,
    metrics: null,
    ledger: null,
    latency_ms: null,
  };

  constructor(url: string = RUNTIME_BRIDGE_URL) {
    this.url = url;
  }

  async connect(): Promise<void> {
    if (this.ws != null) {
      return this.handshake ?? Promise.resolve();
    }
    this.intentionallyClosed = false;
    this.handshake = new Promise<void>((resolve, reject) => {
      let resolved = false;
      try {
        const ws = new WebSocket(this.url);
        this.ws = ws;
        ws.onmessage = (msg) => {
          let frame: ConsoleFrame;
          try {
            frame = JSON.parse(String(msg.data)) as ConsoleFrame;
          } catch {
            return;
          }
          this.merge(frame);
          this.emit(frame);
          if (!resolved && frame.kind === 'snapshot') {
            resolved = true;
            this.reconnectDelay = 500; // healthy again, reset the backoff
            resolve();
            this.handshake = null;
          }
        };
        ws.onerror = () => {
          if (!resolved) reject(new Error(`cannot reach the bridge at ${this.url}`));
        };
        ws.onclose = () => {
          this.ws = null;
          if (this.intentionallyClosed) {
            return;
          }
          if (!resolved) {
            resolved = true;
            this.handshake = null;
            reject(new Error(`bridge closed before the snapshot arrived (${this.url})`));
          } else {
            this.clientSeq += 1;
            this.emit({
              kind: 'error',
              sequence: this.clientSeq,
              timestamp: Date.now(),
              source: this.kind,
              protocol: CONSOLE_PROTOCOL_VERSION,
              bridge: 'runtime',
              message: 'bridge connection closed',
            } as ConsoleFrame);
          }
          this.scheduleReconnect();
        };
      } catch (err) {
        reject(err);
      }
    });
    return this.handshake;
  }

  /**
   * Exponential backoff (500ms → 8s cap) so a dead bridge doesn't hammer the
   * port but recovers the moment it returns. Each reconnect is a new hello →
   * snapshot epoch; the store counts it as a reconnect, not a dropped frame.
   */
  private scheduleReconnect(): void {
    if (this.intentionallyClosed) return;
    if (this.reconnectTimer != null) return;
    const delay = this.reconnectDelay;
    this.reconnectDelay = Math.min(this.reconnectDelay * 2, 8000);
    this.reconnectTimer = setTimeout(() => {
      this.reconnectTimer = null;
      void this.connect().catch(() => {});
    }, delay);
  }

  disconnect(): void {
    this.intentionallyClosed = true;
    if (this.reconnectTimer != null) {
      clearTimeout(this.reconnectTimer);
      this.reconnectTimer = null;
    }
    this.ws?.close();
    this.ws = null;
    this.listeners.clear();
  }

  subscribe(listener: (frame: ConsoleFrame) => void): () => void {
    this.listeners.add(listener);
    return () => {
      this.listeners.delete(listener);
    };
  }

  async getDashboard(): Promise<DashboardDTO> {
    return this.expect('dashboard');
  }

  async getTimelines(): Promise<DecisionTimeline[]> {
    return this.expect('timelines');
  }

  async getMetrics(): Promise<MetricsReport> {
    return this.expect('metrics');
  }

  async getLedger(): Promise<DecisionRecord[]> {
    return this.expect('ledger');
  }

  private expect<K extends keyof LatestSlices>(slice: K): NonNullable<LatestSlices[K]> {
    const value = this.latest[slice];
    if (value == null) {
      throw new Error(`RuntimeConsoleDataSource: no ${slice} yet — connect() first.`);
    }
    return value as NonNullable<LatestSlices[K]>;
  }

  private merge(frame: ConsoleFrame): void {
    switch (frame.kind) {
      case 'snapshot':
        this.latest = {
          dashboard: frame.dashboard,
          timelines: frame.timelines,
          metrics: frame.metrics,
          ledger: frame.ledger,
          latency_ms: frame.latency_ms,
        };
        break;
      case 'delta':
        this.latest = {
          ...this.latest,
          dashboard: frame.dashboard ?? this.latest.dashboard,
          timelines: frame.timelines ?? this.latest.timelines,
          metrics: frame.metrics ?? this.latest.metrics,
          ledger: frame.ledger ?? this.latest.ledger,
          latency_ms: frame.latency_ms ?? this.latest.latency_ms,
        };
        break;
      case 'heartbeat':
        this.latest = { ...this.latest, latency_ms: frame.latency_ms };
        break;
      default:
        break;
    }
  }

  private emit(frame: ConsoleFrame): void {
    this.listeners.forEach((l) => l(frame));
  }
}
