//! Wire protocol for the FRIDAY ReadModelBridge.
//!
//! Shape-accurate DTO mirrors of the frozen Rust read models, matching
//! `frontend/src/console/types.ts` field-for-field. The bridge owns these
//! types because the frozen read models (`DecisionTimeline`, `MetricsReport`,
//! `DecisionRecord`, ...) carry no serde derives — see
//! `services/experience/src/lib.rs` and `services/plugin-runtime/src/contract.rs`.
//!
//! Every frame carries metadata: `kind` (so consumers can switch without
//! guessing), a monotonic `sequence`, the bridge clock `timestamp`, the
//! `source`, and the bridge's `protocol` version + identity.
//!
//! The protocol is additive: a `DeltaFrame` carries the full replacement
//! value of each slice that changed (e.g. `{ dashboard }` after the executive
//! phase advances, or `{ ledger, metrics }` after a record is appended).
//! Consumers merge frames; they never see partial patches.

use std::collections::HashMap;

use friday_experience::{
    CalibrationFactor as FrozenCalibrationFactor, CalibrationMetrics, CalibrationOffsets,
    CandidateExplanation as FrozenCandidateExplanation, DecisionExplanation, DecisionTimeline,
    ExperienceFactor as FrozenExperienceFactor, ExperienceMetrics, ExplanationReason,
    FallbackMetrics, MetricsReport, PredictionAccuracy, ReputationFactor as FrozenReputationFactor,
    ReputationMetrics,
};
use friday_plugin_runtime::{
    Capability, CapabilityRequest, CostEstimate, DecisionRecord, EvaluatorKind, EvaluatorSignal,
    ExecutionPlan, Experience as FrozenExperience, ExperienceKey, PlanOutcome, PlannerFeedback,
    PlanningRationale, Reputation as FrozenReputation, ScoredCandidate, SelectionContext, Urgency,
    SecurityLevel,
};
use serde::{Deserialize, Serialize};

/// The console protocol version. Both ends must agree before streaming.
pub const PROTOCOL_VERSION: u32 = 1;

