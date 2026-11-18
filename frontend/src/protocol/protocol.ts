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
  | { type: 'auth_challenge'; challenge: string }
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

export type ParsedTaskStatus = 'done' | 'running' | 'waiting' | 'failed' | 'cancelled' | 'paused';

export interface ParsedTask {
  id: string;
  title: string;
  status: ParsedTaskStatus;
  source: string;
}

export interface EvidenceChannel {
  /** The capability prefix / adapter key, e.g. `git`, `web`, `external`. */
  channel: string;
  /** Whether this channel's evidence ended Verified. */
  verified: boolean;
}

/** The kind of a goal result. Mirrors `friday_goal_execution::GoalResultKind`. */
export type GoalResultKind =
  | 'text'
  | 'structured'
  | 'file'
  | 'image'
  | 'tool'
  | 'synthesis'
  | 'error'
  | 'empty';

/** The canonical result of a goal execution (`goal.completed.result`). */
export interface GoalResult {
  kind: GoalResultKind;
  /** Human-readable result text (`''` for `empty`). */
  text: string;
  /** Structured payload for `structured`/`file` kinds. */
  structured?: unknown;
  /** The tool/skill that produced it, when known. */
  tool?: string;
  /** Whether the producing execution reported success. */
  success?: boolean;
}

export interface ParsedEvent {
  kind: string;
  source: string;
  summary: string;
  task: ParsedTask | null;
  /** The goal uuid from lifecycle payloads (`goal.started` / `completed` / `failed`). */
  goalId?: string;
  /** Executive phases completed (`goal.completed.phases`). */
  phases?: number;
  /** Verified evidence actions (`goal.completed.evidence_verified`). */
  evidenceVerified?: number;
  /** Per-channel evidence lifecycle (`goal.completed.channels`). */
  channels?: EvidenceChannel[];
  /** Investigation strategy that produced the plan: `llm` or `heuristic`. */
  strategy?: string;
  /** The provider that served the model answer (`goal.completed.provider`). */
  provider?: string;
  /** The model id that answered (`goal.completed.model`). */
  model?: string;
  /** The goal's actual result — the execution output (`goal.completed.result`). */
  result?: GoalResult;
  /** Failure reason (`goal.failed.error`). */
  error?: string;
  /** `task_id` from control events (`goal.approved.task_id` = the held
   * episode id, distinct from the goal's run id). */
  taskId?: string;
  /** Whether the completed goal was held for a human (`goal.completed.pending_approval`). */
  pendingApproval?: boolean;
  /** The held episode's id — the exact `task_id` a `task.approve` must target
   * (`goal.completed.pending_approval_id`). */
  pendingApprovalId?: string;
}

export function parseEvent(topic: string, payload: unknown): ParsedEvent {
  const obj = (payload && typeof payload === 'object' ? payload : {}) as Record<string, unknown>;
  const kind = (obj._kind as string) ?? topic;
  const source = (obj._source as string) ?? '';
  const summary = (obj.goal ?? obj.message ?? obj.task ?? obj.name ?? kind) as string;
  const task = buildTask(kind, obj);
  return {
    kind,
    source,
    summary,
    task,
    goalId: typeof obj.id === 'string' ? obj.id : undefined,
    phases: typeof obj.phases === 'number' ? obj.phases : undefined,
    evidenceVerified: typeof obj.evidence_verified === 'number' ? obj.evidence_verified : undefined,
    channels: parseChannels(obj.channels),
    strategy: typeof obj.strategy === 'string' ? obj.strategy : undefined,
    provider: typeof obj.provider === 'string' ? obj.provider : undefined,
    model: typeof obj.model === 'string' ? obj.model : undefined,
    result: parseResult(obj.result),
    error: typeof obj.error === 'string' ? obj.error : undefined,
    taskId: typeof obj.task_id === 'string' ? obj.task_id : undefined,
    pendingApproval: obj.pending_approval === true,
    pendingApprovalId: typeof obj.pending_approval_id === 'string' ? obj.pending_approval_id : undefined,
  };
}

const RESULT_KINDS: ReadonlySet<string> = new Set([
  'text',
  'structured',
  'file',
  'image',
  'tool',
  'synthesis',
  'error',
  'empty',
]);

function parseResult(value: unknown): GoalResult | undefined {
  if (!value || typeof value !== 'object') return undefined;
  const o = value as Record<string, unknown>;
  const kind = typeof o.kind === 'string' && RESULT_KINDS.has(o.kind) ? (o.kind as GoalResultKind) : 'text';
  const text = typeof o.text === 'string' ? o.text : '';
  const success = typeof o.success === 'boolean' ? o.success : undefined;
  return { kind, text, success, tool: typeof o.tool === 'string' ? o.tool : undefined, structured: o.structured };
}

function parseChannels(value: unknown): EvidenceChannel[] | undefined {
  if (!Array.isArray(value)) return undefined;
  const out: EvidenceChannel[] = [];
  for (const item of value) {
    if (item && typeof item === 'object') {
      const o = item as Record<string, unknown>;
      const channel = typeof o.channel === 'string' ? o.channel : '';
      if (channel) out.push({ channel, verified: o.verified === true });
    }
  }
  return out.length ? out : undefined;
}

function buildTask(kind: string, obj: Record<string, unknown>): ParsedTask | null {
  let status: ParsedTaskStatus | null = null;
  if (kind.endsWith('.completed') || kind.endsWith('.succeeded')) status = 'done';
  else if (kind.endsWith('.failed') || kind.endsWith('.error')) status = 'failed';
  else if (kind.endsWith('.started') || kind.endsWith('.running')) status = 'running';
  else if (kind.endsWith('.created') || kind.endsWith('.planned')) status = 'waiting';
  else if (kind.endsWith('.cancelled')) status = 'cancelled';
  else if (kind.endsWith('.paused')) status = 'paused';
  if (!status) return null;

  const id = (obj.id ?? obj.task_id ?? obj.goal) as string;
  const title = (obj.goal ?? obj.task ?? obj.name ?? kind) as string;
  if (!id) return null;
  return { id, title, status, source: (obj._source as string) ?? '' };
}
