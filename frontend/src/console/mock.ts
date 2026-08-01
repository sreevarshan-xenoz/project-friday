/**
 * FRIDAY Engineering Console — deterministic mock data source.
 *
 * Generates a synthetic decision ledger and derives the same read models the
 * real engines produce (experience/reputation/calibration accumulation,
 * planner feedback, explanations, metrics). Mirrors the VP-004 generator's
 * LCG approach: same seed => same data.
 *
 * A real bridge replaces this module later; the store and screens only consume
 * the `ConsoleDataSource` interface.
 */

import type {
  CalibrationOffsets,
  Capability,
  ExecutivePhaseName,
  ExecutivePhaseState,
  WorldState,
  CapabilityRequest,
  CandidateExplanation,
  CostEstimate,
  DecisionExplanation,
  DecisionRecord,
  DecisionTimeline,
  EvaluatorSignal,
  ExecutionPlan,
  Experience,
  ExperienceKey,
  ExperienceMetrics,
  MetricsReport,
  PlannerFeedback,
  PlanOutcome,
  PlanningRationale,
  Reputation,
  ScoredCandidate,
  SelectionContext,
  ExplanationReason,
} from './types';

export interface OperatorProfile {
  id: string;
  name: string;
  reliability: number;
  availability: number;
  avg_latency_ms: number;
  avg_cost: number;
  capable: Capability[];
}

export const OPERATORS: OperatorProfile[] = [
  { id: 'filesystem', name: 'Filesystem', reliability: 0.97, availability: 0.99, avg_latency_ms: 12, avg_cost: 0.01, capable: ['read_file', 'write_file', 'list_directory', 'execute_command'] },
  { id: 'shell', name: 'Command Shell', reliability: 0.93, availability: 0.96, avg_latency_ms: 42, avg_cost: 0.03, capable: ['execute_command', 'write_file', 'list_directory'] },
  { id: 'code-reviewer', name: 'Code Reviewer', reliability: 0.91, availability: 0.94, avg_latency_ms: 96, avg_cost: 0.05, capable: ['read_file', 'write_file'] },
  { id: 'web-search', name: 'Web Search', reliability: 0.87, availability: 0.92, avg_latency_ms: 140, avg_cost: 0.08, capable: ['search_web', 'search_memory'] },
  { id: 'browser', name: 'Browser', reliability: 0.84, availability: 0.9, avg_latency_ms: 210, avg_cost: 0.12, capable: ['open_browser', 'search_web'] },
  { id: 'memory-search', name: 'Memory Search', reliability: 0.95, availability: 0.98, avg_latency_ms: 8, avg_cost: 0.02, capable: ['search_memory', 'list_directory'] },
];

export const CAPABILITIES: Capability[] = [
  'execute_command',
  'read_file',
  'write_file',
  'list_directory',
  'search_web',
  'open_browser',
  'search_memory',
];

class Lcg {
  private state: number;
  constructor(seed: number) {
    this.state = seed >>> 0;
  }
  next(): number {
    this.state = (this.state * 1664525 + 1013904223) >>> 0;
    return this.state;
  }
  below(max: number): number {
    return this.next() % max;
  }
  chance(p: number): boolean {
    return (this.next() % 1000) / 1000 < p;
  }
  range(min: number, max: number): number {
    return min + (this.next() % Math.max(1, Math.round((max - min) * 100))) / 100;
  }
}

function clamp(v: number, lo: number, hi: number): number {
  return Math.min(hi, Math.max(lo, v));
}

const BASE_TS = 1_784_000_000_000;
let seq = 0;

function cid(): string {
  return `cid-${seq}`;
}

function candidatesFor(capability: Capability): OperatorProfile[] {
  return OPERATORS.filter((o) => o.capable.includes(capability));
}

