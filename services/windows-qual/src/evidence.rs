//! Evidence Packs and metric aggregation for Sprint 001A.
//!
//! Every scenario produces `report.json`, `trace.json`, `metrics.json`, and
//! (for captures) `screenshot.png`. Failures also produce a benchmark-linked
//! issue file. Aggregated metrics follow `specs/windows-qualification.md`.

use std::fs;
use std::path::Path;

use serde::Serialize;
use serde_json::json;

use crate::harness::Outcome;
use crate::issue::Issue;

use crate::environment::Environment;

/// Breakdown of how launch intents were resolved by the host. Both `New Instance`
/// and `Existing Instance` are successes; `Failed` is a genuine failure. Tracking
/// this makes single-instance application behavior visible in qualification.
#[derive(Debug, Clone, Serialize, Default)]
pub struct LaunchResolution {
    pub new_instance: usize,
    pub existing_instance: usize,
    pub failed: usize,
}

/// Windows-specific qualification metrics (see the spec). Recovery metrics are
/// `None` in 001A because the harness deliberately contains no recovery.
#[derive(Debug, Clone, Serialize)]
pub struct Metrics {
    pub launch_success: f64,
    pub focus_accuracy: f64,
    pub input_accuracy: f64,
    pub window_detection: f64,
    pub clipboard_reliability: f64,
    pub geometry_accuracy: f64,
    pub recovery_success: Option<f64>,
    pub recovery_cost_ms: Option<f64>,
    pub mean_interaction_time_ms: f64,
    pub state_accuracy: f64,
    pub passed: usize,
    pub total: usize,
    pub launch_resolution: LaunchResolution,
}

impl Default for Metrics {
    fn default() -> Self {
        Self {
            launch_success: 0.0,
            focus_accuracy: 0.0,
            input_accuracy: 0.0,
            window_detection: 0.0,
            clipboard_reliability: 0.0,
            geometry_accuracy: 0.0,
            recovery_success: None,
            recovery_cost_ms: None,
            mean_interaction_time_ms: 0.0,
            state_accuracy: 0.0,
            passed: 0,
            total: 0,
            launch_resolution: LaunchResolution::default(),
        }
    }
}

/// Aggregate outcomes into the Windows qualification metrics.
pub fn aggregate(outcomes: &[Outcome]) -> Metrics {
    let mut m = Metrics::default();
    let total = outcomes.len();
    let passed = outcomes.iter().filter(|o| o.passed).count();

    let mut buckets: std::collections::HashMap<String, (usize, usize)> =
        std::collections::HashMap::new();
    let mut lat_sum = 0u128;
    let mut lat_n = 0usize;
    let mut sa_sum = 0usize;
    let mut sa_n = 0usize;

    for o in outcomes {
        *buckets.entry(o.group.clone()).or_insert((0, 0)) = {
            let (p, t) = buckets.get(&o.group).copied().unwrap_or((0, 0));
            (p + if o.passed { 1 } else { 0 }, t + 1)
        };
        lat_sum += o.latency_ms;
        lat_n += 1;
        if let Some(sa) = o.state_accuracy {
            sa_sum += sa as usize;
            sa_n += 1;
        }
        match o.launch_resolution.as_deref() {
            Some("New Instance") => m.launch_resolution.new_instance += 1,
            Some("Existing Instance") => m.launch_resolution.existing_instance += 1,
            Some("Failed") => m.launch_resolution.failed += 1,
            _ => {}
        }
    }

    let rate = |g: &str| -> f64 {
        buckets
            .get(g)
            .map(|(p, t)| if *t > 0 { *p as f64 / *t as f64 } else { 0.0 })
            .unwrap_or(0.0)
    };

    m.launch_success = rate("launch");
    m.focus_accuracy = rate("focus");
    m.input_accuracy = rate("input");
    m.window_detection = rate("discovery");
    m.clipboard_reliability = rate("clipboard");
    m.geometry_accuracy = rate("geometry");
    m.mean_interaction_time_ms = if lat_n > 0 {
        (lat_sum / lat_n as u128) as f64
    } else {
        0.0
    };
    m.state_accuracy = if sa_n > 0 {
        sa_sum as f64 / sa_n as f64
    } else {
        0.0
    };
    m.passed = passed;
    m.total = total;
    m
}

