//! VP-004 — Scaling benchmark.
//!
//! Generates synthetic ledgers of increasing size and measures the read-model
//! operations: ledger append, Experience update, Reputation update, Timeline
//! build, Metrics generation, and Replay. Writes per-size JSON results to
//! `benchmarks/VP-004/results/` and prints a console summary.
//!
//! Run from the workspace root:
//!   cargo run -p friday-experience --example vp004
//!
//! Protocol: docs/VALIDATION.md (VP-004).

use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::time::Instant;

use friday_experience::{
    CalibrationStore, ExperienceEngine, ExperienceStore, MetricsEngine, ReplayEngine,
    ReputationEngine, ReputationStore, TimelineBuilder,
};
use friday_plugin_runtime::{
    Capability, CorrelationId, DecisionLedger, DecisionRecord, EvaluatorKind, EvaluatorSignal,
    ExecutionPlan, PlanOutcome, PlanningRationale, ScoredCandidate, SelectionContext,
};

const SIZES: [usize; 5] = [10, 100, 1_000, 10_000, 100_000];
const REPLAY_SAMPLE: usize = 100;
const OPERATORS: [&str; 3] = ["op_fs_a", "op_fs_b", "op_db"];
const CAPABILITIES: [Capability; 4] = [
    Capability::ReadFile,
    Capability::WriteFile,
    Capability::ListDirectory,
    Capability::ExecuteCommand,
];

/// Tiny deterministic RNG (LCG) — no dependencies.
struct Lcg(u64);

impl Lcg {
    fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0 >> 33
    }
}