function contextFor(rng: Lcg): SelectionContext {
  const urgencies = ['low', 'normal', 'high'] as const;
  const urgency = urgencies[rng.below(3)];
  const budget = rng.below(20) < 3 ? 0.2 : 1.0;
  return {
    urgency,
    budget,
    offline: rng.chance(0.15),
    security_level: rng.chance(0.2) ? 'medium' : 'low',
    user_preferences: {},
    preferred_providers: [],
  };
}

function estimateFor(
  operator: OperatorProfile,
  experience: Experience | null,
  reputation: Reputation | null,
  calibration: CalibrationOffsets | null,
): CostEstimate {
  let success = operator.reliability;
  let latency = operator.avg_latency_ms;
  let confidence = 0.25;
  if (experience) {
    success = experience.success_rate;
    latency = experience.average_latency_ms;
    confidence = Math.min(1, 0.3 + experience.sample_size * 0.015);
  } else if (reputation) {
    success = reputation.reliability;
    confidence = Math.min(1, reputation.confidence + 0.1);
  }
  let cost = operator.avg_cost;
  let risk = (1 - success) * 0.5;
  if (calibration) {
    success += calibration.success_offset;
    latency += calibration.latency_offset;
    cost += calibration.cost_offset;
    confidence += calibration.confidence_offset;
  }
  success = clamp(success, 0.02, 0.99);
  confidence = clamp(confidence, 0.05, 1);
  latency = Math.max(1, Math.round(latency));
  return {
    operator_id: operator.id,
    estimated_latency_ms: Math.round(latency),
    estimated_cost: Math.max(0, cost),
    estimated_success_probability: success,
    estimated_risk: clamp(risk, 0, 1),
    confidence,
  };
}

function scoreCandidate(
  operator: OperatorProfile,
  capability: Capability,
  estimate: CostEstimate,
  rng: Lcg,
): ScoredCandidate {
  const jitter = (rng.below(20) - 10) / 100;
  const reliability_score = clamp(estimate.estimated_success_probability + jitter, 0.05, 0.99);
  const availability_score = clamp(operator.availability + jitter / 2, 0.5, 1);
  const latency_penalty = clamp(1 - estimate.estimated_latency_ms / 400, 0, 1);
  const cost_penalty = clamp(1 - estimate.estimated_cost / 0.25, 0, 1);
  const profile_match_score = clamp(0.6 + jitter, 0.1, 1);
  const context_fit_score = clamp(0.7 + jitter / 2, 0.1, 1);
  const policy_bonus = capability === 'read_file' && operator.id === 'filesystem' ? 0.08 : 0;
  const weighted_score = clamp(
    0.38 * reliability_score +
      0.18 * availability_score +
      0.14 * profile_match_score +
      0.12 * context_fit_score +
      0.09 * latency_penalty +
      0.09 * cost_penalty +
      policy_bonus,
    0,
    1,
  );
  return {
    plugin_id: operator.id,
    plugin_name: operator.name,
    reliability_score,
    availability_score,
    latency_penalty,
    cost_penalty,
    policy_bonus,
    profile_match_score,
    context_fit_score,
    weighted_score,
    selected: false,
    rejection_reason: null,
  };
}

