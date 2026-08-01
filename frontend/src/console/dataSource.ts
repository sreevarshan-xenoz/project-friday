/**
 * FRIDAY Engineering Console — data sources.
 *
 * React never talks to FRIDAY directly. It consumes the frozen read models
 * through the `ConsoleDataSource` interface:
 *
 *   ConsoleDataSource
 *   ├── MockConsoleDataSource    (synthetic ledger, same DTOs as the runtime)
 *   └── RuntimeConsoleDataSource (Week 2: ReadModelBridge -> WebSocket)
 *
 * Replace the provider, never the screens.
 */
import type { ConsoleDataSource, DashboardDTO, DecisionRecord, DecisionTimeline, MetricsReport } from './types';
import { computeMetrics, executiveCycle, generateLedger, worldState } from './mock';

export type { ConsoleDataSource };

export class MockConsoleDataSource implements ConsoleDataSource {
  readonly kind = 'mock';

  private readonly timelines: DecisionTimeline[];
  private readonly metrics: MetricsReport;

  constructor(count = 140, seed = 42) {
    this.timelines = generateLedger(count, seed);
    this.metrics = computeMetrics(this.timelines);
  }

  async getDashboard(): Promise<DashboardDTO> {
    return {
      mission: 'Mission K-731 · Plan and execute the next objective safely',
      goal_progress: 0.72,
      phase: 'EXECUTING',
      executive: executiveCycle(['Observe', 'Attend']),
      world: worldState(this.timelines),
      recent: this.timelines.slice(-6).reverse(),
    };
  }

  async getTimelines(): Promise<DecisionTimeline[]> {
    return this.timelines;
  }

  async getMetrics(): Promise<MetricsReport> {
    return this.metrics;
  }

  async getLedger(): Promise<DecisionRecord[]> {
    return this.timelines.map((t) => t.record);
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
