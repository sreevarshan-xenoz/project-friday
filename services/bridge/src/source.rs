//! Read-model sources for the bridge.
//!
//! A `RuntimeSource` is the bridge's only connection to FRIDAY read models.
//! It answers two questions:
//!
//!   1. `snapshot()` — what does the runtime know right now?
//!   2. `tick()`     — what changed since the last tick?
//!
//! The bridge itself is read-only: it never plans, learns, or aggregates
//! beyond what the frozen engines already produce. It just serializes what
//! the runtime already knows.
//!
//! `DemoRuntimeSource` stands in for a live runtime until the daemon surfaces
//! its ledger through this same seam. It generates a synthetic ledger and
//! derives every read model through the **frozen** engines (ExperienceEngine,
//! ReputationEngine, TimelineBuilder, MetricsEngine) — the exact pipeline a
//! real runtime bridge would run over a real ledger.

use friday_experience::{
    CalibrationMetrics, CalibrationStore, ExperienceEngine, ExperienceMetrics, ExperienceStore,
    FallbackMetrics, MetricsEngine, MetricsReport, PredictionAccuracy, ReputationEngine,
    ReputationMetrics, ReputationStore, TimelineBuilder,
};
use friday_plugin_runtime::{
    Capability, CorrelationId, DecisionLedger, DecisionRecord, ExecutionPlan, EvaluatorKind,
    EvaluatorSignal, PlanOutcome, PlanningRationale, ScoredCandidate, SelectionContext,
};

use crate::protocol::{
    DashboardDto, DecisionRecordDto, DecisionTimelineDto, ExecutivePhaseStateDto, MetricsReportDto,
    WorldStateDto,
};

/// Which slices a `SourceDelta` touches. Used by tests and instrumentation;
/// the wire `DeltaFrame` carries the actual replacement values.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Slice {
    Dashboard,
    Timelines,
    Metrics,
    Ledger,
}

/// An additive change produced by a source tick. Only the slices listed in
/// `slices` are `Some`; consumers merge them over the last known snapshot.
#[derive(Debug, Clone, PartialEq)]
pub struct SourceDelta {
    pub slices: Vec<Slice>,
    pub dashboard: Option<DashboardDto>,
    pub timelines: Option<Vec<DecisionTimelineDto>>,
    pub metrics: Option<MetricsReportDto>,
    pub ledger: Option<Vec<DecisionRecordDto>>,
}

/// A complete point-in-time view of the read models.
#[derive(Debug, Clone, PartialEq)]
pub struct SourceSnapshot {
    pub dashboard: DashboardDto,
    pub timelines: Vec<DecisionTimelineDto>,
    pub metrics: MetricsReportDto,
    pub ledger: Vec<DecisionRecordDto>,
}

/// The provider seam between the bridge and the runtime read models.
pub trait RuntimeSource: Send + Sync {
    /// Stable identity of this bridge instance (shown in the console Runtime card).
    fn bridge_id(&self) -> &str;
    /// Where the data comes from (e.g. "mock", later "runtime").
    fn source_name(&self) -> &str;
    /// A full, current view of every read-model slice.
    fn snapshot(&self) -> SourceSnapshot;
    /// Advance the source by one tick and report what changed.
    /// Empty result = nothing changed this tick.
    fn tick(&mut self, now_ms: u64) -> Vec<SourceDelta>;
}

/// Monotonic clock source for the bridge, injectable in tests.
#[derive(Debug, Clone)]
pub struct Clock {
    next: u64,
}

impl Clock {
    pub fn new() -> Self {
        Self { next: 0 }
    }
    pub fn now_ms(&mut self) -> u64 {
        self.next += 1;
        self.next
    }
}