function planFor(
  request: CapabilityRequest,
  experience: Map<string, Experience>,
  reputation: Map<string, Reputation>,
  calibration: Map<string, CalibrationOffsets>,
  rng: Lcg,
): ExecutionPlan {
  const keyFor = (op: OperatorProfile) => `${request.capability}\u0000${op.id}`;
  const ops = candidatesFor(request.capability);
  const ranked = ops
    .map((op) => estimateFor(op, experience.get(keyFor(op)) ?? null, reputation.get(op.id) ?? null, calibration.get(keyFor(op)) ?? null))
    .sort((a, b) => b.estimated_success_probability - a.estimated_success_probability || b.confidence - a.confidence);
  const scored = ops.map((op, i) => scoreCandidate(op, request.capability, ranked[i], rng));
  const best = [...scored].sort((a, b) => b.weighted_score - a.weighted_score)[0];
  best.selected = true;
  const selectedOp = OPERATORS.find((o) => o.id === best.plugin_id)!;
  const bestEstimate = ranked.find((e) => e.operator_id === best.plugin_id)!;

  const evidence: EvaluatorSignal[] = [
    { source: 'Health', score: clamp(selectedOp.availability, 0, 1), confidence: 0.9, explanation: 'operator healthy' },
    { source: 'Policy', score: best.policy_bonus, confidence: 0.8, explanation: 'policy applied' },
    { source: 'Profile', score: best.profile_match_score, confidence: 0.7, explanation: 'profile match' },
    { source: 'Context', score: best.context_fit_score, confidence: 0.7, explanation: 'context fit' },
  ];
  const exp = experience.get(keyFor(selectedOp)) ?? null;
  const rep = reputation.get(selectedOp.id) ?? null;
  if (exp) evidence.push({ source: 'Experience', score: exp.success_rate, confidence: Math.min(1, 0.2 + exp.sample_size * 0.03), explanation: 'historical experience' });
  if (rep) evidence.push({ source: 'Reputation', score: rep.reliability, confidence: rep.confidence, explanation: 'cross-capability trust' });

  const fallbackOps = ops.filter((o) => o.id !== best.plugin_id);
  const fallback_chain = fallbackOps.map((op) => {
    const est = estimateFor(op, experience.get(keyFor(op)) ?? null, reputation.get(op.id) ?? null, calibration.get(keyFor(op)) ?? null);
    return makePlanShell(request, op, est);
  });

  const rationale: PlanningRationale = {
    candidates: scored.map((c) => (c.plugin_id === best.plugin_id ? best : c)),
    selected_operator_id: best.plugin_id,
    composite_score: best.weighted_score,
    explanation: `Selected ${selectedOp.name} with weighted score ${best.weighted_score.toFixed(3)}.`,
    correlation_id: request.correlation_id,
    evaluator_evidence: evidence,
  };

  const plan = makePlanShell(request, selectedOp, bestEstimate);
  plan.fallback_chain = fallback_chain;
  plan.rationale = rationale;
  return plan;
}

function makePlanShell(request: CapabilityRequest, op: OperatorProfile, est: CostEstimate): ExecutionPlan {
  return {
    plan_id: cid(),
    version: 1,
    capability: request.capability,
    operator_id: op.id,
    estimated_latency_ms: est.estimated_latency_ms,
    estimated_cost: est.estimated_cost,
    estimated_success_probability: est.estimated_success_probability,
    estimated_risk: est.estimated_risk,
    confidence: est.confidence,
    fallback_chain: [],
    rationale: {
      candidates: [],
      selected_operator_id: op.id,
      composite_score: est.estimated_success_probability,
      explanation: '',
      correlation_id: request.correlation_id,
      evaluator_evidence: [],
    },
  };
}

function outcomeFor(plan: ExecutionPlan, rng: Lcg): PlanOutcome {
  const success = rng.chance(0.88);
  const latency = Math.max(
    1,
    Math.round(plan.estimated_latency_ms * (0.7 + (rng.below(90) / 100)) + rng.below(25)),
  );
  const cost = Math.max(0, plan.estimated_cost * (0.6 + (rng.below(80) / 100)));
  return {
    latency_ms: latency,
    cost: Number(cost.toFixed(4)),
    success,
    error: success ? null : rng.chance(0.5) ? 'timeout' : 'runtime_error',
    completed_steps: success ? 1 : 0,
    cancelled: false,
    timeout: !success && rng.chance(0.5),
    retries: success ? 0 : rng.below(2),
  };
}