// ---------------------------------------------------------------------------
// Read-model DTOs
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SelectionContextDto {
    pub urgency: String,
    pub budget: f64,
    pub offline: bool,
    pub security_level: String,
    pub user_preferences: HashMap<String, String>,
    pub preferred_providers: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CapabilityRequestDto {
    pub capability: String,
    pub parameters: HashMap<String, String>,
    pub permissions: Vec<String>,
    pub correlation_id: String,
    pub context: SelectionContextDto,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EvaluatorSignalDto {
    pub source: String,
    pub score: f64,
    pub confidence: f64,
    pub explanation: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ScoredCandidateDto {
    pub plugin_id: String,
    pub plugin_name: String,
    pub reliability_score: f64,
    pub availability_score: f64,
    pub latency_penalty: f64,
    pub cost_penalty: f64,
    pub policy_bonus: f64,
    pub profile_match_score: f64,
    pub context_fit_score: f64,
    pub weighted_score: f64,
    pub selected: bool,
    pub rejection_reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PlanningRationaleDto {
    pub candidates: Vec<ScoredCandidateDto>,
    pub selected_operator_id: String,
    pub composite_score: f64,
    pub explanation: String,
    pub correlation_id: String,
    pub evaluator_evidence: Vec<EvaluatorSignalDto>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExecutionPlanDto {
    pub plan_id: String,
    pub version: u32,
    pub capability: String,
    pub operator_id: String,
    pub estimated_latency_ms: u64,
    pub estimated_cost: f64,
    pub estimated_success_probability: f64,
    pub estimated_risk: f64,
    pub confidence: f64,
    pub fallback_chain: Vec<ExecutionPlanDto>,
    pub rationale: PlanningRationaleDto,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CostEstimateDto {
    pub operator_id: String,
    pub estimated_latency_ms: u64,
    pub estimated_cost: f64,
    pub estimated_success_probability: f64,
    pub estimated_risk: f64,
    pub confidence: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PlanOutcomeDto {
    pub latency_ms: u64,
    pub cost: f64,
    pub success: bool,
    pub error: Option<String>,
    pub completed_steps: u64,
    pub cancelled: bool,
    pub timeout: bool,
    pub retries: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DecisionReflectionDto {
    pub latency_variance: f64,
    pub cost_variance: f64,
    pub success_accurate: bool,
    pub failure_reason: Option<String>,
    pub lessons: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DecisionRecordDto {
    pub id: String,
    pub timestamp_ms: u64,
    pub capability: String,
    pub context: SelectionContextDto,
    pub parameters: HashMap<String, String>,
    pub plan: ExecutionPlanDto,
    pub outcome: PlanOutcomeDto,
    pub reflection: Option<DecisionReflectionDto>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PlannerFeedbackDto {
    pub capability: String,
    pub operator_id: String,
    pub latency_error_ms: i64,
    pub cost_error: f64,
    pub success_bias: f64,
    pub latency_adjustment_ms: f64,
    pub cost_adjustment: f64,
    pub success_adjustment: f64,
    pub confidence_adjustment: f64,
    pub trigger_model_update: bool,
    pub timestamp_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExperienceKeyDto {
    pub capability: String,
    pub operator_id: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExperienceDto {
    pub key: ExperienceKeyDto,
    pub sample_size: u64,
    pub success_rate: f64,
    pub success_count: u64,
    pub failure_count: u64,
    pub average_latency_ms: f64,
    pub p50_latency_ms: u64,
    pub p95_latency_ms: u64,
    pub p99_latency_ms: u64,
    pub last_execution_ms: Option<u64>,
    pub last_failure_ms: Option<u64>,
    pub last_failure_reason: Option<String>,
    pub consecutive_successes: u64,
    pub consecutive_failures: u64,
    pub since_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReputationDto {
    pub operator_id: String,
    pub reliability: f64,
    pub confidence: f64,
    pub total_observations: u64,
    pub since_ms: u64,
    pub last_updated_ms: u64,
    pub decay_factor: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CalibrationOffsetsDto {
    pub latency_offset: f64,
    pub cost_offset: f64,
    pub success_offset: f64,
    pub confidence_offset: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CandidateExplanationDto {
    pub operator_id: String,
    pub composite_score: f64,
    pub policy_bonus: f64,
    pub profile_match_score: f64,
    pub context_fit_score: f64,
    pub selected: bool,
    pub rejection_reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExperienceFactorDto {
    pub sample_size: u64,
    pub success_rate: f64,
    pub average_latency_ms: f64,
    pub last_failure_reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReputationFactorDto {
    pub reliability: f64,
    pub confidence: f64,
    pub total_observations: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CalibrationFactorDto {
    pub latency_offset: f64,
    pub cost_offset: f64,
    pub success_offset: f64,
    pub confidence_offset: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DecisionExplanationDto {
    pub plan_id: String,
    pub capability: String,
    pub selected_operator_id: String,
    pub reasons: Vec<String>,
    pub candidates: Vec<CandidateExplanationDto>,
    pub policy_factors: Vec<String>,
    pub experience_factor: Option<ExperienceFactorDto>,
    pub reputation_factor: Option<ReputationFactorDto>,
    pub calibration_factor: Option<CalibrationFactorDto>,
    pub narrative: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DecisionTimelineDto {
    pub correlation_id: String,
    pub request: CapabilityRequestDto,
    pub plan: ExecutionPlanDto,
    pub prediction: CostEstimateDto,
    pub outcome: PlanOutcomeDto,
    pub record: DecisionRecordDto,
    pub experience: Option<ExperienceDto>,
    pub reputation: Option<ReputationDto>,
    pub feedback: PlannerFeedbackDto,
    pub calibration: Option<CalibrationOffsetsDto>,
    pub explanation: DecisionExplanationDto,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PredictionAccuracyDto {
    pub decisions: usize,
    pub correct_predictions: usize,
    pub accuracy: f64,
    pub mean_latency_error_ms: f64,
    pub mean_cost_error: f64,
    pub mean_success_bias: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CalibrationMetricsDto {
    pub decisions_with_calibration: usize,
    pub mean_latency_offset: f64,
    pub mean_cost_offset: f64,
    pub mean_success_offset: f64,
    pub mean_confidence_offset: f64,
    pub converging: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FallbackMetricsDto {
    pub decisions: usize,
    pub decisions_with_fallback_chain: usize,
    pub fallback_rate: f64,
    pub avg_fallback_depth: f64,
    pub failures: usize,
    pub failure_rate: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReputationMetricsDto {
    pub decisions: usize,
    pub decisions_with_reputation: usize,
    pub coverage: f64,
    pub avg_reliability: f64,
    pub avg_confidence: f64,
    pub total_observations: u64,
    pub distinct_operators: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExperienceMetricsDto {
    pub decisions: usize,
    pub decisions_with_experience: usize,
    pub coverage: f64,
    pub distinct_pairs: usize,
    pub total_samples: u64,
    pub avg_sample_size: f64,
    pub avg_success_rate: f64,
    pub avg_latency_ms: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MetricsReportDto {
    pub decisions: usize,
    pub prediction: PredictionAccuracyDto,
    pub calibration: CalibrationMetricsDto,
    pub fallback: FallbackMetricsDto,
    pub reputation: ReputationMetricsDto,
    pub experience: ExperienceMetricsDto,
}

/// The ten-phase executive cycle, mirroring
/// `services/goal-execution/src/executive.rs`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExecutivePhaseStateDto {
    pub name: String,
    pub status: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorldStateDto {
    pub files: u64,
    pub processes: u64,
    pub capabilities: u64,
    pub policies: u64,
    pub episodes: u64,
    pub confidence: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DashboardDto {
    pub mission: String,
    pub goal_progress: f64,
    pub phase: String,
    pub executive: Vec<ExecutivePhaseStateDto>,
    pub world: WorldStateDto,
    pub recent: Vec<DecisionTimelineDto>,
}

// ---------------------------------------------------------------------------
// Frames
// ---------------------------------------------------------------------------

/// Everything a snapshot or delta publishes, so new connections can always
/// receive a full view.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SourceSnapshotDto {
    pub dashboard: DashboardDto,
    pub timelines: Vec<DecisionTimelineDto>,
    pub metrics: MetricsReportDto,
    pub ledger: Vec<DecisionRecordDto>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HelloFrame {
    pub sequence: u64,
    pub timestamp: u64,
    pub source: String,
    pub protocol: u32,
    pub bridge: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SnapshotFrame {
    pub sequence: u64,
    pub timestamp: u64,
    pub source: String,
    pub protocol: u32,
    pub bridge: String,
    pub dashboard: DashboardDto,
    pub timelines: Vec<DecisionTimelineDto>,
    pub metrics: MetricsReportDto,
    pub ledger: Vec<DecisionRecordDto>,
    pub latency_ms: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DeltaFrame {
    pub sequence: u64,
    pub timestamp: u64,
    pub source: String,
    pub protocol: u32,
    pub bridge: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dashboard: Option<DashboardDto>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timelines: Option<Vec<DecisionTimelineDto>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metrics: Option<MetricsReportDto>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ledger: Option<Vec<DecisionRecordDto>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub latency_ms: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HeartbeatFrame {
    pub sequence: u64,
    pub timestamp: u64,
    pub source: String,
    pub protocol: u32,
    pub bridge: String,
    pub latency_ms: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ErrorFrame {
    pub sequence: u64,
    pub timestamp: u64,
    pub source: String,
    pub protocol: u32,
    pub bridge: String,
    pub message: String,
}

/// The console protocol: `kind` is the tag; metadata + payload follow.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Frame {
    Hello(HelloFrame),
    Snapshot(SnapshotFrame),
    Delta(DeltaFrame),
    Heartbeat(HeartbeatFrame),
    Error(ErrorFrame),
}

// ---------------------------------------------------------------------------
// Conversions from frozen types
// ---------------------------------------------------------------------------

fn capability_str(c: &Capability) -> String {
    c.as_str().to_string()
}

fn urgency_str(u: &Urgency) -> &'static str {
    match u {
        Urgency::Low => "low",
        Urgency::Normal => "normal",
        Urgency::High => "high",
        Urgency::Critical => "critical",
    }
}

fn security_str(s: &SecurityLevel) -> &'static str {
    match s {
        SecurityLevel::Low => "low",
        SecurityLevel::Medium => "medium",
        SecurityLevel::High => "high",
        SecurityLevel::Maximum => "maximum",
    }
}

fn evaluator_str(k: &EvaluatorKind) -> &'static str {
    match k {
        EvaluatorKind::Health => "Health",
        EvaluatorKind::Policy => "Policy",
        EvaluatorKind::Profile => "Profile",
        EvaluatorKind::Context => "Context",
        EvaluatorKind::Experience => "Experience",
        EvaluatorKind::Reputation => "Reputation",
    }
}

fn reason_str(r: &ExplanationReason) -> &'static str {
    match r {
        ExplanationReason::HighestCompositeScore => "HighestCompositeScore",
        ExplanationReason::PolicyConstraint => "PolicyConstraint",
        ExplanationReason::HigherReliability => "HigherReliability",
        ExplanationReason::BetterHistoricalLatency => "BetterHistoricalLatency",
        ExplanationReason::CalibrationAdjustment => "CalibrationAdjustment",
        ExplanationReason::FallbackSelection => "FallbackSelection",
    }
}

impl From<&SelectionContext> for SelectionContextDto {
    fn from(c: &SelectionContext) -> Self {
        Self {
            urgency: urgency_str(&c.urgency).to_string(),
            budget: c.budget,
            offline: c.offline,
            security_level: security_str(&c.security_level).to_string(),
            user_preferences: c.user_preferences.clone(),
            preferred_providers: c.preferred_providers.clone(),
        }
    }
}

impl From<&CapabilityRequest> for CapabilityRequestDto {
    fn from(r: &CapabilityRequest) -> Self {
        Self {
            capability: capability_str(&r.capability),
            parameters: r.parameters.clone(),
            permissions: r.permissions.clone(),
            correlation_id: r.correlation_id.0.clone(),
            context: SelectionContextDto::from(&r.context),
        }
    }
}

impl From<&EvaluatorSignal> for EvaluatorSignalDto {
    fn from(s: &EvaluatorSignal) -> Self {
        Self {
            source: evaluator_str(&s.source).to_string(),
            score: s.score,
            confidence: s.confidence,
            explanation: s.explanation.clone(),
        }
    }
}

impl From<&ScoredCandidate> for ScoredCandidateDto {
    fn from(c: &ScoredCandidate) -> Self {
        Self {
            plugin_id: c.plugin_id.clone(),
            plugin_name: c.plugin_name.clone(),
            reliability_score: c.reliability_score,
            availability_score: c.availability_score,
            latency_penalty: c.latency_penalty,
            cost_penalty: c.cost_penalty,
            policy_bonus: c.policy_bonus,
            profile_match_score: c.profile_match_score,
            context_fit_score: c.context_fit_score,
            weighted_score: c.weighted_score,
            selected: false,
            rejection_reason: None,
        }
    }
}

impl From<&PlanningRationale> for PlanningRationaleDto {
    fn from(r: &PlanningRationale) -> Self {
        let selected = r.selected_operator_id.as_str();
        let candidates = r
            .candidates
            .iter()
            .map(|c| {
                let mut dto = ScoredCandidateDto::from(c);
                dto.selected = c.plugin_id == selected;
                dto
            })
            .collect();
        Self {
            candidates,
            selected_operator_id: r.selected_operator_id.clone(),
            composite_score: r.composite_score,
            explanation: r.explanation.clone(),
            correlation_id: r.correlation_id.0.clone(),
            evaluator_evidence: r.evaluator_evidence.iter().map(EvaluatorSignalDto::from).collect(),
        }
    }
}

impl From<&ExecutionPlan> for ExecutionPlanDto {
    fn from(p: &ExecutionPlan) -> Self {
        Self {
            plan_id: p.plan_id.0.clone(),
            version: p.version,
            capability: capability_str(&p.capability),
            operator_id: p.operator_id.clone(),
            estimated_latency_ms: p.estimated_latency_ms,
            estimated_cost: p.estimated_cost,
            estimated_success_probability: p.estimated_success_probability,
            estimated_risk: p.estimated_risk,
            confidence: p.confidence,
            fallback_chain: p.fallback_chain.iter().map(ExecutionPlanDto::from).collect(),
            rationale: PlanningRationaleDto::from(&p.rationale),
        }
    }
}

impl From<&CostEstimate> for CostEstimateDto {
    fn from(e: &CostEstimate) -> Self {
        Self {
            operator_id: e.operator_id.clone(),
            estimated_latency_ms: e.estimated_latency_ms,
            estimated_cost: e.estimated_cost,
            estimated_success_probability: e.estimated_success_probability,
            estimated_risk: e.estimated_risk,
            confidence: e.confidence,
        }
    }
}

impl From<&PlanOutcome> for PlanOutcomeDto {
    fn from(o: &PlanOutcome) -> Self {
        Self {
            latency_ms: o.latency_ms,
            cost: o.cost,
            success: o.success,
            error: o.error.clone(),
            completed_steps: o.completed_steps,
            cancelled: o.cancelled,
            timeout: o.timeout,
            retries: o.retries,
        }
    }
}

impl From<&friday_plugin_runtime::DecisionReflection> for DecisionReflectionDto {
    fn from(r: &friday_plugin_runtime::DecisionReflection) -> Self {
        Self {
            latency_variance: r.latency_variance,
            cost_variance: r.cost_variance,
            success_accurate: r.success_accurate,
            failure_reason: r.failure_reason.clone(),
            lessons: r.lessons.clone(),
        }
    }
}

impl From<&DecisionRecord> for DecisionRecordDto {
    fn from(r: &DecisionRecord) -> Self {
        Self {
            id: r.id.0.clone(),
            timestamp_ms: r.timestamp_ms,
            capability: capability_str(&r.capability),
            context: SelectionContextDto::from(&r.context),
            parameters: r.parameters.clone(),
            plan: ExecutionPlanDto::from(&r.plan),
            outcome: PlanOutcomeDto::from(&r.outcome),
            reflection: r.reflection.as_ref().map(DecisionReflectionDto::from),
        }
    }
}

impl From<&PlannerFeedback> for PlannerFeedbackDto {
    fn from(f: &PlannerFeedback) -> Self {
        Self {
            capability: capability_str(&f.capability),
            operator_id: f.operator_id.clone(),
            latency_error_ms: f.latency_error_ms,
            cost_error: f.cost_error,
            success_bias: f.success_bias,
            latency_adjustment_ms: f.latency_adjustment_ms,
            cost_adjustment: f.cost_adjustment,
            success_adjustment: f.success_adjustment,
            confidence_adjustment: f.confidence_adjustment,
            trigger_model_update: f.trigger_model_update,
            timestamp_ms: f.timestamp_ms,
        }
    }
}

impl From<&ExperienceKey> for ExperienceKeyDto {
    fn from(k: &ExperienceKey) -> Self {
        Self {
            capability: capability_str(&k.capability),
            operator_id: k.operator_id.clone(),
        }
    }
}

impl From<&FrozenExperience> for ExperienceDto {
    fn from(e: &FrozenExperience) -> Self {
        Self {
            key: ExperienceKeyDto::from(&e.key),
            sample_size: e.sample_size,
            success_rate: e.success_rate,
            success_count: e.success_count,
            failure_count: e.failure_count,
            average_latency_ms: e.average_latency_ms,
            p50_latency_ms: e.p50_latency_ms,
            p95_latency_ms: e.p95_latency_ms,
            p99_latency_ms: e.p99_latency_ms,
            last_execution_ms: e.last_execution_ms,
            last_failure_ms: e.last_failure_ms,
            last_failure_reason: e.last_failure_reason.clone(),
            consecutive_successes: e.consecutive_successes,
            consecutive_failures: e.consecutive_failures,
            since_ms: e.since_ms,
        }
    }
}

impl From<&FrozenReputation> for ReputationDto {
    fn from(r: &FrozenReputation) -> Self {
        Self {
            operator_id: r.operator_id.clone(),
            reliability: r.reliability,
            confidence: r.confidence,
            total_observations: r.total_observations,
            since_ms: r.since_ms,
            last_updated_ms: r.last_updated_ms,
            decay_factor: r.decay_factor,
        }
    }
}

impl From<&CalibrationOffsets> for CalibrationOffsetsDto {
    fn from(c: &CalibrationOffsets) -> Self {
        Self {
            latency_offset: c.latency_offset,
            cost_offset: c.cost_offset,
            success_offset: c.success_offset,
            confidence_offset: c.confidence_offset,
        }
    }
}

impl From<&FrozenCandidateExplanation> for CandidateExplanationDto {
    fn from(c: &FrozenCandidateExplanation) -> Self {
        Self {
            operator_id: c.operator_id.clone(),
            composite_score: c.composite_score,
            policy_bonus: c.policy_bonus,
            profile_match_score: c.profile_match_score,
            context_fit_score: c.context_fit_score,
            selected: c.selected,
            rejection_reason: c.rejection_reason.clone(),
        }
    }
}

impl From<&FrozenExperienceFactor> for ExperienceFactorDto {
    fn from(f: &FrozenExperienceFactor) -> Self {
        Self {
            sample_size: f.sample_size,
            success_rate: f.success_rate,
            average_latency_ms: f.average_latency_ms,
            last_failure_reason: f.last_failure_reason.clone(),
        }
    }
}

impl From<&FrozenReputationFactor> for ReputationFactorDto {
    fn from(f: &FrozenReputationFactor) -> Self {
        Self {
            reliability: f.reliability,
            confidence: f.confidence,
            total_observations: f.total_observations,
        }
    }
}

impl From<&FrozenCalibrationFactor> for CalibrationFactorDto {
    fn from(f: &FrozenCalibrationFactor) -> Self {
        Self {
            latency_offset: f.latency_offset,
            cost_offset: f.cost_offset,
            success_offset: f.success_offset,
            confidence_offset: f.confidence_offset,
        }
    }
}

impl From<&DecisionExplanation> for DecisionExplanationDto {
    fn from(e: &DecisionExplanation) -> Self {
        Self {
            plan_id: e.plan_id.0.clone(),
            capability: capability_str(&e.capability),
            selected_operator_id: e.selected_operator_id.clone(),
            reasons: e.reasons.iter().map(|r| reason_str(r).to_string()).collect(),
            candidates: e.candidates.iter().map(CandidateExplanationDto::from).collect(),
            policy_factors: e.policy_factors.clone(),
            experience_factor: e.experience_factor.as_ref().map(ExperienceFactorDto::from),
            reputation_factor: e.reputation_factor.as_ref().map(ReputationFactorDto::from),
            calibration_factor: e.calibration_factor.as_ref().map(CalibrationFactorDto::from),
            narrative: e.narrative.clone(),
        }
    }
}

impl From<&DecisionTimeline> for DecisionTimelineDto {
    fn from(t: &DecisionTimeline) -> Self {
        Self {
            correlation_id: t.correlation_id.0.clone(),
            request: CapabilityRequestDto::from(&t.request),
            plan: ExecutionPlanDto::from(&t.plan),
            prediction: CostEstimateDto::from(&t.prediction),
            outcome: PlanOutcomeDto::from(&t.outcome),
            record: DecisionRecordDto::from(&t.record),
            experience: t.experience.as_ref().map(ExperienceDto::from),
            reputation: t.reputation.as_ref().map(ReputationDto::from),
            feedback: PlannerFeedbackDto::from(&t.feedback),
            calibration: t.calibration.as_ref().map(CalibrationOffsetsDto::from),
            explanation: DecisionExplanationDto::from(&t.explanation),
        }
    }
}

impl From<&PredictionAccuracy> for PredictionAccuracyDto {
    fn from(m: &PredictionAccuracy) -> Self {
        Self {
            decisions: m.decisions,
            correct_predictions: m.correct_predictions,
            accuracy: m.accuracy,
            mean_latency_error_ms: m.mean_latency_error_ms,
            mean_cost_error: m.mean_cost_error,
            mean_success_bias: m.mean_success_bias,
        }
    }
}

impl From<&CalibrationMetrics> for CalibrationMetricsDto {
    fn from(m: &CalibrationMetrics) -> Self {
        Self {
            decisions_with_calibration: m.decisions_with_calibration,
            mean_latency_offset: m.mean_latency_offset,
            mean_cost_offset: m.mean_cost_offset,
            mean_success_offset: m.mean_success_offset,
            mean_confidence_offset: m.mean_confidence_offset,
            converging: m.converging,
        }
    }
}

impl From<&FallbackMetrics> for FallbackMetricsDto {
    fn from(m: &FallbackMetrics) -> Self {
        Self {
            decisions: m.decisions,
            decisions_with_fallback_chain: m.decisions_with_fallback_chain,
            fallback_rate: m.fallback_rate,
            avg_fallback_depth: m.avg_fallback_depth,
            failures: m.failures,
            failure_rate: m.failure_rate,
        }
    }
}

impl From<&ReputationMetrics> for ReputationMetricsDto {
    fn from(m: &ReputationMetrics) -> Self {
        Self {
            decisions: m.decisions,
            decisions_with_reputation: m.decisions_with_reputation,
            coverage: m.coverage,
            avg_reliability: m.avg_reliability,
            avg_confidence: m.avg_confidence,
            total_observations: m.total_observations,
            distinct_operators: m.distinct_operators,
        }
    }
}

impl From<&ExperienceMetrics> for ExperienceMetricsDto {
    fn from(m: &ExperienceMetrics) -> Self {
        Self {
            decisions: m.decisions,
            decisions_with_experience: m.decisions_with_experience,
            coverage: m.coverage,
            distinct_pairs: m.distinct_pairs,
            total_samples: m.total_samples,
            avg_sample_size: m.avg_sample_size,
            avg_success_rate: m.avg_success_rate,
            avg_latency_ms: m.avg_latency_ms,
        }
    }
}

impl From<&MetricsReport> for MetricsReportDto {
    fn from(m: &MetricsReport) -> Self {
        Self {
            decisions: m.decisions,
            prediction: PredictionAccuracyDto::from(&m.prediction),
            calibration: CalibrationMetricsDto::from(&m.calibration),
            fallback: FallbackMetricsDto::from(&m.fallback),
            reputation: ReputationMetricsDto::from(&m.reputation),
            experience: ExperienceMetricsDto::from(&m.experience),
        }
    }
}
