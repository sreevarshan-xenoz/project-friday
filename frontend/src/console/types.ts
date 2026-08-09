/**
 * FRIDAY Engineering Console — read-model DTOs.
 *
 * Shape-accurate TypeScript mirrors of the frozen Rust contracts:
 *   - services/plugin-runtime/src/contract.rs (record, plan, outcome, feedback)
 *   - services/experience/src/lib.rs        (timeline, explanation, metrics, replay)
 *
 * These types are the stable interface the console consumes. A real bridge
 * (WebSocket/Tauri) later serializes the same shapes; the screens never change.
 */

export type CorrelationId = string;
export type Capability = string;

export type Urgency = 'low' | 'normal' | 'high' | 'critical';
export type SecurityLevel = 'low' | 'medium' | 'high' | 'maximum';

export interface SelectionContext {
  urgency: Urgency;
  budget: number;
  offline: boolean;
  security_level: SecurityLevel;
  user_preferences: Record<string, string>;
  preferred_providers: string[];
}

export interface CapabilityRequest {
  capability: Capability;
  parameters: Record<string, string>;
  permissions: string[];
  correlation_id: CorrelationId;
  context: SelectionContext;
}

export type EvaluatorKind =
  | 'Health'
  | 'Policy'
  | 'Profile'
  | 'Context'
  | 'Experience'
  | 'Reputation';

export interface EvaluatorSignal {
  source: EvaluatorKind;
  score: number;
  confidence: number;
  explanation: string;
}

export interface ScoredCandidate {
  plugin_id: string;
  plugin_name: string;
  reliability_score: number;
  availability_score: number;
  latency_penalty: number;
  cost_penalty: number;
  policy_bonus: number;
  profile_match_score: number;
  context_fit_score: number;
  weighted_score: number;
  selected: boolean;
  rejection_reason: string | null;
}

export interface PlanningRationale {
  candidates: ScoredCandidate[];
  selected_operator_id: string;
  composite_score: number;
  explanation: string;
  correlation_id: CorrelationId;
  evaluator_evidence: EvaluatorSignal[];
}

export interface ExecutionPlan {
  plan_id: CorrelationId;
  version: number;
  capability: Capability;
  operator_id: string;
  estimated_latency_ms: number;
  estimated_cost: number;
  estimated_success_probability: number;
  estimated_risk: number;
  confidence: number;
  fallback_chain: ExecutionPlan[];
  rationale: PlanningRationale;
}

export interface CostEstimate {
  operator_id: string;
  estimated_latency_ms: number;
  estimated_cost: number;
  estimated_success_probability: number;
  estimated_risk: number;
  confidence: number;
}

export interface PlanOutcome {
  latency_ms: number;
  cost: number;
  success: boolean;
  error: string | null;
  completed_steps: number;
  cancelled: boolean;
  timeout: boolean;
  retries: number;
}

export interface DecisionReflection {
  latency_variance: number;
  cost_variance: number;
  success_accurate: boolean;
  failure_reason: string | null;
  lessons: string[];
}

export interface DecisionRecord {
  id: CorrelationId;
  timestamp_ms: number;
  capability: Capability;
  context: SelectionContext;
  parameters: Record<string, string>;
  plan: ExecutionPlan;
  outcome: PlanOutcome;
  reflection: DecisionReflection | null;
}

export interface PlannerFeedback {
  capability: Capability;
  operator_id: string;
  latency_error_ms: number;
  cost_error: number;
  success_bias: number;
  latency_adjustment_ms: number;
  cost_adjustment: number;
  success_adjustment: number;
  confidence_adjustment: number;
  trigger_model_update: boolean;
  timestamp_ms: number;
}

export interface ExperienceKey {
  capability: Capability;
  operator_id: string;
}

export interface Experience {
  key: ExperienceKey;
  sample_size: number;
  success_rate: number;
  success_count: number;
  failure_count: number;
  average_latency_ms: number;
  p50_latency_ms: number;
  p95_latency_ms: number;
  p99_latency_ms: number;
  last_execution_ms: number | null;
  last_failure_ms: number | null;
  last_failure_reason: string | null;
  consecutive_successes: number;
  consecutive_failures: number;
  since_ms: number;
}

export interface Reputation {
  operator_id: string;
  reliability: number;
  confidence: number;
  total_observations: number;
  since_ms: number;
  last_updated_ms: number;
  decay_factor: number;
}

export interface CalibrationOffsets {
  latency_offset: number;
  cost_offset: number;
  success_offset: number;
  confidence_offset: number;
}

export type ExplanationReason =
  | 'HighestCompositeScore'
  | 'PolicyConstraint'
  | 'HigherReliability'
  | 'BetterHistoricalLatency'
  | 'CalibrationAdjustment'
  | 'FallbackSelection';

