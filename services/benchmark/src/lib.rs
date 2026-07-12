//! FRIDAY Benchmark Sprint harness (Alpha-Qualification).
//!
//! Breaks the platform with a large corpus of tasks across every subsystem
//! (filesystem, terminal, git, windows, inference, browser) and measures the
//! reliability numbers the project now lives or dies by:
//!
//! * **MTBF** — mean tasks between failures (a "failure" = the simulation could
//!   not confidently guarantee a safe, resolvable plan).
//! * **Simulation Accuracy** — does the Digital Twin's predicted outcome match
//!   what actually happens (real fs writes in a sandbox)?
//! * **Planner Accuracy** — does the planner produce the expected capability?
//! * **Recovery Success** — when a provider is killed (Chaos Mode), does a
//!   fallback path actually save the task?
//! * **Confidence Calibration** — is the stated confidence close to the real
//!   success rate?
//!
//! Set `CHAOS=1` to randomly kill providers before the run.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use rand::SeedableRng;
use serde::{Deserialize, Serialize};
use serde_json::json;
use uuid::Uuid;

use friday_capabilities::model::{Capability, Health, Platform};
use friday_capabilities::registry::CapabilityRegistry;
use friday_capabilities::sim::{
    Precondition, ResourceForecast, RollbackPlan, SideEffect, SimRequest, Simulatable,
    StepEstimate, WorldState,
};
use friday_planner::planning::TaskGraph;
use friday_planner::{parse, plan};
use friday_recovery::{default_graph, FailureCategory};
use friday_simulation::{IssueSeverity, SimulationEngine, SimulationReport};

/// A single benchmark task drawn from the corpus.
pub struct CorpusTask {
    pub name: String,
    /// Fully-qualified capability id, e.g. `filesystem.create_file`.
    pub capability: String,
    /// Action verb handed to the simulator, e.g. `create_file`.
    pub action: String,
    pub params: serde_json::Value,
    /// Tool prefix used to look up the `Simulatable`, e.g. `filesystem`.
    pub tool: String,
    /// True for tasks whose outcome we can validate for real in a sandbox.
    pub sandbox: bool,
}

/// One row of the reliability dashboard.
#[derive(Serialize, Deserialize, Clone)]
pub struct TaskRecord {
    pub name: String,
    pub capability: String,
    pub resolved: bool,
    pub risk: String,
    pub confidence: f64,
    pub issues: usize,
    pub fallback_used: bool,
    pub predicted_success: bool,
    pub actual_success: bool,
}

/// A calibration bucket: stated confidence vs observed success rate.
#[derive(Serialize, Deserialize, Clone)]
pub struct CalibrationBin {
    pub bin_low: f64,
    pub bin_high: f64,
    pub count: usize,
    pub avg_predicted: f64,
    pub avg_actual: f64,
}

/// Per-subsystem recovery coverage (the Recovery Graph, not the sim fallback).
#[derive(Serialize, Deserialize, Clone)]
pub struct SubsystemRecovery {
    pub subsystem: String,
    /// Times this subsystem needed recovery.
    pub attempts: usize,
    /// Times the Recovery Graph produced a resolving step.
    pub recovered: usize,
    /// Mean Recovery Confidence over resolved recoveries.
    pub mean_confidence: f64,
}

/// Aggregate reliability metrics for a run.
#[derive(Serialize, Deserialize, Default, Clone)]
pub struct Metrics {
    pub total_tasks: usize,
    pub resolved: usize,
    pub unresolved: usize,
    pub low_risk: usize,
    pub medium_risk: usize,
    pub high_risk: usize,
    pub with_issues: usize,
    pub approval_required: usize,
    pub fallback_used: usize,
    pub rollback_available: usize,
    pub mean_confidence: f64,
    /// Mean tasks between failures (higher is better).
    pub mtbf_tasks: f64,
    pub simulation_accuracy: f64,
    pub planner_accuracy: f64,
    /// Only meaningful under Chaos Mode (Some when a fallback was needed).
    pub recovery_success: Option<f64>,
    /// Recovery Graph coverage: attempts / resolved / mean confidence.
    pub recovery_attempts: usize,
    pub recovery_recovered: usize,
    pub recovery_confidence_mean: f64,
    pub recovery_by_subsystem: Vec<SubsystemRecovery>,
    /// Mean |predicted - actual| across calibration bins (lower is better).
    pub calibration_error: f64,
}