function feedbackFor(plan: ExecutionPlan, outcome: PlanOutcome, ts: number): PlannerFeedback {
  const latency_error_ms = plan.estimated_latency_ms - outcome.latency_ms;
  const cost_error = plan.estimated_cost - outcome.cost;
  const success_actual = outcome.success ? 1 : 0;
  const success_bias = plan.estimated_success_probability - success_actual;
  const correct = outcome.success === (plan.estimated_success_probability >= 0.5);
  const confidence_adjustment = correct ? 0.1 : plan.confidence > 0.7 ? -0.3 : -0.1;
  const trigger_model_update =
    !correct || Math.abs(latency_error_ms) > plan.estimated_latency_ms / 2 || Math.abs(cost_error) > 1.0;
  return {
    capability: plan.capability,
    operator_id: plan.operator_id,
    latency_error_ms,
    cost_error,
    success_bias,
    latency_adjustment_ms: latency_error_ms,
    cost_adjustment: cost_error,
    success_adjustment: success_bias,
    confidence_adjustment,
    trigger_model_update,
    timestamp_ms: ts,
  };
}

function applyCalibration(current: CalibrationOffsets, fb: PlannerFeedback): CalibrationOffsets {
  const rate = 0.3;
  const clampV = (v: number, hi: number) => clamp(v, -hi, hi);
  return {
    latency_offset: clampV(current.latency_offset + fb.latency_adjustment_ms * rate, 100),
    cost_offset: clampV(current.cost_offset + fb.cost_adjustment * rate, 0.5),
    success_offset: clampV(current.success_offset + fb.success_adjustment * rate, 0.3),
    confidence_offset: clampV(current.confidence_offset + fb.confidence_adjustment * rate, 0.5),
  };
}

function isZeroCal(cal: CalibrationOffsets): boolean {
  return cal.latency_offset === 0 && cal.cost_offset === 0 && cal.success_offset === 0 && cal.confidence_offset === 0;
}

function explain(
  plan: ExecutionPlan,
  exp: Experience | null,
  rep: Reputation | null,
  cal: CalibrationOffsets | null,
): DecisionExplanation {
  const reasons: ExplanationReason[] = [];
  const candidates: CandidateExplanation[] = plan.rationale.candidates.map((c) => ({
    operator_id: c.plugin_id,
    composite_score: c.weighted_score,
    policy_bonus: c.policy_bonus,
    profile_match_score: c.profile_match_score,
    context_fit_score: c.context_fit_score,
    selected: c.selected,
    rejection_reason: c.selected ? null : `weighted score ${c.weighted_score.toFixed(3)} below winner`,
  }));
  reasons.push('HighestCompositeScore');
  if (rep && rep.reliability > 0.5) reasons.push('HigherReliability');
  if (exp && exp.average_latency_ms < 120) reasons.push('BetterHistoricalLatency');
  if (cal && !isZeroCal(cal)) reasons.push('CalibrationAdjustment');
  const policy_factors = plan.rationale.candidates
    .filter((c) => c.policy_bonus > 0)
    .map((c) => `prefer ${c.plugin_id} (+${c.policy_bonus.toFixed(2)})`);
  if (policy_factors.length > 0) reasons.push('PolicyConstraint');

  const narrativeParts: string[] = [];
  narrativeParts.push(`Selected ${plan.operator_id} (composite ${plan.rationale.composite_score.toFixed(2)}).`);
  if (exp) narrativeParts.push(`Historical reliability ${exp.success_rate.toFixed(2)} over ${exp.sample_size} samples.`);
  if (rep) narrativeParts.push(`Reputation reliability ${rep.reliability.toFixed(2)}.`);
  if (cal && !isZeroCal(cal)) narrativeParts.push(`Calibration adjusted the estimate.`);
  if (policy_factors.length > 0) narrativeParts.push('Policy rules favored this choice.');

  return {
    plan_id: plan.plan_id,
    capability: plan.capability,
    selected_operator_id: plan.operator_id,
    reasons,
    candidates,
    policy_factors,
    experience_factor: exp
      ? {
          sample_size: exp.sample_size,
          success_rate: exp.success_rate,
          average_latency_ms: exp.average_latency_ms,
          last_failure_reason: exp.last_failure_reason,
        }
      : null,
    reputation_factor: rep
      ? { reliability: rep.reliability, confidence: rep.confidence, total_observations: rep.total_observations }
      : null,
    calibration_factor: cal && !isZeroCal(cal)
      ? { latency_offset: cal.latency_offset, cost_offset: cal.cost_offset, success_offset: cal.success_offset, confidence_offset: cal.confidence_offset }
      : null,
    narrative: narrativeParts.join(' '),
  };
}