export interface CandidateExplanation {
  operator_id: string;
  composite_score: number;
  policy_bonus: number;
  profile_match_score: number;
  context_fit_score: number;
  selected: boolean;
  rejection_reason: string | null;
}

export interface ExperienceFactor {
  sample_size: number;
  success_rate: number;
  average_latency_ms: number;
  last_failure_reason: string | null;
}

export interface ReputationFactor {
  reliability: number;
  confidence: number;
  total_observations: number;
}

export interface CalibrationFactor {
  latency_offset: number;
  cost_offset: number;
  success_offset: number;
  confidence_offset: number;
}

export interface DecisionExplanation {
  plan_id: CorrelationId;
  capability: Capability;
  selected_operator_id: string;
  reasons: ExplanationReason[];
  candidates: CandidateExplanation[];
  policy_factors: string[];
  experience_factor: ExperienceFactor | null;
  reputation_factor: ReputationFactor | null;
  calibration_factor: CalibrationFactor | null;
  narrative: string;
}

export interface DecisionTimeline {
  correlation_id: CorrelationId;
  request: CapabilityRequest;
  plan: ExecutionPlan;
  prediction: CostEstimate;
  outcome: PlanOutcome;
  record: DecisionRecord;
  experience: Experience | null;
  reputation: Reputation | null;
  feedback: PlannerFeedback;
  calibration: CalibrationOffsets | null;
  explanation: DecisionExplanation;
}

export interface PredictionAccuracy {
  decisions: number;
  correct_predictions: number;
  accuracy: number;
  mean_latency_error_ms: number;
  mean_cost_error: number;
  mean_success_bias: number;
}

export interface CalibrationMetrics {
  decisions_with_calibration: number;
  mean_latency_offset: number;
  mean_cost_offset: number;
  mean_success_offset: number;
  mean_confidence_offset: number;
  converging: boolean;
}

export interface FallbackMetrics {
  decisions: number;
  decisions_with_fallback_chain: number;
  fallback_rate: number;
  avg_fallback_depth: number;
  failures: number;
  failure_rate: number;
}

export interface ReputationMetrics {
  decisions: number;
  decisions_with_reputation: number;
  coverage: number;
  avg_reliability: number;
  avg_confidence: number;
  total_observations: number;
  distinct_operators: number;
}

export interface ExperienceMetrics {
  decisions: number;
  decisions_with_experience: number;
  coverage: number;
  distinct_pairs: number;
  total_samples: number;
  avg_sample_size: number;
  avg_success_rate: number;
  avg_latency_ms: number;
}

export interface MetricsReport {
  decisions: number;
  prediction: PredictionAccuracy;
  calibration: CalibrationMetrics;
  fallback: FallbackMetrics;
  reputation: ReputationMetrics;
  experience: ExperienceMetrics;
}

export interface PredictionDelta {
  latency_delta_ms: number;
  cost_delta: number;
  success_probability_delta: number;
  confidence_delta: number;
}

export interface HistoricalSnapshot {
  plan: ExecutionPlan;
  prediction: CostEstimate;
  explanation: DecisionExplanation;
  outcome: PlanOutcome;
}

export interface ReplayEvaluation {
  plan: ExecutionPlan;
  prediction: CostEstimate;
  explanation: DecisionExplanation;
  ranked_candidates: CostEstimate[];
}

export interface ReplayDifferences {
  operator_changed: boolean;
  decision_still_optimal: boolean;
  prediction_delta: PredictionDelta;
  reputation_delta: number | null;
  calibration_delta: number | null;
  explanation_delta: string[];
}

export interface ReplayReport {
  correlation_id: CorrelationId;
  capability: Capability;
  historical: HistoricalSnapshot;
  replay: ReplayEvaluation;
  differences: ReplayDifferences;
  replay_timestamp_ms: number;
}

// ---------------------------------------------------------------------------
// Engineering Console DTOs (the bridge contract)
// ---------------------------------------------------------------------------

/**
 * The eleven-phase executive cycle, mirroring
 * `services/goal-execution/src/executive.rs`. `Investigate` is the M7
 * evidence gate: the cycle enters it for focused goals that must know
 * something before planning.
 */
export type ExecutivePhaseName =
  | 'Observe'
  | 'Attend'
  | 'Investigate'
  | 'Plan'
  | 'Simulate'
  | 'Execute'
  | 'Verify'
  | 'Reflect'
  | 'Consolidate'
  | 'Learn'
  | 'Idle';

export type PhaseStatus = 'done' | 'running' | 'waiting' | 'failed';

export interface ExecutivePhaseState {
  name: ExecutivePhaseName;
  status: PhaseStatus;
}