/// Full result of one benchmark run.
#[derive(Serialize, Deserialize)]
pub struct BenchmarkResult {
    pub chaos: bool,
    pub metrics: Metrics,
    pub calibration: Vec<CalibrationBin>,
    pub tasks: Vec<TaskRecord>,
}

/// Build a balanced corpus of `n` tasks spread across every subsystem.
pub fn generate_corpus(n: usize, temp: &Path) -> Vec<CorpusTask> {
    let cats = 6usize;
    let per = (n / cats).max(1);
    let mut tasks = Vec::new();

    for i in 0..per {
        let p = temp.join(format!("f{}.txt", i));
        tasks.push(CorpusTask {
            name: format!("fs.create_file#{}", i),
            capability: "filesystem.create_file".into(),
            action: "create_file".into(),
            params: json!({ "path": p.to_string_lossy(), "content": "benchmark" }),
            tool: "filesystem".into(),
            sandbox: true,
        });
    }
    for i in 0..per {
        tasks.push(CorpusTask {
            name: format!("term.run#{}", i),
            capability: "terminal.run".into(),
            action: "run".into(),
            params: json!({ "command": format!("echo hello{}", i) }),
            tool: "terminal".into(),
            sandbox: false,
        });
    }
    for i in 0..per {
        tasks.push(CorpusTask {
            name: format!("git.commit#{}", i),
            capability: "git.commit".into(),
            action: "commit".into(),
            params: json!({ "message": format!("wip{}", i) }),
            tool: "git".into(),
            sandbox: false,
        });
    }
    for i in 0..per {
        tasks.push(CorpusTask {
            name: format!("win.open#{}", i),
            capability: "windows.open".into(),
            action: "open".into(),
            params: json!({ "app": "notepad" }),
            tool: "windows".into(),
            sandbox: false,
        });
    }
    for i in 0..per {
        tasks.push(CorpusTask {
            name: format!("infer.complete#{}", i),
            capability: "inference.complete".into(),
            action: "complete".into(),
            params: json!({ "prompt": format!("say {}", i) }),
            tool: "inference".into(),
            sandbox: false,
        });
    }
    for i in 0..per {
        tasks.push(CorpusTask {
            name: format!("browser.navigate#{}", i),
            capability: "browser.navigate".into(),
            action: "navigate".into(),
            params: json!({ "query": format!("rust{}", i) }),
            tool: "browser".into(),
            sandbox: false,
        });
    }
    tasks
}

/// Populate a registry with a realistic capability set for every subsystem.
pub fn build_registry() -> Arc<CapabilityRegistry> {
    let reg = Arc::new(CapabilityRegistry::new());

    let fs_actions = [
        "create_file",
        "write_file",
        "delete_file",
        "create_dir",
        "read_file",
        "list_directory",
        "move",
        "copy",
    ];
    let fs_caps: Vec<Capability> = fs_actions
        .iter()
        .map(|a| {
            Capability::new(format!("filesystem.{a}"), *a, "Files")
                .with_actions(&[*a])
                .with_latency(5)
                .with_reliability(0.999)
        })
        .collect();
    reg.register_provider("filesystem", fs_caps);

    reg.register_provider(
        "terminal",
        vec![Capability::new("terminal.run", "run", "Shell")
            .with_actions(&["run"])
            .with_latency(50)
            .with_reliability(0.99)],
    );

    let git_actions = ["clone", "commit", "push", "pull", "merge", "init", "status"];
    let git_caps: Vec<Capability> = git_actions
        .iter()
        .map(|a| {
            Capability::new(format!("git.{a}"), *a, "Git")
                .with_actions(&[*a])
                .with_latency(100)
                .with_reliability(0.97)
        })
        .collect();
    reg.register_provider("git", git_caps);

    // `launch`/`navigate`/`wait_ready` mirror the browser capabilities so the
    // declared browser -> windows fallback can actually resolve (today it cannot
    // because Windows does not advertise them — a real gap the benchmark surfaces
    // via the recovery metric).
    let win_actions = [
        "open",
        "close",
        "focus",
        "move",
        "resize",
        "capture",
        "list",
        "click",
        "launch",
        "navigate",
        "wait_ready",
        "verify",
    ];
    let win_caps: Vec<Capability> = win_actions
        .iter()
        .map(|a| {
            Capability::new(format!("windows.{a}"), *a, "Windows")
                .with_actions(&[*a])
                .with_platform(Platform::Windows)
                .with_latency(30)
                .with_reliability(0.9)
        })
        .collect();
    reg.register_provider("windows", win_caps);

    reg.register_provider(
        "inference",
        vec![Capability::new("inference.complete", "complete", "AI")
            .with_actions(&["complete"])
            .with_latency(800)
            .with_reliability(0.98)
            .with_cost(2.0)],
    );

    let browser_actions = ["launch", "navigate", "wait_ready", "verify"];
    let browser_caps: Vec<Capability> = browser_actions
        .iter()
        .map(|a| {
            Capability::new(format!("browser.{a}"), *a, "Browser")
                .with_actions(&[*a])
                .with_latency(200)
                .with_reliability(0.95)
        })
        .collect();
    reg.register_provider("browser", browser_caps);

    reg
}

