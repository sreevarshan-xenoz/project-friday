/**
 * FRIDAY Mobile Protocol — TypeScript types.
 *
 * Mirrors `services/mobile/src/protocol.rs` and
 * `clients/android/.../protocol/ProtocolModels.kt` exactly.
 * Tag style: `"type"` discriminator, `snake_case` variants.
 */

// ── Client → Server ──────────────────────────────────────

export interface ClientInfo {
  device: string;
  model: string;
  app_version: string;
}

export type ClientMessage =
  | { type: 'hello'; client: ClientInfo; token: string | null; protocol: string }
  | { type: 'auth_response'; code: string; signature: string }
  | { type: 'command'; id: string; method: string; params: unknown }
  | { type: 'command_ack'; id: string; ok: boolean }
  | { type: 'subscribe'; topics: string[] }
  | { type: 'ping' };

// ── Server → Client ──────────────────────────────────────

export interface ServiceStatus {
  state: 'Created' | 'Starting' | 'Healthy' | 'Busy' | 'Recovering' | 'Offline' | 'Disabled';
  since: string;
  health: 'Healthy' | 'Busy' | 'Offline' | 'Recovering' | 'Disabled';
  message: string;
}

export interface SystemHealth {
  subsystems: Record<string, ServiceStatus>;
  latency_ms: number;
}

export interface TelemetrySample {
  cpu_percent: number;
  memory_used_mb: number;
  memory_total_mb: number;
  disk_used_mb: number;
  disk_total_mb: number;
}

export type ServerMessage =
  | { type: 'welcome'; session_id: string; requires_auth: boolean; protocol: string }
  | { type: 'auth_challenge'; pairing_code: string; challenge: string }
  | { type: 'auth_complete'; token: string }
  | { type: 'event'; topic: string; payload: unknown }
  | { type: 'health'; snapshot: SystemHealth }
  | { type: 'telemetry'; sample: TelemetrySample }
  | { type: 'command_result'; id: string; ok: boolean; result: unknown }
  | { type: 'pong' }
  | { type: 'error'; code: string; message: string };

// ── Command methods ──────────────────────────────────────

export const Methods = {
  PLANNER_GOAL: 'planner.goal',
  TASK_CANCEL: 'task.cancel',
  TASK_PAUSE: 'task.pause',
  TASK_APPROVE: 'task.approve',
  SYSTEM_INFO: 'system.info',
} as const;

// ── Parsed event (from bus bridge _kind/_source tags) ────

export type ParsedTaskStatus = 'done' | 'running' | 'waiting' | 'failed';

export interface ParsedTask {
  id: string;
  title: string;
  status: ParsedTaskStatus;
  source: string;
}

export interface ParsedEvent {
  kind: string;
  source: string;
  summary: string;
  task: ParsedTask | null;
}

export function parseEvent(topic: string, payload: unknown): ParsedEvent {
  const obj = (payload && typeof payload === 'object' ? payload : {}) as Record<string, unknown>;
  const kind = (obj._kind as string) ?? topic;
  const source = (obj._source as string) ?? '';
  const summary = (obj.goal ?? obj.message ?? obj.task ?? obj.name ?? kind) as string;
  const task = buildTask(kind, obj);
  return { kind, source, summary, task };
}

function buildTask(kind: string, obj: Record<string, unknown>): ParsedTask | null {
  let status: ParsedTaskStatus | null = null;
  if (kind.endsWith('.completed') || kind.endsWith('.succeeded')) status = 'done';
  else if (kind.endsWith('.failed') || kind.endsWith('.error')) status = 'failed';
  else if (kind.endsWith('.started') || kind.endsWith('.running')) status = 'running';
  else if (kind.endsWith('.created') || kind.endsWith('.planned')) status = 'waiting';
  if (!status) return null;

  const id = (obj.id ?? obj.task_id ?? obj.goal) as string;
  const title = (obj.goal ?? obj.task ?? obj.name ?? kind) as string;
  if (!id) return null;
  return { id, title, status, source: (obj._source as string) ?? '' };
}