export interface WorldState {
  files: number;
  processes: number;
  capabilities: number;
  policies: number;
  episodes: number;
  confidence: number;
}

/**
 * An information channel a goal may require before execution — the
 * lowercase label of `InvestigationChannel` (services/investigation).
 */
export type InvestigationChannel =
  | 'web'
  | 'docs'
  | 'codeintel'
  | 'memory'
  | 'git'
  | 'filesystem';

/** One evidence branch: the channel, the auditable "why", and its strength. */
export interface InformationRequirement {
  channel: InvestigationChannel;
  /** Which keyword(s) triggered the requirement. */
  rationale: string;
  /** Evidence strength (0..1). */
  confidence: number;
}

/**
 * The "watch FRIDAY think" view: goal → evidence branches → phases.
 * Mirrors `friday_investigation::InvestigationGraph` (carried by the bridge
 * in `DashboardDto.graph`).
 */
export interface InvestigationGraphDTO {
  goal: string;
  requirements: InformationRequirement[];
  /** e.g. `['plan', 'execute']` or `['investigate', 'plan', 'execute']`. */
  phases: string[];
}

export interface DashboardDTO {
  mission: string;
  goal_progress: number;
  phase: string;
  executive: ExecutivePhaseState[];
  world: WorldState;
  recent: DecisionTimeline[];
  /** The live investigation trace for the current goal (null while loading). */
  graph: InvestigationGraphDTO | null;
}

/**
 * Bridge-wide counters mirrored from `services/bridge/src/protocol.rs`
 * (`BridgeStatsDto`). Carried on hello + every heartbeat so the Runtime card
 * can show how the bridge itself is behaving (frames, bytes, resyncs).
 */
export interface BridgeStats {
  /** Consoles currently connected. */
  connections: number;
  /** Total connections accepted since the bridge started. */
  reconnects: number;
  /** Automatic (lagged) + client-requested resyncs served. */
  resyncs: number;
  /** Frames written to clients (server-wide). */
  frames_sent: number;
  /** Serialized bytes written to clients (server-wide). */
  bytes_sent: number;
  /** Wire size of the last snapshot frame sent. */
  snapshot_size_bytes: number;
  /** Wire size of the last delta frame sent. */
  delta_size_bytes: number;
}

/**
 * Live status of the data source — what the Dashboard's Runtime card shows.
 * `bridge` is the provider kind ('mock', and later 'runtime'); `frames` is the
 * highest sequence seen; `dropped` is the count of sequence gaps (lost
 * frames); `reconnects` counts hello frames after the first (a new connection
 * epoch); `fps` is the measured frame arrival rate; `round_trip_ms` is the
 * latest measured round-trip; `avg_rtt_ms`/`peak_rtt_ms` are over the session;
 * `snapshot_bytes`/`delta_bytes` come from the bridge's own counters.
 */
export interface RuntimeStatus {
  connected: boolean;
  bridge: string;
  protocol: number;
  frames: number;
  dropped: number;
  reconnects: number;
  fps: number;
  round_trip_ms: number | null;
  avg_rtt_ms: number | null;
  peak_rtt_ms: number | null;
  snapshot_bytes: number | null;
  delta_bytes: number | null;
  records: number;
  last_error: string | null;
}

export type ConsoleFrameKind =
  | 'hello'
  | 'snapshot'
  | 'delta'
  | 'heartbeat'
  | 'error'
  | 'capabilities';

export const CONSOLE_PROTOCOL_VERSION = 1;

// ---------------------------------------------------------------------------
// Capability handshake + structured error taxonomy (Bridge v2, Phases 1+2)
// Mirrors `services/bridge/src/protocol.rs`.
// ---------------------------------------------------------------------------

/**
 * Every capability the protocol can name. The bridge advertises a
 * `CapabilitySet` in its hello; the console enables the subset it wants.
 * `gzip`/`zstd`/`binary`/`auth` are forward-declared transport capabilities
 * that arrive with later phases — the handshake is the feature gate.
 */
export type CapabilityName =
  | 'delta'
  | 'heartbeat'
  | 'resync'
  | 'errors'
  | 'metrics'
  | 'replay'
  | 'gzip'
  | 'zstd'
  | 'binary'
  | 'auth';

/**
 * A single advertised or negotiated capability. Named `BridgeCapability` to
 * avoid colliding with the frozen read-model `Capability` (a string) that the
 * planning DTOs use throughout.
 */
export interface BridgeCapability {
  name: CapabilityName;
  version: number;
  /** True when part of the `required` set; informational on the wire. */
  required: boolean;
}

/**
 * What this bridge speaks and what a console may enable. The console answers
 * hello with `{"type":"hello_response","capabilities":{name: bool}}`; the
 * server acks with a `capabilities` frame carrying the negotiated set.
 */