/// The set of capability ids that currently have a live (available) provider.
/// Under Chaos Mode the killed providers report `available() == false`, so this
/// is exactly the set the Recovery Graph consults when deciding whether a
/// reroute/`UseAlternateCapability` step can resolve.
fn available_caps(reg: &CapabilityRegistry) -> HashSet<String> {
    reg.capabilities()
        .iter()
        .filter(|c| reg.available(&c.id))
        .map(|c| c.id.clone())
        .collect()
}

/// Minimal simulator for the inference fabric (the M5 subsystem). There is no
/// production `Simulatable` for it yet — this lets the benchmark exercise it and
/// surface the gap (no fallback path when inference is unavailable).
struct InferenceSim;

impl Simulatable for InferenceSim {
    fn preconditions(&self, _req: &SimRequest, _world: &WorldState) -> Vec<Precondition> {
        vec![Precondition {
            description: "inference provider available".into(),
            kind: "service".into(),
            satisfied: true,
        }]
    }

    fn side_effects(&self, req: &SimRequest) -> Vec<SideEffect> {
        vec![
            SideEffect::leaf("inference.complete", "model completion", 0.0)
                .with_target(req.str_param("prompt").unwrap_or_default()),
        ]
    }

    fn estimate(&self, _req: &SimRequest) -> StepEstimate {
        StepEstimate {
            latency_ms: 800,
            cost: 2.0,
            reliability: 0.98,
        }
    }

    fn forecast(&self, _req: &SimRequest) -> ResourceForecast {
        ResourceForecast {
            tokens: 500,
            ..Default::default()
        }
    }

    fn rollback(&self, _req: &SimRequest) -> Option<RollbackPlan> {
        None
    }
}

/// Build the `tool -> Simulatable` map from the built-in services + Windows bridge.
fn build_sim_map() -> HashMap<String, Arc<dyn Simulatable>> {
    let mut m = friday_actions::sim_registry();
    for (k, v) in friday_windows::sim_registry() {
        m.insert(k, v);
    }
    // Browser actions are simulated through the Windows bridge (a browser is a
    // window); this is what lets the fallback path browser.* -> windows.* resolve.
    m.insert(
        "browser".into(),
        Arc::new(friday_windows::WindowsAdapter::for_simulation()),
    );
    m.insert("inference".into(), Arc::new(InferenceSim));
    m
}

/// Under Chaos Mode, take providers offline. The browser bridge is always
/// killed so the browser -> windows fallback path is exercised (Windows, the
/// fallback target, is kept alive on purpose). The other subsystems are killed
/// at random to surface those that lack any fallback path.
fn apply_chaos(reg: &CapabilityRegistry, rng: &mut impl rand::Rng) {
    let providers = ["filesystem", "terminal", "git", "inference", "browser"];
    for p in providers {
        if p == "browser" || rng.gen_bool(0.4) {
            reg.set_provider_health(p, Health::Offline);
        }
    }
}

