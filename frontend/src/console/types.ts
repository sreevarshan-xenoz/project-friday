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
export type SecurityLevel = 'public' | 'sensitive' | 'secret';

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