export interface CapabilitySet {
  protocol_version: number;
  required: BridgeCapability[];
  optional: BridgeCapability[];
}

/** Machine-readable error classification, mirrored from `ErrorCode`. */
export type ProtocolErrorCode =
  | 'unsupported_version'
  | 'unsupported_capability'
  | 'authentication_failed'
  | 'compression_negotiation_failed'
  | 'invalid_frame'
  | 'invalid_sequence'
  | 'resync_required'
  | 'rate_limited'
  | 'internal_bridge_error';

/** How bad a problem is; the console decides how loudly to surface it. */
export type ErrorSeverity = 'info' | 'warning' | 'error' | 'critical';

/**
 * Every frame carries metadata: `kind` (so consumers can switch without
 * guessing), a monotonic `sequence`, the bridge clock `timestamp`, the
 * `source`, and the bridge's `protocol` version + identity.
 */
export interface ConsoleFrameBase {
  kind: ConsoleFrameKind;
  sequence: number;
  timestamp: number;
  source: string;
  protocol: number;
  bridge: string;
}

/** Sent once on connect: announces the bridge + protocol version + counters. */
export interface HelloFrame extends ConsoleFrameBase {
  kind: 'hello';
  bridge_stats?: BridgeStats | null;
  /** The capability set this bridge advertises (Bridge v2 handshake). */
  capabilities?: CapabilitySet | null;
}

/** Full read-model snapshot, published right after hello and on demand. */
export interface SnapshotFrame extends ConsoleFrameBase {
  kind: 'snapshot';
  dashboard: DashboardDTO;
  timelines: DecisionTimeline[];
  metrics: MetricsReport;
  ledger: DecisionRecord[];
  latency_ms: number | null;
}

/**
 * Additive delta: carries only the slices that changed (e.g. `{ ledger }`
 * after an append, or `{ dashboard }` after the executive phase advances).
 */
export interface DeltaFrame extends ConsoleFrameBase {
  kind: 'delta';
  dashboard?: DashboardDTO;
  timelines?: DecisionTimeline[];
  metrics?: MetricsReport;
  ledger?: DecisionRecord[];
  latency_ms?: number | null;
}

/** Liveness frame; `latency_ms` is the measured round-trip time. */
export interface HeartbeatFrame extends ConsoleFrameBase {
  kind: 'heartbeat';
  latency_ms: number | null;
  bridge_stats?: BridgeStats | null;
}

/**
 * The bridge hit a problem; `message` explains it. `code`/`severity`/
 * `recoverable`/`recommended_action` are the machine-readable taxonomy so the
 * console can react programmatically instead of parsing strings. Does not
 * drop the stream.
 */
export interface ErrorFrame extends ConsoleFrameBase {
  kind: 'error';
  message: string;
  code?: ProtocolErrorCode | null;
  severity?: ErrorSeverity | null;
  /** Can the client simply retry the same request? */
  recoverable?: boolean | null;
  /** What the client should do next, in machine-friendly words. */
  recommended_action?: string | null;
}

/**
 * The server's acknowledgement of a successful negotiation: `required` is
 * unchanged, `optional` is the subset the console enabled. Sent right after
 * the console's `hello_response`, before the snapshot.
 */
export interface CapabilitiesFrame extends ConsoleFrameBase {
  kind: 'capabilities';
  capabilities: CapabilitySet;
}

export type ConsoleFrame =
  | HelloFrame
  | SnapshotFrame
  | DeltaFrame
  | HeartbeatFrame
  | ErrorFrame
  | CapabilitiesFrame;

/**
 * The provider seam between the Engineering Console and FRIDAY.
 *
 * React only ever talks to read models through this interface. The mock
 * implementation satisfies it today; a runtime implementation (WebSocket
 * bridge / Tauri commands) satisfies it tomorrow. Screens never change.
 *
 * Streaming is the primary path: `connect()` publishes hello + snapshot, then
 * pushes typed `ConsoleFrame`s (deltas, heartbeats, errors). Consumers
 * `switch(frame.kind)`. Request/response (`get*`) methods remain for one-shot
 * reads.
 */
export interface ConsoleDataSource {
  readonly kind: string;
  /** Open the connection. Resolves after hello + the initial snapshot frame. */
  connect(): Promise<void>;
  /** Close the connection and drop subscribers. */
  disconnect(): void;
  /** Subscribe to typed frames. Returns an unsubscribe fn. */
  subscribe(listener: (frame: ConsoleFrame) => void): () => void;
  getDashboard(): Promise<DashboardDTO>;
  getTimelines(): Promise<DecisionTimeline[]>;
  getMetrics(): Promise<MetricsReport>;
  getLedger(): Promise<DecisionRecord[]>;
}
