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
//! feeds it through the incremental `TimelineStore` (ADR-0005 mechanism
//! C4+C6) — `apply_record` + `ReputationEngine::update_operator` +
//! `build_timeline` — the exact pipeline a real runtime bridge would run
//! over a real ledger. One point-in-time snapshot per decision, no O(history)
//! rebuild.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use friday_actions::{AgentKernel, GitAdapter, WebAgentAdapter};
use friday_agent_runtime::{AgentDirector, CognitiveTraceContext, GoalId, InMemoryTraceCollector, SessionId, SpanId, TraceCollector, TraceId};
use friday_cognitive::{
    ActionExecution, ActionVerifier, EngineId, GlobalWorkspace, TickAction, Topic,
};
use friday_experience::{CalibrationStore, TimelineStore};
use friday_goal_execution::{
    AdapterTool, Capability as GoalCapability, CausalCondition, CausalEdge, CausalEffect,
    CognitiveBlackboard, DesiredState, Executive, IntentDecision, MockTool, WorldModel, WorldState,
};
use friday_investigation::InvestigationGraph;
use friday_plugin_runtime::{
    Capability, CorrelationId, DecisionRecord, EvaluatorKind, EvaluatorSignal, ExecutionPlan,
    PlanOutcome, PlanningRationale, ScoredCandidate, SelectionContext,
};

use crate::protocol::{
    AutonomyTraceDto, DashboardDto, DecisionRecordDto, DecisionTimelineDto, EvidenceActionDto,
    EvidenceTrailDto, ExecutivePhaseStateDto, InvestigationGraphDto, MetricsReportDto,
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
///
/// Async because the read models include the investigation graph, whose
/// strategy seam is async (an LLM-backed strategy is a drop-in for the
/// deterministic heuristic). A deterministic source returns immediately when
/// awaited.
#[async_trait]
pub trait RuntimeSource: Send + Sync {
    /// Stable identity of this bridge instance (shown in the console Runtime card).
    fn bridge_id(&self) -> &str;
    /// Where the data comes from (e.g. "mock", later "runtime").
    fn source_name(&self) -> &str;
    /// A full, current view of every read-model slice.
    async fn snapshot(&self) -> SourceSnapshot;
    /// Advance the source by one tick and report what changed.
    /// Empty result = nothing changed this tick.
    async fn tick(&mut self, now_ms: u64) -> Vec<SourceDelta>;
}

/// Emit a structured timing line for the Validation Harness analyzer
/// (`validation/analyze.mjs` parses `bridge.log` for `TIMING` lines). Written
/// to stderr, which the runner captures into `bridge.log`. Deliberately NOT on
/// the wire protocol — the frozen frame format stays byte-identical. Format:
/// `TIMING <name> <milliseconds> <epoch-ms>`.
fn emit_timing(name: &str, elapsed: std::time::Duration) {
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    eprintln!(
        "TIMING {} {:.3} {}",
        name,
        elapsed.as_secs_f64() * 1000.0,
        ts
    );
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

const PHASE_ORDER: [&str; 11] = [
    "Observe",
    "Attend",
    "Investigate",
    "Plan",
    "Simulate",
    "Execute",
    "Verify",
    "Reflect",
    "Consolidate",
    "Learn",
    "Idle",
];

/// Input goal queue fed to the real goal-execution `Executive`, one per
/// executive cycle. Each exercises a different graph shape: web+docs evidence
/// branches, git+codeintel+filesystem, a memory recall, and a self-contained
/// goal that needs no investigation (`plan → execute` only). Mirrors the
/// frontend mock. In production this is the daemon's goal intake — the runtime
/// is the same `Executive`, only the goal source differs.
pub const DEMO_GOALS: [&str; 4] = [
    "Search the web for the latest Rust async docs",
    "Check the git status of the workspace repository and show changed files",
    "Do you remember what I asked about yesterday?",
    "Open notepad and start writing a note",
];

/// Accessor for tests and the console wiring.
pub fn demo_goals() -> &'static [&'static str] {
    &DEMO_GOALS
}

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
    OpProfile {
        id: "filesystem",
        name: "Filesystem",
        reliability: 0.97,
        availability: 0.99,
        avg_latency_ms: 12,
        avg_cost: 0.01,
        capable: &[
            Capability::ReadFile,
            Capability::WriteFile,
            Capability::ListDirectory,
            Capability::ExecuteCommand,
        ],
    },
    OpProfile {
        id: "shell",
        name: "Command Shell",
        reliability: 0.93,
        availability: 0.96,
        avg_latency_ms: 42,
        avg_cost: 0.03,
        capable: &[
            Capability::ExecuteCommand,
            Capability::WriteFile,
            Capability::ListDirectory,
        ],
    },
    OpProfile {
        id: "code-reviewer",
        name: "Code Reviewer",
        reliability: 0.91,
        availability: 0.94,
        avg_latency_ms: 96,
        avg_cost: 0.05,
        capable: &[Capability::ReadFile, Capability::WriteFile],
    },
    OpProfile {
        id: "web-search",
        name: "Web Search",
        reliability: 0.87,
        availability: 0.92,
        avg_latency_ms: 140,
        avg_cost: 0.08,
        capable: &[Capability::SearchWeb, Capability::SearchMemory],
    },
    OpProfile {
        id: "browser",
        name: "Browser",
        reliability: 0.84,
        availability: 0.90,
        avg_latency_ms: 210,
        avg_cost: 0.12,
        capable: &[Capability::OpenBrowser, Capability::SearchWeb],
    },
    OpProfile {
        id: "memory-search",
        name: "Memory Search",
        reliability: 0.95,
        availability: 0.98,
        avg_latency_ms: 8,
        avg_cost: 0.02,
        capable: &[Capability::SearchMemory, Capability::ListDirectory],
    },
];