function accumulateExperience(
  map: Map<string, Experience>,
  histories: Map<string, number[]>,
  capability: Capability,
  opId: string,
  outcome: PlanOutcome,
  ts: number,
  reason: string | null,
): Experience {
  const key: ExperienceKey = { capability, operator_id: opId };
  const cur = map.get(`${capability}\u0000${opId}`) ?? null;
  const sample_size = (cur?.sample_size ?? 0) + 1;
  const success_count = (cur?.success_count ?? 0) + (outcome.success ? 1 : 0);
  const failure_count = (cur?.failure_count ?? 0) + (outcome.success ? 0 : 1);
  const average_latency_ms = cur ? (cur.average_latency_ms * (sample_size - 1) + outcome.latency_ms) / sample_size : outcome.latency_ms;
  const all = [...(histories.get(`${capability}\u0000${opId}`) ?? []), outcome.latency_ms].sort((a, b) => a - b);
  histories.set(`${capability}\u0000${opId}`, all);
  const p = (q: number) => all[Math.min(all.length - 1, Math.floor(all.length * q))] ?? 0;
  const next: Experience = {
    key,
    sample_size,
    success_rate: success_count / sample_size,
    success_count,
    failure_count,
    average_latency_ms: Number(average_latency_ms.toFixed(1)),
    p50_latency_ms: p(0.5),
    p95_latency_ms: p(0.95),
    p99_latency_ms: p(0.99),
    last_execution_ms: ts,
    last_failure_ms: outcome.success ? (cur?.last_failure_ms ?? null) : ts,
    last_failure_reason: outcome.success ? (cur?.last_failure_reason ?? null) : reason,
    consecutive_successes: outcome.success ? (cur?.consecutive_successes ?? 0) + 1 : 0,
    consecutive_failures: outcome.success ? 0 : (cur?.consecutive_failures ?? 0) + 1,
    since_ms: cur?.since_ms ?? ts,
  };
  map.set(`${capability}\u0000${opId}`, next);
  return next;
}

function accumulateReputation(map: Map<string, Reputation>, opId: string, outcome: PlanOutcome, ts: number): Reputation {
  const cur = map.get(opId) ?? null;
  const total = (cur?.total_observations ?? 0) + 1;
  const alpha = 0.7;
  const observed = outcome.success ? 1 : 0;
  const reliability = cur ? cur.reliability + alpha * (observed - cur.reliability) : operatorReliability(opId);
  const confidence = Math.min(1, total / 20);
  const next: Reputation = {
    operator_id: opId,
    reliability: clamp(reliability, 0.02, 0.99),
    confidence,
    total_observations: total,
    since_ms: cur?.since_ms ?? ts,
    last_updated_ms: ts,
    decay_factor: 0.95,
  };
  map.set(opId, next);
  return next;
}

function operatorReliability(opId: string): number {
  return OPERATORS.find((o) => o.id === opId)?.reliability ?? 0.5;
}