impl Default for Clock {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// DemoRuntimeSource
// ---------------------------------------------------------------------------

const PHASE_ORDER: [&str; 10] = [
    "Observe",
    "Attend",
    "Plan",
    "Simulate",
    "Execute",
    "Verify",
    "Reflect",
    "Consolidate",
    "Learn",
    "Idle",
];

const CAPABILITIES: [Capability; 7] = [
    Capability::ReadFile,
    Capability::WriteFile,
    Capability::ListDirectory,
    Capability::ExecuteCommand,
    Capability::SearchWeb,
    Capability::OpenBrowser,
    Capability::SearchMemory,
];

struct OpProfile {
    id: &'static str,
    name: &'static str,
    reliability: f64,
    availability: f64,
    avg_latency_ms: u64,
    avg_cost: f64,
    capable: &'static [Capability],
}

const OPERATORS: [OpProfile; 6] = [
    OpProfile { id: "filesystem", name: "Filesystem", reliability: 0.97, availability: 0.99, avg_latency_ms: 12, avg_cost: 0.01, capable: &[Capability::ReadFile, Capability::WriteFile, Capability::ListDirectory, Capability::ExecuteCommand] },
    OpProfile { id: "shell", name: "Command Shell", reliability: 0.93, availability: 0.96, avg_latency_ms: 42, avg_cost: 0.03, capable: &[Capability::ExecuteCommand, Capability::WriteFile, Capability::ListDirectory] },
    OpProfile { id: "code-reviewer", name: "Code Reviewer", reliability: 0.91, availability: 0.94, avg_latency_ms: 96, avg_cost: 0.05, capable: &[Capability::ReadFile, Capability::WriteFile] },
    OpProfile { id: "web-search", name: "Web Search", reliability: 0.87, availability: 0.92, avg_latency_ms: 140, avg_cost: 0.08, capable: &[Capability::SearchWeb, Capability::SearchMemory] },
    OpProfile { id: "browser", name: "Browser", reliability: 0.84, availability: 0.90, avg_latency_ms: 210, avg_cost: 0.12, capable: &[Capability::OpenBrowser, Capability::SearchWeb] },
    OpProfile { id: "memory-search", name: "Memory Search", reliability: 0.95, availability: 0.98, avg_latency_ms: 8, avg_cost: 0.02, capable: &[Capability::SearchMemory, Capability::ListDirectory] },
];

/// Deterministic LCG (no external RNG dependency), mirroring VP-004 and the
/// frontend mock: same seed => same ledger.
pub struct Lcg(u64);

impl Lcg {
    pub fn new(seed: u64) -> Self {
        Self(seed)
    }
    pub fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0 >> 33
    }
    pub fn below(&mut self, max: u64) -> u64 {
        self.next() % max
    }
    pub fn chance(&mut self, p: f64) -> bool {
        (self.next() % 1000) as f64 / 1000.0 < p
    }
}

fn clamp(v: f64, lo: f64, hi: f64) -> f64 {
    v.max(lo).min(hi)
}

fn operators_for(capability: &Capability) -> Vec<&'static OpProfile> {
    OPERATORS
        .iter()
        .filter(|o| o.capable.contains(capability))
        .collect()
}

fn policy_bonus(capability: &Capability, op: &OpProfile) -> f64 {
    if matches!(capability, Capability::ReadFile) && op.id == "filesystem" {
        0.08
    } else {
        0.0
    }
}