/// Write one scenario's Evidence Pack under `dir/<id>/`.
pub fn write_outcome(dir: &Path, o: &Outcome) -> std::io::Result<()> {
    let scen_dir = dir.join(&o.id);
    fs::create_dir_all(&scen_dir)?;

    let report = json!({
        "id": o.id,
        "group": o.group,
        "action_ok": o.action_ok,
        "latency_ms": o.latency_ms,
        "state_accuracy": o.state_accuracy,
        "passed": o.passed,
        "launch_resolution": o.launch_resolution,
        "note": o.note,
        "issues": o.issues.iter().map(|i| i.id.clone()).collect::<Vec<_>>(),
    });
    fs::write(
        scen_dir.join("report.json"),
        serde_json::to_string_pretty(&report)?,
    )?;

    let trace: Vec<serde_json::Value> = o
        .trace
        .iter()
        .map(|c| {
            json!({
                "intent": c.intent,
                "ok": c.ok,
                "latency_ms": c.latency_ms,
                "ts": c.ts,
            })
        })
        .collect();
    fs::write(
        scen_dir.join("trace.json"),
        serde_json::to_string_pretty(&json!(trace))?,
    )?;

    let metrics = json!({
        "action_ok": o.action_ok,
        "latency_ms": o.latency_ms,
        "state_accuracy": o.state_accuracy,
        "passed": o.passed,
    });
    fs::write(
        scen_dir.join("metrics.json"),
        serde_json::to_string_pretty(&metrics)?,
    )?;

    if let Some(bytes) = &o.screenshot {
        fs::write(scen_dir.join("screenshot.png"), bytes)?;
    }
    Ok(())
}

/// Write a benchmark-linked issue file (a graduated, real issue).
pub fn write_issue(dir: &Path, issue: &Issue) -> std::io::Result<()> {
    let issues_dir = dir.join("issues");
    fs::create_dir_all(&issues_dir)?;
    fs::write(
        issues_dir.join(format!("{}.md", issue.id)),
        issue.to_markdown(),
    )
}

/// Write a *candidate* issue file (not yet graduated to the tracker).
pub fn write_candidate(dir: &Path, issue: &Issue) -> std::io::Result<()> {
    let cand_dir = dir.join("candidates");
    fs::create_dir_all(&cand_dir)?;
    fs::write(
        cand_dir.join(format!("{}.md", issue.id)),
        issue.to_markdown(),
    )
}

/// Write the run's `environment.json`.
pub fn write_environment(dir: &Path, env: &Environment) -> std::io::Result<()> {
    fs::write(
        dir.join("environment.json"),
        serde_json::to_string_pretty(env)?,
    )
}

/// Render a compact qualification summary.
pub fn render(m: &Metrics) -> String {
    let pct = |x: f64| format!("{:.0}%", x * 100.0);
    let bar = |x: f64| {
        let filled = (x * 10.0).round().clamp(0.0, 10.0) as usize;
        format!("{}{}", "█".repeat(filled), "░".repeat(10 - filled))
    };
    let rec = match m.recovery_success {
        Some(r) => pct(r),
        None => "n/a".to_string(),
    };
    format!(
        "## Windows Qualification (Sprint 001A)\n\n\
         | Metric | Value |\n|---|---|\n\
         | Launch Success | {} |\n\
         | Focus Accuracy | {} |\n\
         | Input Accuracy | {} |\n\
         | Window Detection | {} |\n\
         | Clipboard Reliability | {} |\n\
         | Geometry Accuracy | {} |\n\
         | Recovery Success | {} |\n\
         | Mean Interaction Time | {:.0} ms |\n\
         | State Accuracy | {} |\n\
         | Launch Resolution | New {} / Existing {} / Failed {} |\n\
         | Scenarios passed | {}/{} |\n\n\
         Current Qualification: Windows  {} {}\n",
        pct(m.launch_success),
        pct(m.focus_accuracy),
        pct(m.input_accuracy),
        pct(m.window_detection),
        pct(m.clipboard_reliability),
        pct(m.geometry_accuracy),
        rec,
        m.mean_interaction_time_ms,
        pct(m.state_accuracy),
        m.launch_resolution.new_instance,
        m.launch_resolution.existing_instance,
        m.launch_resolution.failed,
        m.passed,
        m.total,
        bar(m.state_accuracy),
        pct(m.state_accuracy),
    )
}