/// Deterministic LCG (no external RNG dependency), mirroring VP-004 and the
/// frontend mock: same seed => same ledger.
pub struct Lcg(u64);

impl Lcg {
    pub fn new(seed: u64) -> Self {
        Self(seed)
    }
    pub fn next_u64(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0 >> 33
    }
    pub fn below(&mut self, max: u64) -> u64 {
        self.next_u64() % max
    }
    pub fn chance(&mut self, p: f64) -> bool {
        (self.next_u64() % 1000) as f64 / 1000.0 < p
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
    debug_assert!(
        !ops.is_empty(),
        "every capability has at least one operator"
    );

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
        EvaluatorSignal {
            source: EvaluatorKind::Health,
            score: selected_op.availability,
            confidence: 0.9,
            explanation: "operator healthy".into(),
        },
        EvaluatorSignal {
            source: EvaluatorKind::Policy,
            score: best.policy_bonus,
            confidence: 0.8,
            explanation: "policy applied".into(),
        },
        EvaluatorSignal {
            source: EvaluatorKind::Profile,
            score: best.profile_match_score,
            confidence: 0.7,
            explanation: "profile match".into(),
        },
        EvaluatorSignal {
            source: EvaluatorKind::Context,
            score: best.context_fit_score,
            confidence: 0.7,
            explanation: "context fit".into(),
        },
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
    let outcome_latency = (latency as f64 * (0.7 + rng.below(90) as f64 / 100.0)
        + rng.below(25) as f64)
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
            error: if success {
                None
            } else {
                Some("timeout".into())
            },
            completed_steps: if success { 1 } else { 0 },
            cancelled: false,
            timeout: !success,
            retries: 0,
        },
        reflection: None,
    }
}