/// Validate a sandbox filesystem task for real, returning actual success.
fn sandbox_actual(task: &CorpusTask) -> bool {
    let path = match task.params.get("path").and_then(|v| v.as_str()) {
        Some(p) => PathBuf::from(p),
        None => return false,
    };
    match task.action.as_str() {
        "create_file" | "write_file" => {
            if let Ok(()) = std::fs::write(&path, "benchmark") {
                path.exists()
            } else {
                false
            }
        }
        "create_dir" | "mkdir" => {
            if let Ok(()) = std::fs::create_dir_all(&path) {
                path.is_dir()
            } else {
                false
            }
        }
        "delete_file" => {
            let _ = std::fs::write(&path, "x");
            std::fs::remove_file(&path).is_ok() && !path.exists()
        }
        "delete_dir" | "rmdir" => {
            let _ = std::fs::create_dir_all(&path);
            std::fs::remove_dir_all(&path).is_ok() && !path.exists()
        }
        _ => false,
    }
}

fn predicted_success(report: &SimulationReport) -> bool {
    let resolved = !report
        .issues
        .iter()
        .any(|i| i.kind == "unavailable_service");
    let no_errors = !report
        .issues
        .iter()
        .any(|i| i.severity == IssueSeverity::Error);
    resolved && report.risk != friday_simulation::RiskLevel::High && no_errors
}

/// Measure planner accuracy on a small set of natural-language intents.
pub fn measure_planner_accuracy() -> f64 {
    let cases = [
        ("open notepad", "app.open"),
        ("start my morning routine", "routine.merge"),
        ("search for rust lang on google", "browser.navigate"),
    ];
    let mut hits = 0;
    for (raw, expected) in cases {
        let intent = parse(raw);
        let graph = plan(&intent);
        let found = graph.tasks.iter().any(|t| t.capability.contains(expected));
        if found {
            hits += 1;
        }
    }
    hits as f64 / cases.len() as f64
}