fn make_record(seq: u64, rng: &mut Lcg) -> DecisionRecord {
    let cid = CorrelationId::new();
    let capability = CAPABILITIES[(seq % 7) as usize].clone();
    let ops = operators_for(&capability);
    debug_assert!(!ops.is_empty(), "every capability has at least one operator");

    let candidates: Vec<ScoredCandidate> = ops
        .iter()
        .map(|op| {
            let jitter = (rng.below(20) as f64 - 10.0) / 100.0;
            let reliability_score = clamp(op.reliability + jitter, 0.05, 0.99);
            let availability_score = clamp(op.availability + jitter / 2.0, 0.5, 1.0);
            let latency_penalty = clamp(1.0 - op.avg_latency_ms as f64 / 400.0, 0.0, 1.0);
            let cost_penalty = clamp(1.0 - op.avg_cost / 0.25, 0.0, 1.0);
            let profile_match_score = clamp(0.6 + jitter, 0.1, 1.0);
            let context_fit_score = clamp(0.7 + jitter / 2.0, 0.1, 1.0);
            let bonus = policy_bonus(&capability, op);
            let weighted_score = clamp(
                0.38 * reliability_score
                    + 0.18 * availability_score
                    + 0.14 * profile_match_score
                    + 0.12 * context_fit_score
                    + 0.09 * latency_penalty
                    + 0.09 * cost_penalty
                    + bonus,
                0.0,
                1.0,
            );
            ScoredCandidate {
                plugin_id: op.id.into(),
                plugin_name: op.name.into(),
                reliability_score,
                availability_score,
                latency_penalty,
                cost_penalty,
                policy_bonus: bonus,
                profile_match_score,
                context_fit_score,
                weighted_score,
            }
        })
        .collect();

    let mut ranked = candidates.clone();
    ranked.sort_by(|a, b| b.weighted_score.partial_cmp(&a.weighted_score).unwrap());
    let best = ranked.first().expect("non-empty candidates").clone();
    let fallback_chain: Vec<ExecutionPlan> = ranked
        .iter()
        .skip(1)
        .map(|c| {
            let op = OPERATORS.iter().find(|o| o.id == c.plugin_id).unwrap();
            plan_shell(&cid, &capability, op, c.weighted_score)
        })
        .collect();

    let selected_op = OPERATORS.iter().find(|o| o.id == best.plugin_id).unwrap();
    let latency = selected_op.avg_latency_ms;
    let cost = selected_op.avg_cost;
    let success_probability = best.reliability_score;
    let confidence = clamp(0.3 + best.weighted_score * 0.6, 0.05, 1.0);
    let risk = (1.0 - success_probability) * 0.5;

    let evidence = vec![
        EvaluatorSignal { source: EvaluatorKind::Health, score: selected_op.availability, confidence: 0.9, explanation: "operator healthy".into() },
        EvaluatorSignal { source: EvaluatorKind::Policy, score: best.policy_bonus, confidence: 0.8, explanation: "policy applied".into() },
        EvaluatorSignal { source: EvaluatorKind::Profile, score: best.profile_match_score, confidence: 0.7, explanation: "profile match".into() },
        EvaluatorSignal { source: EvaluatorKind::Context, score: best.context_fit_score, confidence: 0.7, explanation: "context fit".into() },
    ];

    let plan = ExecutionPlan {
        plan_id: cid.clone(),
        version: 1,
        capability: capability.clone(),
        operator_id: best.plugin_id.clone(),
        estimated_latency_ms: latency,
        estimated_cost: cost,
        estimated_success_probability: success_probability,
        estimated_risk: risk,
        confidence,
        fallback_chain,
        rationale: PlanningRationale {
            candidates,
            selected_operator_id: best.plugin_id.clone(),
            composite_score: best.weighted_score,
            explanation: format!(
                "Selected {} with weighted score {:.3}.",
                selected_op.name, best.weighted_score
            ),
            correlation_id: cid.clone(),
            evaluator_evidence: evidence,
        },
    };

    let success = rng.chance(0.88);
    let outcome_latency = (latency as f64 * (0.7 + rng.below(90) as f64 / 100.0) + rng.below(25) as f64)
        .round() as u64;
    let outcome_cost = (cost * (0.6 + rng.below(80) as f64 / 100.0) * 100.0).round() / 100.0;

    DecisionRecord {
        id: cid.clone(),
        timestamp_ms: 1_784_000_000_000 + seq * 4000 + rng.below(3000),
        capability: capability.clone(),
        context: SelectionContext {
            budget: 1.0,
            ..SelectionContext::default()
        },
        parameters: std::collections::HashMap::new(),
        plan,
        outcome: PlanOutcome {
            latency_ms: outcome_latency,
            cost: outcome_cost,
            success,
            error: if success { None } else { Some("timeout".into()) },
            completed_steps: if success { 1 } else { 0 },
            cancelled: false,
            timeout: !success,
            retries: 0,
        },
        reflection: None,
    }
}

fn plan_shell(cid: &CorrelationId, capability: &Capability, op: &OpProfile, score: f64) -> ExecutionPlan {
    ExecutionPlan {
        plan_id: cid.clone(),
        version: 1,
        capability: capability.clone(),
        operator_id: op.id.into(),
        estimated_latency_ms: op.avg_latency_ms,
        estimated_cost: op.avg_cost,
        estimated_success_probability: op.reliability,
        estimated_risk: (1.0 - op.reliability) * 0.5,
        confidence: clamp(0.3 + score * 0.6, 0.05, 1.0),
        fallback_chain: Vec::new(),
        rationale: PlanningRationale {
            candidates: Vec::new(),
            selected_operator_id: op.id.into(),
            composite_score: score,
            explanation: String::new(),
            correlation_id: cid.clone(),
            evaluator_evidence: Vec::new(),
        },
    }
}

/// A synthetic FRIDAY runtime for end-to-end testing of the bridge.
///
/// Generates a deterministic ledger and derives the read models through the
/// frozen engines, exactly as a real runtime bridge would. Each tick advances
/// the ten-phase executive by one step; when a full cycle completes, a new
/// decision is appended to the ledger and the derived models are rebuilt.
pub struct DemoRuntimeSource {
    bridge_id: String,
    source_name: String,
    ledger: DecisionLedger,
    timelines: Vec<friday_experience::DecisionTimeline>,
    metrics: MetricsReport,
    phase_index: usize,
    cycles_completed: u64,
    rng: Lcg,
}

impl DemoRuntimeSource {
    /// Seed the demo runtime with `count` synthetic decisions.
    pub fn new(seed: u64, count: u64) -> Self {
        let mut rng = Lcg::new(seed);
        let mut ledger = DecisionLedger::new();
        for seq in 0..count {
            ledger.append(make_record(seq, &mut rng));
        }
        let mut source = Self {
            bridge_id: "friday-bridge-demo".into(),
            source_name: "mock".into(),
            ledger,
            timelines: Vec::new(),
            metrics: MetricsReport {
                decisions: 0,
                prediction: PredictionAccuracy::default(),
                calibration: CalibrationMetrics::default(),
                fallback: FallbackMetrics::default(),
                reputation: ReputationMetrics::default(),
                experience: ExperienceMetrics::default(),
            },
            phase_index: 0,
            cycles_completed: 0,
            rng,
        };
        source.regenerate();
        source
    }