export function generateLedger(count: number, seed = 42): DecisionTimeline[] {
  const rng = new Lcg(seed);
  const experience = new Map<string, Experience>();
  const histories = new Map<string, number[]>();
  const reputation = new Map<string, Reputation>();
  const calibration = new Map<string, CalibrationOffsets>();
  const timelines: DecisionTimeline[] = [];

  for (let i = 0; i < count; i++) {
    seq = i;
    const ts = BASE_TS + i * 4000 + rng.below(3000);
    const capability = CAPABILITIES[rng.below(CAPABILITIES.length)];
    const correlation_id = cid();
    const request: CapabilityRequest = {
      capability,
      parameters: {},
      permissions: [],
      correlation_id,
      context: contextFor(rng),
    };

    const plan = planFor(request, experience, reputation, calibration, rng);
    const keyFor = (opId2: string) => `${capability}\u0000${opId2}`;

    const expSnapshot = experience.get(keyFor(plan.operator_id)) ?? null;
    const repSnapshot = reputation.get(plan.operator_id) ?? null;
    const calSnapshot = calibration.get(keyFor(plan.operator_id)) ?? null;

    const outcome = outcomeFor(plan, rng);
    const feedback = feedbackFor(plan, outcome, ts);

    const explanation = explain(plan, expSnapshot, repSnapshot, calSnapshot);

    const record: DecisionRecord = {
      id: cid(),
      timestamp_ms: ts,
      capability,
      context: request.context,
      parameters: request.parameters,
      plan,
      outcome,
      reflection: null,
    };

    timelines.push({
      correlation_id,
      request,
      plan,
      prediction: {
        operator_id: plan.operator_id,
        estimated_latency_ms: plan.estimated_latency_ms,
        estimated_cost: plan.estimated_cost,
        estimated_success_probability: plan.estimated_success_probability,
        estimated_risk: plan.estimated_risk,
        confidence: plan.confidence,
      },
      outcome,
      record,
      experience: expSnapshot,
      reputation: repSnapshot,
      feedback,
      calibration: calSnapshot && !isZeroCal(calSnapshot) ? calSnapshot : null,
      explanation,
    });

    accumulateExperience(experience, histories, capability, plan.operator_id, outcome, ts, outcome.error);
    accumulateReputation(reputation, plan.operator_id, outcome, ts);
    if (feedback.trigger_model_update) {
      const cur = calibration.get(keyFor(plan.operator_id)) ?? {
        latency_offset: 0,
        cost_offset: 0,
        success_offset: 0,
        confidence_offset: 0,
      };
      calibration.set(keyFor(plan.operator_id), applyCalibration(cur, feedback));
    }
  }
  return timelines;
}