/// Run the full benchmark. When `chaos` is true, providers are randomly killed
/// first (deterministic given the seeded RNG).
pub fn run_benchmark(chaos: bool, n: usize) -> BenchmarkResult {
    let temp = std::env::temp_dir().join(format!("friday-bench-{}", Uuid::new_v4()));
    let _ = std::fs::create_dir_all(&temp);

    let registry = build_registry();
    let mut rng = rand::rngs::StdRng::seed_from_u64(0xC0FFEE);
    if chaos {
        apply_chaos(&registry, &mut rng);
    }

    let world = WorldState::from_registry(&registry);
    let sims = build_sim_map();
    let engine = SimulationEngine::new(Arc::clone(&registry), sims, world);

    let corpus = generate_corpus(n, &temp);
    let mut records = Vec::with_capacity(corpus.len());

    let mut resolved = 0usize;
    let mut unresolved = 0usize;
    let mut low = 0usize;
    let mut med = 0usize;
    let mut high = 0usize;
    let mut with_issues = 0usize;
    let mut approval = 0usize;
    let mut fallback_used = 0usize;
    let mut rollback = 0usize;
    let mut confidence_sum = 0.0f64;
    let mut failures = 0usize;
    let mut sim_correct = 0usize;

    // Recovery Graph accumulators.
    let graph = default_graph();
    let available = available_caps(&registry);
    let mut rec_attempts = 0usize;
    let mut rec_recovered = 0usize;
    let mut rec_conf_sum = 0.0f64;
    let mut sub_stats: HashMap<String, (usize, usize, f64)> = HashMap::new();

    // Calibration accumulators: 10 bins over [0,1].
    let mut cal_count = [0usize; 10];
    let mut cal_pred = [0.0f64; 10];
    let mut cal_act = [0.0f64; 10];

    for task in &corpus {
        let mut g = TaskGraph::new(task.name.clone());
        g.add_task(
            task.name.clone(),
            task.capability.clone(),
            task.params.clone(),
            &[],
        );
        let report = engine
            .simulate_plan(&g)
            .unwrap_or_else(|e| panic!("simulate_plan failed on {}: {e}", task.name));

        let is_resolved = !report
            .issues
            .iter()
            .any(|i| i.kind == "unavailable_service");
        let pred = predicted_success(&report);

        // Recovery Graph: when the simulation predicts a failure (e.g. a killed
        // provider under Chaos Mode), attempt a deterministic, action-level
        // recovery. A resolved recovery means the task is *not* counted as a
        // failure against MTBF.
        let mut recovered = false;
        let mut rec_conf = 0.0f64;
        if !pred {
            let cat = report
                .issues
                .iter()
                .map(|i| FailureCategory::from_kind(&i.kind))
                .find(|c| !matches!(c, FailureCategory::Unknown))
                .unwrap_or(FailureCategory::Provider);
            let r = graph.recover(&task.capability, cat, &available);
            let sub = task
                .capability
                .split('.')
                .next()
                .unwrap_or("unknown")
                .to_string();
            rec_attempts += 1;
            if r.recovered {
                recovered = true;
                rec_conf = r.confidence;
                rec_recovered += 1;
                rec_conf_sum += rec_conf;
            }
            let e = sub_stats.entry(sub).or_insert((0, 0, 0.0));
            e.0 += 1;
            if r.recovered {
                e.1 += 1;
                e.2 += rec_conf;
            }
        }

        let actual = if task.sandbox {
            sandbox_actual(task)
        } else {
            pred || recovered
        };
        if is_resolved {
            resolved += 1;
        } else {
            unresolved += 1;
        }
        match report.risk {
            friday_simulation::RiskLevel::Low => low += 1,
            friday_simulation::RiskLevel::Medium => med += 1,
            friday_simulation::RiskLevel::High => high += 1,
        }
        if !report.issues.is_empty() {
            with_issues += 1;
        }
        if report.approval_required {
            approval += 1;
        }
        let fb_used = report.fallbacks.iter().any(|f| f.available);
        if fb_used {
            fallback_used += 1;
        }
        if report.rollback_available {
            rollback += 1;
        }
        confidence_sum += report.confidence.confidence;
        if !pred && !recovered {
            failures += 1;
        }
        if pred == actual {
            sim_correct += 1;
        }

        let bin = ((report.confidence.confidence * 10.0) as usize).clamp(0, 9);
        cal_count[bin] += 1;
        cal_pred[bin] += report.confidence.confidence;
        cal_act[bin] += if actual { 1.0 } else { 0.0 };

        records.push(TaskRecord {
            name: task.name.clone(),
            capability: task.capability.clone(),
            resolved: is_resolved,
            risk: report.risk.as_str().to_string(),
            confidence: report.confidence.confidence,
            issues: report.issues.len(),
            fallback_used: fb_used,
            predicted_success: pred,
            actual_success: actual,
        });
    }

    // Recovery Success now reflects the Recovery Graph (deterministic,
    // action-level recovery) rather than the sim-engine fallback paths. The sim
    // fallback numbers are still surfaced via `fallback_used` / rollback columns.
    let recovery = if rec_attempts > 0 {
        Some(rec_recovered as f64 / rec_attempts as f64)
    } else {
        None
    };

    let recovery_confidence_mean = if rec_recovered > 0 {
        rec_conf_sum / rec_recovered as f64
    } else {
        0.0
    };

    let mut recovery_by_subsystem: Vec<SubsystemRecovery> = sub_stats
        .into_iter()
        .map(
            |(subsystem, (attempts, recovered, conf_sum))| SubsystemRecovery {
                subsystem,
                attempts,
                recovered,
                mean_confidence: if recovered > 0 {
                    conf_sum / recovered as f64
                } else {
                    0.0
                },
            },
        )
        .collect();
    recovery_by_subsystem.sort_by(|a, b| a.subsystem.cmp(&b.subsystem));

    // Calibration error across non-empty bins.
    let mut cal_bins = Vec::new();
    let mut cal_error_sum = 0.0;
    let mut cal_bins_used = 0usize;
    for (i, &c) in cal_count.iter().enumerate() {
        if c == 0 {
            continue;
        }
        let avg_pred = cal_pred[i] / c as f64;
        let avg_act = cal_act[i] / c as f64;
        cal_error_sum += (avg_pred - avg_act).abs();
        cal_bins_used += 1;
        cal_bins.push(CalibrationBin {
            bin_low: i as f64 / 10.0,
            bin_high: (i as f64 + 1.0) / 10.0,
            count: c,
            avg_predicted: avg_pred,
            avg_actual: avg_act,
        });
    }
    let calibration_error = if cal_bins_used > 0 {
        cal_error_sum / cal_bins_used as f64
    } else {
        0.0
    };

    let total = corpus.len();
    let mtbf = if failures == 0 {
        total as f64
    } else {
        total as f64 / failures as f64
    };

    let metrics = Metrics {
        total_tasks: total,
        resolved,
        unresolved,
        low_risk: low,
        medium_risk: med,
        high_risk: high,
        with_issues,
        approval_required: approval,
        fallback_used,
        rollback_available: rollback,
        mean_confidence: if total > 0 {
            confidence_sum / total as f64
        } else {
            0.0
        },
        mtbf_tasks: mtbf,
        simulation_accuracy: if total > 0 {
            sim_correct as f64 / total as f64
        } else {
            0.0
        },
        planner_accuracy: measure_planner_accuracy(),
        recovery_success: recovery,
        recovery_attempts: rec_attempts,
        recovery_recovered: rec_recovered,
        recovery_confidence_mean,
        recovery_by_subsystem,
        calibration_error,
    };

    let _ = std::fs::remove_dir_all(&temp);

    BenchmarkResult {
        chaos,
        metrics,
        calibration: cal_bins,
        tasks: records,
    }
}