fn plan_shell(
    cid: &CorrelationId,
    capability: &Capability,
    op: &OpProfile,
    score: f64,
) -> ExecutionPlan {
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

/// A FRIDAY runtime stand-in for the bridge: a synthetic decision ledger (the
/// read models a daemon would surface) plus a **real goal-execution
/// `Executive`** that genuinely processes each goal — investigate → plan →
/// execute → verify → reflect → consolidate → learn — through real adapters
/// where they exist (the External channel is backed by the actual
/// `WebAgentAdapter`, profile/site from `FRIDAY_BROWSER_PROFILE` /
/// `FRIDAY_WEBAGENT_SITE`) and mocks for the remaining channels.
///
/// The dashboard's live `InvestigationGraph` is read back from the Executive's
/// own workspace after each cycle: the exact plan that gated planning, never a
/// parallel derivation from the goal string. The ledger feeds the incremental
/// `TimelineStore` (ADR-0005 mechanism C4+C6) exactly as a real runtime bridge
/// would; each tick advances the eleven-phase executive by one step, and on
/// cycle completion the next goal flows through the runtime and a new decision
/// is appended — no `regenerate()` rebuild of the history.
pub struct DemoRuntimeSource {
    bridge_id: String,
    source_name: String,
    store: TimelineStore,
    phase_index: usize,
    cycles_completed: u64,
    rng: Lcg,
    /// The real goal-execution Executive (plus its lazy-start flag). The
    /// source feeds each input goal here and reads the investigation graph
    /// back from the blackboard afterwards.
    executive: tokio::sync::Mutex<ExecutiveState>,
    /// The Executive's cognitive blackboard — carries the last investigation
    /// snapshot (goal + plan) this source renders into the live graph.
    workspace: Arc<CognitiveBlackboard>,
    /// Which input goal the Executive is processing (wraps at DEMO_GOALS.len()).
    goal_index: usize,
}

/// The owned Executive and its lazy-init flag: the first snapshot runs goal 0
/// through a full cycle so the initial frame already carries a real graph.
struct ExecutiveState {
    executive: Executive,
    started: bool,
}

impl DemoRuntimeSource {
    /// Seed the demo runtime with `count` synthetic decisions and a real
    /// goal-execution Executive wired with mock tools for every evidence
    /// channel (web, docs, git, codeintel, memory, filesystem, terminal).
    pub fn new(seed: u64, count: u64) -> Self {
        Self::with_retention(seed, count, usize::MAX)
    }

    /// Same as [`DemoRuntimeSource::new`], but bounds the bridge's retained
    /// read model to the `retention` most recent records (C2 windowed
    /// emission, MECH-0002). Older records are evicted from the bridge's
    /// in-memory store, so the emitted `timelines`/`ledger` lists are
    /// **recent/windowed**, not full history. The durable Experience-layer
    /// history (planner redb `TimelineStore`) is a separate store and is
    /// never touched; the dashboard `episodes` count and metrics remain
    /// cumulative over every appended record.
    pub fn with_retention(seed: u64, count: u64, retention: usize) -> Self {
        let mut rng = Lcg::new(seed);
        let mut store = TimelineStore::new(CalibrationStore::new()).with_retention(retention);
        for seq in 0..count {
            store.append(Arc::new(make_record(seq, &mut rng)));
        }

        let workspace = CognitiveBlackboard::new();
        let mut executive = Executive::new();
        // The evidence actions carry the real skill capability ids (web.search,
        // git.status, …) as Capability::Custom, and the goal's own action uses
        // the legacy infer_capability — register a tool per channel so the
        // demo goals execute end-to-end through the real runtime.
        executive.register_tool(Box::new(
            MockTool::success("web", "search results").with_capabilities(vec![
                GoalCapability::SearchWeb,
                GoalCapability::Custom("web.search".into()),
                GoalCapability::Custom("web.fetch".into()),
            ]),
        ));
        executive.register_tool(Box::new(
            MockTool::success("docs", "docs content").with_capabilities(vec![
                GoalCapability::ReadFile,
                GoalCapability::Custom("docs.search".into()),
                GoalCapability::Custom("docs.read".into()),
            ]),
        ));
        executive.register_tool(Box::new(
            MockTool::success("git", "clean working tree").with_capabilities(vec![
                GoalCapability::ReadFile,
                GoalCapability::Custom("git.status".into()),
                GoalCapability::Custom("git.log".into()),
            ]),
        ));
        executive.register_tool(Box::new(
            MockTool::success("codeintel", "symbol map").with_capabilities(vec![
                GoalCapability::ReadFile,
                GoalCapability::Custom("codeintel.understand".into()),
            ]),
        ));
        executive.register_tool(Box::new(
            MockTool::success("memory", "prior recall").with_capabilities(vec![
                GoalCapability::SearchMemory,
                GoalCapability::Custom("memory.recall".into()),
            ]),
        ));
        executive.register_tool(Box::new(
            MockTool::success("filesystem", "listed").with_capabilities(vec![
                GoalCapability::ListDirectory,
                GoalCapability::ReadFile,
                GoalCapability::Custom("filesystem.list_directory".into()),
                GoalCapability::Custom("filesystem.read_file".into()),
            ]),
        ));
        executive.register_tool(Box::new(
            MockTool::success("terminal", "ok")
                .with_capabilities(vec![GoalCapability::ExecuteCommand]),
        ));
        // The External channel routes through the WebAgent layer
        // (`webagent.ask/send/read` — logged-in sites via the Browser Organ).
        // The REAL Browser Organ adapter backs it (same `AdapterTool` seam as
        // `register_evidence_adapters`): `FRIDAY_BROWSER_PROFILE` selects the
        // dedicated logged-in profile (default `~/.friday/browser/`) and
        // `FRIDAY_WEBAGENT_SITE` the default site (else `https://chatgpt.com`).
        // Research goals that need another intelligence ("ask ChatGPT…") execute
        // the logged-in site through the browser — no API keys. Registered but
        // inert in the demo: none of the demo goals trigger the External channel,
        // so no browser launches during bridge runs/tests. If a demo goal ever
        // mentions "ask chatgpt/grok", a real Chrome would open here.
        executive.register_tool(Box::new(AdapterTool::for_evidence(Arc::new(
            WebAgentAdapter::new(),
        ))));
        // "FRIDAY chooses the expert": `reasoning.consult` asks the
        // best-suited external intelligence by capability profile (browser /
        // API / local transports are interchangeable). Mock in the demo so a
        // consult evidence action resolves end-to-end; a real deployment
        // registers a ReasoningAdapter with its provider router.
        executive.register_tool(Box::new(
            MockTool::success("reasoning", "expert reply")
                .with_capabilities(vec![GoalCapability::Custom("reasoning.consult".into())]),
        ));
        // Advertise the same surface to the evaluator/simulator so evidence
        // plans are judged feasible and actually execute.
        workspace.set_world_state(
            WorldState::new()
                .with_tool(GoalCapability::SearchWeb)
                .with_tool(GoalCapability::SearchMemory)
                .with_tool(GoalCapability::ReadFile)
                .with_tool(GoalCapability::ListDirectory)
                .with_tool(GoalCapability::ExecuteCommand)
                .with_tool(GoalCapability::Custom("web.search".into()))
                .with_tool(GoalCapability::Custom("docs.search".into()))
                .with_tool(GoalCapability::Custom("git.status".into()))
                .with_tool(GoalCapability::Custom("codeintel.understand".into()))
                .with_tool(GoalCapability::Custom("memory.recall".into()))
                .with_tool(GoalCapability::Custom("filesystem.list_directory".into()))
                .with_tool(GoalCapability::Custom("webagent.ask".into()))
                .with_tool(GoalCapability::Custom("webagent.send".into()))
                .with_tool(GoalCapability::Custom("webagent.read".into()))
                .with_tool(GoalCapability::Custom("reasoning.consult".into())),
        );

        Self {
            bridge_id: "friday-bridge-demo".into(),
            source_name: "mock".into(),
            store,
            phase_index: 0,
            cycles_completed: 0,
            rng,
            executive: tokio::sync::Mutex::new(ExecutiveState {
                executive,
                started: false,
            }),
            workspace,
            goal_index: 0,
        }
    }

    async fn dashboard(&self) -> DashboardDto {
        let t = Instant::now();
        let dto = self.dashboard_impl();
        emit_timing("dashboard_generation", t.elapsed());
        dto
    }

    fn dashboard_impl(&self) -> DashboardDto {
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

        let policies = self.store.policy_count();
        let confidence = self.store.confidence_mean();

        let recent = self
            .store
            .timelines()
            .iter()
            .rev()
            .take(6)
            .map(DecisionTimelineDto::from)
            .collect();

        // The live graph is the Executive's own evidence-gate outcome: what it
        // actually decided this goal needed before planning. The first snapshot
        // runs goal 0 through the runtime, so this is always populated.
        let graph = self.workspace.get_investigation().map(|inv| {
            let g = InvestigationGraph::new(&inv.goal, &inv.plan);
            InvestigationGraphDto::from(&g)
        });

        // The evidence lifecycle is the Execute-phase segment of the cognitive
        // trace, read back from the Executive's own workspace: every evidence
        // action with its recorded Prepared → Observed → Verified | Failed
        // state. None for direct-plan goals that gather no evidence (the
        // workspace slot is cleared at the start of every Execute phase).
        //
        // Note on live feel: the demo runs each executive cycle atomically, so
        // this carries the cycle's *final* recorded states (the mock console
        // animates prepared → observed → verified by phase instead — both are
        // truthful to their source). A real runtime that streams the Executive
        // per-phase would emit intermediate states here too.
        let evidence = self
            .workspace
            .get_evidence_trail()
            .map(|trail| EvidenceTrailDto {
                goal: trail.goal.clone(),
                records: trail
                    .records
                    .iter()
                    .map(|r| EvidenceActionDto {
                        channel: r.channel.clone(),
                        capability: r.capability.clone(),
                        execution_state: r.execution_state.label().to_string(),
                        verified: r.verified,
                        tool_success: r.tool_success,
                        duration_ms: r.duration_ms,
                    })
                    .collect(),
            });

        DashboardDto {
            mission: "Mission K-731 · Plan and execute the next objective safely".into(),
            goal_progress: 0.1 + (self.cycles_completed % 10) as f64 * 0.09,
            phase: match phase {
                "Observe" => "OBSERVING",
                "Attend" => "ATTENDING",
                "Investigate" => "INVESTIGATING",
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
                episodes: self.store.total_appended(),
                confidence,
            },
            recent,
            graph,
            evidence,
            // DemoRuntimeSource intentionally does not fabricate autonomy
            // events; a deployed source supplies this from GlobalWorkspace.
            autonomy: None,
        }
    }

    /// Feed the current input goal into the real Executive and run one full
    /// cognitive cycle (investigate → plan → execute → verify → reflect →
    /// consolidate → learn). Afterwards the workspace carries the genuine
    /// investigation snapshot this source renders as the live graph.
    async fn run_cycle(&self, executive: &mut Executive) {
        // Keep focus deterministic: drop stale attention items (each cycle's
        // observe phase can add one when world confidence is low) so the fed
        // goal is always the next focus target.
        executive.attention_manager_mut().clear();
        executive.add_goal(DEMO_GOALS[self.goal_index], 0.9);
        let _ = executive.cycle(&self.workspace).await;
    }

    /// The retained ledger as DTOs. With a retention window set (MECH-0002
    /// C2) this is the **recent window**, not full history — older records
    /// are evicted from the bridge read model and remain only in the durable
    /// Experience-layer store.
    fn ledger_dto(&self) -> Vec<DecisionRecordDto> {
        self.store
            .timelines()
            .iter()
            .map(|t| DecisionRecordDto::from(t.record.as_ref()))
            .collect()
    }

    /// The retained decision timelines as DTOs. With a retention window set
    /// (MECH-0002 C2) this is the **recent window**, not full history.
    fn timelines_dto(&self) -> Vec<DecisionTimelineDto> {
        self.store
            .timelines()
            .iter()
            .map(DecisionTimelineDto::from)
            .collect()
    }
}

#[async_trait]
impl RuntimeSource for DemoRuntimeSource {
    fn bridge_id(&self) -> &str {
        &self.bridge_id
    }

    fn source_name(&self) -> &str {
        &self.source_name
    }

    async fn snapshot(&self) -> SourceSnapshot {
        // The first snapshot runs goal 0 through the real Executive so the
        // initial frame already carries a genuine investigation graph.
        {
            let mut st = self.executive.lock().await;
            if !st.started {
                self.run_cycle(&mut st.executive).await;
                st.started = true;
            }
        }
        SourceSnapshot {
            dashboard: self.dashboard().await,
            timelines: self.timelines_dto(),
            metrics: MetricsReportDto::from(&self.store.metrics_report()),
            ledger: self.ledger_dto(),
        }
    }

    async fn tick(&mut self, _now_ms: u64) -> Vec<SourceDelta> {
        self.phase_index = (self.phase_index + 1) % PHASE_ORDER.len();
        let new_record = self.phase_index == 0;
        if new_record {
            self.cycles_completed += 1;
            // A new executive cycle means a new goal: feed it to the real
            // Executive and run one full cognitive cycle, so the workspace
            // carries the genuine investigation graph for that goal.
            self.goal_index = (self.goal_index + 1) % DEMO_GOALS.len();
            {
                let mut st = self.executive.lock().await;
                self.run_cycle(&mut st.executive).await;
                st.started = true;
            }
            let record = make_record(self.store.total_appended() as u64, &mut self.rng);
            self.store.append(Arc::new(record));
        }

        let slices = vec![Slice::Dashboard];
        let mut delta = SourceDelta {
            slices,
            dashboard: Some(self.dashboard().await),
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
            delta.metrics = Some(MetricsReportDto::from(&self.store.metrics_report()));
            delta.ledger = Some(self.ledger_dto());
        }
        vec![delta]
    }
}

// ---------------------------------------------------------------------------
// LiveRuntimeSource
// ---------------------------------------------------------------------------

/// A small real-runtime source used by the daemon mode and acceptance tests.
/// It preserves the existing frozen read models from `DemoRuntimeSource`, but
/// obtains the autonomy trace from a real `AutonomyHost` and `GlobalWorkspace`.
/// The action is deliberately harmless: `git.status` against a caller-provided
/// repository, verified by the actual command output. The exact intent action
/// is delegated through an `AgentDirector` child that runs it on the real
/// Action Kernel; the child's structured observation is folded back into the
/// WorldModel.
pub struct LiveRuntimeSource {
    demo: DemoRuntimeSource,
    host: friday_goal_execution::AutonomyHost,
    repo: PathBuf,
    started: Mutex<bool>,
}

struct GitStatusVerifier;

#[async_trait]
impl ActionVerifier for GitStatusVerifier {
    async fn verify(
        &self,
        _workspace: &GlobalWorkspace,
        _action: &TickAction,
        execution: &ActionExecution,
    ) -> bool {
        execution.success
            && execution.output["stdout"]
                .as_str()
                .unwrap_or_default()
                .contains("live-autonomy-marker.txt")
    }
}

impl LiveRuntimeSource {
    pub async fn new(repo: impl Into<PathBuf>, journal_path: impl Into<PathBuf>) -> Self {
        let repo = repo.into();
        let kernel = Arc::new(AgentKernel::new().with_max_retries(0));
        kernel.register_adapter(Box::new(GitAdapter::new())).await;
        // The exact intent action is delegated through an Agent Director: a
        // bounded child whose driver runs the goal on the real kernel and
        // attaches its output as a structured observation.
        let child_driver = Arc::new(friday_goal_execution::KernelChildDriver::new(
            Arc::clone(&kernel),
            serde_json::json!({ "repo": repo.to_string_lossy() }),
            repo.clone(),
        ));
        let trace_collector = Arc::new(InMemoryTraceCollector::new());
        // One coherent cognitive episode for this live autonomy source. The
        // Director must attach under the upstream span rather than spawning a
        // separate trace root — this is the Phase 5C propagation seam. In the
        // full path the Planner supplies this context (a `TraceContext` child
        // of its `create_plan` span); here the live intent decision is the
        // upstream, so the director nests beneath an `intent` span.
        let live_trace_id = TraceId::default();
        let live_session = SessionId::default();
        let live_goal = GoalId::default();
        let intent_ctx =
            CognitiveTraceContext::for_goal(live_trace_id, live_session, live_goal);
        let intent_span = SpanId::default();
        trace_collector.record(intent_ctx.trace(intent_span, "intent", "live_autonomy"));
        let director_ctx = intent_ctx.child(intent_span);
        let director = Arc::new(
            AgentDirector::new(child_driver)
                .with_tracer(trace_collector)
                .with_trace_ctx(director_ctx),
        );
        let executor =
            friday_goal_execution::DirectorActionExecutor::new(director).with_cwd(repo.clone());
        let workspace = GlobalWorkspace::new();
        let mut initial_model =
            WorldModel::new(WorldState::new().with_working_directory(repo.to_string_lossy()));
        initial_model.causal_mut().add_edge(CausalEdge::new(
            "observe-live-marker",
            friday_goal_execution::Capability::Custom("git.status".into()),
            CausalCondition::Always,
            vec![CausalEffect::FileCreated {
                path: "live-autonomy-marker.txt".into(),
            }],
        ));
        let world_model = Arc::new(Mutex::new(initial_model));
        let intent_decision = IntentDecision::new(
            Arc::clone(&world_model),
            DesiredState::FileExists {
                path: "live-autonomy-marker.txt".into(),
            },
        );
        let host = friday_goal_execution::AutonomyHost::new(
            workspace,
            friday_cognitive::AutonomousTick::new(Arc::new(intent_decision)),
            Arc::new(executor),
            Arc::new(GitStatusVerifier),
            Arc::new(friday_cognitive::JsonActionJournal::new(journal_path)),
        )
        .with_world_model(world_model);
        Self {
            demo: DemoRuntimeSource::new(42, 140),
            host,
            repo,
            started: Mutex::new(false),
        }
    }

    async fn ensure_action(&self) {
        let should_start = {
            let mut started = self.started.lock().expect("live source lock poisoned");
            if *started {
                false
            } else {
                *started = true;
                true
            }
        };
        if should_start {
            // A completed journal is authoritative across process instances.
            // Rehydrate its trace instead of creating a new evidence UUID and
            // redispatching the same side effect.
            if self.host.recover().unwrap_or(false) {
                return;
            }
            self.host.workspace().publish_from(
                Topic::Screen,
                EngineId::Perception,
                serde_json::json!({
                    "change": "content",
                    "content_hash": "live-autonomy",
                    "repository": self.repo,
                }),
            );
            let _ = self.host.tick().await;
        }
    }

    fn attach_autonomy(&self, dashboard: &mut DashboardDto) {
        dashboard.autonomy = Some(AutonomyTraceDto::from(
            &self.host.workspace().autonomy_read_model(),
        ));
    }
}

#[async_trait]
impl RuntimeSource for LiveRuntimeSource {
    fn bridge_id(&self) -> &str {
        "friday-bridge-live"
    }

    fn source_name(&self) -> &str {
        "runtime"
    }

    async fn snapshot(&self) -> SourceSnapshot {
        self.ensure_action().await;
        let mut snapshot = self.demo.snapshot().await;
        self.attach_autonomy(&mut snapshot.dashboard);
        snapshot
    }

    async fn tick(&mut self, now_ms: u64) -> Vec<SourceDelta> {
        self.ensure_action().await;
        let mut deltas = self.demo.tick(now_ms).await;
        for delta in &mut deltas {
            if let Some(dashboard) = &mut delta.dashboard {
                self.attach_autonomy(dashboard);
            }
        }
        deltas
    }
}