export function computeMetrics(timelines: DecisionTimeline[]): MetricsReport {
  const decisions = timelines.length;
  const correct = timelines.filter((t) => t.outcome.success === (t.prediction.estimated_success_probability >= 0.5)).length;
  const latencyErrors = timelines.map((t) => Math.abs(t.prediction.estimated_latency_ms - t.outcome.latency_ms));
  const costErrors = timelines.map((t) => Math.abs(t.prediction.estimated_cost - t.outcome.cost));
  const successBiases = timelines.map((t) => t.prediction.estimated_success_probability - (t.outcome.success ? 1 : 0));
  const mean = (arr: number[]) => (arr.length ? arr.reduce((a, b) => a + b, 0) / arr.length : 0);

  const calibrated = timelines.filter((t) => t.calibration);
  const mag = (cal: { latency_offset: number; cost_offset: number; success_offset: number; confidence_offset: number }) =>
    Math.abs(cal.latency_offset) + Math.abs(cal.cost_offset) + Math.abs(cal.success_offset) + Math.abs(cal.confidence_offset);
  const half = Math.floor(calibrated.length / 2);
  const converging =
    calibrated.length >= 2
      ? mean(calibrated.slice(0, half).map((t) => mag(t.calibration!))) > mean(calibrated.slice(half).map((t) => mag(t.calibration!)))
      : false;

  const withFallback = timelines.filter((t) => t.plan.fallback_chain.length > 0);
  const failures = timelines.filter((t) => !t.outcome.success).length;

  const withRep = timelines.filter((t) => t.reputation);
  const withExp = timelines.filter((t) => t.experience);
  const distinctOps = new Set(timelines.map((t) => t.plan.operator_id)).size;
  const distinctPairs = new Set(timelines.map((t) => `${t.plan.capability}\u0000${t.plan.operator_id}`)).size;

  const experience: ExperienceMetrics = {
    decisions,
    decisions_with_experience: withExp.length,
    coverage: decisions ? withExp.length / decisions : 0,
    distinct_pairs: distinctPairs,
    total_samples: withExp.reduce((a, t) => a + (t.experience?.sample_size ?? 0), 0),
    avg_sample_size: withExp.length ? mean(withExp.map((t) => t.experience?.sample_size ?? 0)) : 0,
    avg_success_rate: withExp.length ? mean(withExp.map((t) => t.experience?.success_rate ?? 0)) : 0,
    avg_latency_ms: withExp.length ? mean(withExp.map((t) => t.experience?.average_latency_ms ?? 0)) : 0,
  };

  return {
    decisions,
    prediction: {
      decisions,
      correct_predictions: correct,
      accuracy: decisions ? correct / decisions : 0,
      mean_latency_error_ms: mean(latencyErrors),
      mean_cost_error: mean(costErrors),
      mean_success_bias: mean(successBiases),
    },
    calibration: {
      decisions_with_calibration: calibrated.length,
      mean_latency_offset: calibrated.length ? mean(calibrated.map((t) => Math.abs(t.calibration!.latency_offset))) : 0,
      mean_cost_offset: calibrated.length ? mean(calibrated.map((t) => Math.abs(t.calibration!.cost_offset))) : 0,
      mean_success_offset: calibrated.length ? mean(calibrated.map((t) => Math.abs(t.calibration!.success_offset))) : 0,
      mean_confidence_offset: calibrated.length ? mean(calibrated.map((t) => Math.abs(t.calibration!.confidence_offset))) : 0,
      converging,
    },
    fallback: {
      decisions,
      decisions_with_fallback_chain: withFallback.length,
      fallback_rate: decisions ? withFallback.length / decisions : 0,
      avg_fallback_depth: decisions ? withFallback.reduce((a, t) => a + t.plan.fallback_chain.length, 0) / decisions : 0,
      failures,
      failure_rate: decisions ? failures / decisions : 0,
    },
    reputation: {
      decisions,
      decisions_with_reputation: withRep.length,
      coverage: decisions ? withRep.length / decisions : 0,
      avg_reliability: withRep.length ? mean(withRep.map((t) => t.reputation?.reliability ?? 0)) : 0,
      avg_confidence: withRep.length ? mean(withRep.map((t) => t.reputation?.confidence ?? 0)) : 0,
      total_observations: withRep.reduce((a, t) => a + (t.reputation?.total_observations ?? 0), 0),
      distinct_operators: distinctOps,
    },
    experience,
  };
}

/**
 * A static snapshot of the current executive cycle: which of the ten phases
 * (Observe, Attend, Plan, Simulate, Execute, Verify, Reflect, Consolidate,
 * Learn, Idle) have finished, which is running, and which are still waiting.
 */
export function executiveCycle(finished: string[]): ExecutivePhaseState[] {
  const order: ExecutivePhaseName[] = ['Observe', 'Attend', 'Plan', 'Simulate', 'Execute', 'Verify', 'Reflect', 'Consolidate', 'Learn', 'Idle'];
  const running = order[finished.length] ?? 'Idle';
  return order.map((name) => {
    if (finished.includes(name)) return { name, status: 'done' };
    if (name === running) return { name, status: 'running' };
    return { name, status: 'waiting' };
  });
}

export function worldState(timelines: DecisionTimeline[]): WorldState {
  const policies = new Set(timelines.flatMap((t) => t.explanation.policy_factors)).size;
  const confidence = timelines.length
    ? timelines.reduce((a, t) => a + t.prediction.confidence, 0) / timelines.length
    : 0;
  return {
    files: 182,
    processes: 8,
    capabilities: CAPABILITIES.length,
    policies,
    episodes: timelines.length,
    confidence,
  };
}