    /// Rebuild the derived read models from the ledger (frozen engines only).
    fn regenerate(&mut self) {
        let mut experience = ExperienceStore::new();
        let mut reputation = ReputationStore::new();
        let calibration = CalibrationStore::new();
        ExperienceEngine::new().update(&self.ledger, &mut experience);
        ReputationEngine::update(&experience, &mut reputation);
        let builder = TimelineBuilder::new(&self.ledger, &experience, &reputation, &calibration);
        self.timelines = builder.build_all();
        self.metrics = MetricsEngine::summarize(&self.timelines);
    }

    fn dashboard(&self) -> DashboardDto {
        let phase = PHASE_ORDER[self.phase_index];
        let executive: Vec<ExecutivePhaseStateDto> = PHASE_ORDER
            .iter()
            .enumerate()
            .map(|(i, name)| ExecutivePhaseStateDto {
                name: name.to_string(),
                status: if i < self.phase_index {
                    "done".into()
                } else if i == self.phase_index {
                    "running".into()
                } else {
                    "waiting".into()
                },
            })
            .collect();

        let policies = self
            .timelines
            .iter()
            .flat_map(|t| t.explanation.policy_factors.iter().map(|_| 1))
            .count() as u64;
        let confidence = if self.timelines.is_empty() {
            0.0
        } else {
            self.timelines
                .iter()
                .map(|t| t.prediction.confidence)
                .sum::<f64>()
                / self.timelines.len() as f64
        };

        let recent = self
            .timelines
            .iter()
            .rev()
            .take(6)
            .map(DecisionTimelineDto::from)
            .collect();

        DashboardDto {
            mission: "Mission K-731 · Plan and execute the next objective safely".into(),
            goal_progress: 0.1 + (self.cycles_completed % 10) as f64 * 0.09,
            phase: match phase {
                "Observe" => "OBSERVING",
                "Attend" => "ATTENDING",
                "Plan" => "PLANNING",
                "Simulate" => "SIMULATING",
                "Execute" => "EXECUTING",
                "Verify" => "VERIFYING",
                "Reflect" => "REFLECTING",
                "Consolidate" => "CONSOLIDATING",
                "Learn" => "LEARNING",
                _ => "IDLE",
            }
            .into(),
            executive,
            world: WorldStateDto {
                files: 182,
                processes: 8,
                capabilities: CAPABILITIES.len() as u64,
                policies,
                episodes: self.timelines.len() as u64,
                confidence,
            },
            recent,
        }
    }

    fn ledger_dto(&self) -> Vec<DecisionRecordDto> {
        self.timelines
            .iter()
            .map(|t| DecisionRecordDto::from(&t.record))
            .collect()
    }

    fn timelines_dto(&self) -> Vec<DecisionTimelineDto> {
        self.timelines.iter().map(DecisionTimelineDto::from).collect()
    }
}

impl RuntimeSource for DemoRuntimeSource {
    fn bridge_id(&self) -> &str {
        &self.bridge_id
    }

    fn source_name(&self) -> &str {
        &self.source_name
    }

    fn snapshot(&self) -> SourceSnapshot {
        SourceSnapshot {
            dashboard: self.dashboard(),
            timelines: self.timelines_dto(),
            metrics: MetricsReportDto::from(&self.metrics),
            ledger: self.ledger_dto(),
        }
    }

    fn tick(&mut self, _now_ms: u64) -> Vec<SourceDelta> {
        self.phase_index = (self.phase_index + 1) % PHASE_ORDER.len();
        let new_record = self.phase_index == 0;
        if new_record {
            self.cycles_completed += 1;
            let record = make_record(self.timelines.len() as u64, &mut self.rng);
            self.ledger.append(record);
            self.regenerate();
        }

        let slices = vec![Slice::Dashboard];
        let mut delta = SourceDelta {
            slices,
            dashboard: Some(self.dashboard()),
            timelines: None,
            metrics: None,
            ledger: None,
        };
        if new_record {
            delta.slices = vec![
                Slice::Dashboard,
                Slice::Timelines,
                Slice::Metrics,
                Slice::Ledger,
            ];
            delta.timelines = Some(self.timelines_dto());
            delta.metrics = Some(MetricsReportDto::from(&self.metrics));
            delta.ledger = Some(self.ledger_dto());
        }
        vec![delta]
    }
}