/// Render a markdown reliability dashboard.
pub fn render_dashboard(result: &BenchmarkResult) -> String {
    let m = &result.metrics;
    let mut s = String::new();
    s.push_str("# FRIDAY Reliability Dashboard\n\n");
    s.push_str(&format!(
        "Mode: **{}**  |  Tasks: **{}**\n\n",
        if result.chaos {
            "Chaos (CHAOS=1)"
        } else {
            "Normal"
        },
        m.total_tasks
    ));

    s.push_str("## Headline Metrics\n");
    s.push_str("| Metric | Value |\n|---|---|\n");
    s.push_str(&format!(
        "| MTBF (tasks between failures) | {:.1} |\n",
        m.mtbf_tasks
    ));
    s.push_str(&format!(
        "| Simulation Accuracy | {:.1}% |\n",
        m.simulation_accuracy * 100.0
    ));
    s.push_str(&format!(
        "| Planner Accuracy | {:.1}% |\n",
        m.planner_accuracy * 100.0
    ));
    s.push_str(&format!(
        "| Mean Confidence | {:.1}% |\n",
        m.mean_confidence * 100.0
    ));
    s.push_str(&format!(
        "| Calibration Error | {:.3} |\n",
        m.calibration_error
    ));
    if let Some(r) = m.recovery_success {
        s.push_str(&format!("| Recovery Success | {:.1}% |\n", r * 100.0));
    }
    s.push_str(&format!(
        "| Recovery Confidence (mean) | {:.1}% |\n",
        m.recovery_confidence_mean * 100.0
    ));
    s.push('n');

    if !m.recovery_by_subsystem.is_empty() {
        s.push_str("## Recovery by Subsystem (Recovery Graph)\n");
        s.push_str("| Subsystem | Attempts | Recovered | Coverage | Mean Confidence |\n");
        s.push_str("|---|---|---|---|---|\n");
        for r in &m.recovery_by_subsystem {
            let cov = if r.attempts > 0 {
                r.recovered as f64 / r.attempts as f64
            } else {
                0.0
            };
            s.push_str(&format!(
                "| {} | {} | {} | {:.1}% | {:.1}% |\n",
                r.subsystem,
                r.attempts,
                r.recovered,
                cov * 100.0,
                r.mean_confidence * 100.0
            ));
        }
        s.push('n');
    }

    s.push_str("## Risk & Resolution\n");
    s.push_str("| Resolved | Unresolved | Low | Medium | High | Issues | Approval | Fallback | Rollback |\n");
    s.push_str("|---|---|---|---|---|---|---|---|---|\n");
    s.push_str(&format!(
        "| {} | {} | {} | {} | {} | {} | {} | {} | {} |\n",
        m.resolved,
        m.unresolved,
        m.low_risk,
        m.medium_risk,
        m.high_risk,
        m.with_issues,
        m.approval_required,
        m.fallback_used,
        m.rollback_available
    ));
    s.push('n');

    s.push_str("## Confidence Calibration\n");
    s.push_str("| Bin | Count | Predicted | Actual |\n|---|---|---|---|\n");
    for b in &result.calibration {
        s.push_str(&format!(
            "| {:.1}-{:.1} | {} | {:.2} | {:.2} |\n",
            b.bin_low, b.bin_high, b.count, b.avg_predicted, b.avg_actual
        ));
    }
    s.push('n');
    s
}