fn make_record(seq: usize, rng: &mut Lcg) -> DecisionRecord {
    let cid = CorrelationId::new();
    let operator = OPERATORS[seq % OPERATORS.len()];
    let capability = CAPABILITIES[(seq / 3) % CAPABILITIES.len()].clone();
    let success = !rng.next().is_multiple_of(5);
    let latency = 5 + (rng.next() % 40);
    let cost = 0.05 + (rng.next() % 10) as f64 / 50.0;

    DecisionRecord {
        id: cid.clone(),
        timestamp_ms: 1_000_000 + seq as u64,
        capability: capability.clone(),
        context: SelectionContext::default(),
        parameters: HashMap::new(),
        plan: ExecutionPlan {
            plan_id: cid.clone(),
            version: 1,
            capability: capability.clone(),
            operator_id: operator.into(),
            estimated_latency_ms: latency,
            estimated_cost: cost,
            estimated_success_probability: 0.9,
            estimated_risk: 0.1,
            confidence: 0.8,
            fallback_chain: Vec::new(),
            rationale: PlanningRationale {
                candidates: vec![ScoredCandidate {
                    plugin_id: operator.into(),
                    plugin_name: String::new(),
                    reliability_score: 0.8,
                    availability_score: 0.9,
                    latency_penalty: latency as f64 / 100.0,
                    cost_penalty: cost,
                    policy_bonus: 0.0,
                    profile_match_score: 0.8,
                    context_fit_score: 0.8,
                    weighted_score: 0.8,
                }],
                selected_operator_id: operator.into(),
                composite_score: 0.8,
                explanation: "synthetic".into(),
                correlation_id: cid.clone(),
                evaluator_evidence: vec![EvaluatorSignal {
                    source: EvaluatorKind::Experience,
                    score: 0.8,
                    confidence: 0.7,
                    explanation: "synthetic".into(),
                }],
            },
        },
        outcome: PlanOutcome {
            latency_ms: latency,
            cost,
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

struct Measurements {
    n: usize,
    ledger_append_us: u128,
    experience_update_us: u128,
    reputation_update_us: u128,
    timeline_build_us: u128,
    metrics_us: u128,
    replay_sample: usize,
    replay_total_us: u128,
    record_bytes: usize,
}

fn run_size(n: usize, rng: &mut Lcg) -> Measurements {
    let mut ledger = DecisionLedger::new();

    let t0 = Instant::now();
    for seq in 0..n {
        ledger.append(make_record(seq, rng));
    }
    let ledger_append_us = t0.elapsed().as_micros();

    let mut experience = ExperienceStore::new();
    let mut reputation = ReputationStore::new();
    let calibration = CalibrationStore::new();

    let t0 = Instant::now();
    ExperienceEngine::new().update(&ledger, &mut experience);
    let experience_update_us = t0.elapsed().as_micros();

    let t0 = Instant::now();
    ReputationEngine::update(&experience, &mut reputation);
    let reputation_update_us = t0.elapsed().as_micros();

    let builder = TimelineBuilder::new(&ledger, &experience, &reputation, &calibration);
    let t0 = Instant::now();
    let timelines = builder.build_all();
    let timeline_build_us = t0.elapsed().as_micros();

    let t0 = Instant::now();
    let _report = MetricsEngine::summarize(&timelines);
    let metrics_us = t0.elapsed().as_micros();

    // Sample a deterministic subset for replay so the per-timeline cost is
    // meaningful at every scale.
    let step = (timelines.len() / REPLAY_SAMPLE).max(1);
    let sample = timelines.iter().step_by(step).take(REPLAY_SAMPLE);
    let mut replay_sample = 0;
    let t0 = Instant::now();
    for tl in sample {
        let _ = ReplayEngine::replay(tl, &experience, &reputation, &calibration);
        replay_sample += 1;
    }
    let replay_total_us = t0.elapsed().as_micros();

    Measurements {
        n,
        ledger_append_us,
        experience_update_us,
        reputation_update_us,
        timeline_build_us,
        metrics_us,
        replay_sample,
        replay_total_us,
        record_bytes: std::mem::size_of::<DecisionRecord>(),
    }
}

fn results_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../benchmarks/VP-004/results")
}

fn json_of(m: &Measurements) -> String {
    let per_append = m.ledger_append_us as f64 / m.n as f64;
    let per_replay = if m.replay_sample > 0 {
        m.replay_total_us as f64 / m.replay_sample as f64
    } else {
        0.0
    };
    format!(
        "{{\n  \"n\": {},\n  \"ledger_append_us\": {},\n  \"ledger_append_per_record_us\": {:.3},\n  \
         \"experience_update_us\": {},\n  \"reputation_update_us\": {},\n  \"timeline_build_us\": {},\n  \
         \"metrics_us\": {},\n  \"replay_sample_count\": {},\n  \"replay_total_us\": {},\n  \
         \"replay_per_timeline_us\": {:.3},\n  \"estimated_record_bytes\": {},\n  \
         \"estimated_ledger_bytes\": {}\n}}",
        m.n,
        m.ledger_append_us,
        per_append,
        m.experience_update_us,
        m.reputation_update_us,
        m.timeline_build_us,
        m.metrics_us,
        m.replay_sample,
        m.replay_total_us,
        per_replay,
        m.record_bytes,
        m.record_bytes * m.n,
    )
}

fn main() {
    let mut rng = Lcg(0x5EED);
    let dir = results_dir();
    fs::create_dir_all(&dir).expect("create results dir");

    println!(
        "{:>8} {:>10} {:>10} {:>10} {:>10} {:>10} {:>12}",
        "records", "append", "experience", "reputation", "timeline", "metrics", "replay/tl"
    );

    let mut all = Vec::new();
    for &n in &SIZES {
        let m = run_size(n, &mut rng);
        let path = dir.join(format!("{}.json", n));
        fs::write(&path, json_of(&m)).expect("write result json");
        all.push(m);
        let per_replay = if all.last().unwrap().replay_sample > 0 {
            all.last().unwrap().replay_total_us as f64 / all.last().unwrap().replay_sample as f64
        } else {
            0.0
        };
        println!(
            "{:>8} {:>10} {:>10} {:>10} {:>10} {:>10} {:>12.3}",
            n,
            all.last().unwrap().ledger_append_us,
            all.last().unwrap().experience_update_us,
            all.last().unwrap().reputation_update_us,
            all.last().unwrap().timeline_build_us,
            all.last().unwrap().metrics_us,
            per_replay,
        );
    }

    // Linearity ratios: per-record cost at the largest size vs the smallest.
    let small = &all[0];
    let large = &all[all.len() - 1];
    println!();
    println!("Scaling check (large/small per-record costs, ~1.0 means linear):");
    println!(
        "  append:    {:.3}",
        (large.ledger_append_us as f64 / large.n as f64)
            / (small.ledger_append_us as f64 / small.n as f64)
    );
    println!(
        "  timeline:  {:.3}",
        (large.timeline_build_us as f64 / large.n as f64)
            / (small.timeline_build_us as f64 / small.n as f64)
    );
    println!(
        "  metrics:   {:.3}",
        (large.metrics_us as f64 / large.n as f64) / (small.metrics_us as f64 / small.n as f64)
    );
    println!(
        "  replay/tl: {:.3}",
        (large.replay_total_us as f64 / large.replay_sample as f64)
            / (small.replay_total_us as f64 / small.replay_sample as f64)
    );
    println!();
    println!("Results written to {}", dir.display());
}