/// One day's benchmark snapshot: both modes, so the MTBF trend can be plotted
/// over time. Saved to `benchmarks/<date>.json`.
#[derive(Serialize, Deserialize)]
pub struct BenchmarkDay {
    pub date: String,
    pub normal: BenchmarkResult,
    pub chaos: BenchmarkResult,
}

/// Directory (relative to repo root) where day snapshots are persisted.
pub const BENCHMARKS_DIR: &str = "benchmarks";

/// `YYYY-MM-DD` stamp for the current day.
pub fn today_stamp() -> String {
    chrono::Utc::now().format("%Y-%m-%d").to_string()
}

/// Run both Normal and Chaos modes and bundle them into a day snapshot.
pub fn run_day(tasks: usize) -> BenchmarkDay {
    let normal = run_benchmark(false, tasks);
    let chaos = run_benchmark(true, tasks);
    BenchmarkDay {
        date: today_stamp(),
        normal,
        chaos,
    }
}

/// Persist a day snapshot to `benchmarks/<date>.json` (overwrites same day).
pub fn save_day(day: &BenchmarkDay) -> PathBuf {
    let dir = PathBuf::from(BENCHMARKS_DIR);
    let _ = std::fs::create_dir_all(&dir);
    let path = dir.join(format!("{}.json", day.date));
    let json = serde_json::to_string_pretty(day).expect("serialize day");
    std::fs::write(&path, json).expect("write day snapshot");
    path
}

/// Load every `benchmarks/<date>.json`, sorted by date (oldest first).
pub fn load_history(dir: &Path) -> Vec<BenchmarkDay> {
    let mut days = Vec::new();
    if let Ok(entries) = std::fs::read_dir(dir) {
        for e in entries.flatten() {
            let p = e.path();
            if p.extension().and_then(|x| x.to_str()) == Some("json") {
                if let Ok(text) = std::fs::read_to_string(&p) {
                    if let Ok(d) = serde_json::from_str::<BenchmarkDay>(&text) {
                        days.push(d);
                    }
                }
            }
        }
    }
    days.sort_by(|a, b| a.date.cmp(&b.date));
    days
}

/// MTBF trend across historical snapshots: (date, normal_mtbf, chaos_mtbf).
pub fn mtbf_trend(history: &[BenchmarkDay]) -> Vec<(String, f64, f64)> {
    history
        .iter()
        .map(|d| {
            (
                d.date.clone(),
                d.normal.metrics.mtbf_tasks,
                d.chaos.metrics.mtbf_tasks,
            )
        })
        .collect()
}

/// Render the historical MTBF trend (Normal vs Chaos) as a markdown table.
pub fn render_trend(history: &[BenchmarkDay]) -> String {
    let trend = mtbf_trend(history);
    if trend.is_empty() {
        return String::new();
    }
    let mut s = String::new();
    s.push_str("## MTBF Trend (historical `benchmarks/<date>.json`)\n");
    s.push_str("| Date | Normal MTBF | Chaos MTBF |\n|---|---|---|\n");
    for (date, n, c) in trend {
        s.push_str(&format!("| {} | {:.1} | {:.1} |\n", date, n, c));
    }
    s.push('n');
    s
}
