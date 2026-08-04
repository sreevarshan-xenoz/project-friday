//! Evidence Packs and metric aggregation for Provider Qualification.
//!
//! Every scenario produces `report.json`, `trace.json`, `metrics.json`, and
//! (on failure) a benchmark-linked issue file. Aggregated metrics follow
//! `specs/provider-qualification.md`. Tool support, streaming reliability, and
//! dynamic discovery coverage are populated from the M5.2 Provider Capability
//! Layer (ProviderV2) — they were `None` until the V2 surface existed.

use std::fs;
use std::path::Path;

use serde::Serialize;
use serde_json::json;

use crate::environment::Environment;
use crate::harness::Outcome;
use crate::issue::Issue;

/// Provider-specific qualification metrics (see the spec).
#[derive(Debug, Clone, Serialize)]
pub struct Metrics {
    pub availability: f64,
    pub completion_success: f64,
    pub json_correctness: f64,
    pub context_handling: f64,
    pub failure_handling_coverage: Option<f64>,
    pub tool_support: Option<f64>,
    pub streaming_reliability: Option<f64>,
    pub dynamic_discovery: Option<f64>,
    pub mean_latency_ms: f64,
    pub passed: usize,
    pub total: usize,
}

impl Default for Metrics {
    fn default() -> Self {
        Self {
            availability: 0.0,
            completion_success: 0.0,
            json_correctness: 0.0,
            context_handling: 0.0,
            failure_handling_coverage: None,
            tool_support: None,
            streaming_reliability: None,
            dynamic_discovery: None,
            mean_latency_ms: 0.0,
            passed: 0,
            total: 0,
        }
    }
}

/// Aggregate outcomes into the provider qualification metrics.
pub fn aggregate(outcomes: &[Outcome]) -> Metrics {
    let mut m = Metrics::default();
    let total = outcomes.len();
    let passed = outcomes.iter().filter(|o| o.passed).count();

    let mut buckets: std::collections::HashMap<String, (usize, usize)> =
        std::collections::HashMap::new();
    let mut lat_sum = 0u128;
    let mut lat_n = 0usize;

    for o in outcomes {
        *buckets.entry(o.group.clone()).or_insert((0, 0)) = {
            let (p, t) = buckets.get(&o.group).copied().unwrap_or((0, 0));
            (p + if o.passed { 1 } else { 0 }, t + 1)
        };
        lat_sum += o.latency_ms;
        lat_n += 1;
    }

    let rate = |g: &str| -> f64 {
        buckets
            .get(g)
            .map(|(p, t)| if *t > 0 { *p as f64 / *t as f64 } else { 0.0 })
            .unwrap_or(0.0)
    };

    m.availability = rate("connectivity");
    m.completion_success = rate("completion");
    m.json_correctness = rate("json");
    m.context_handling = rate("long_context");
    let fail = rate("failure-injection");
    m.failure_handling_coverage = if buckets.contains_key("failure-injection") {
        Some(fail)
    } else {
        None
    };
    let stream = rate("streaming");
    m.streaming_reliability = if buckets.contains_key("streaming") {
        Some(stream)
    } else {
        None
    };
    let disc = rate("dynamic_discovery");
    m.dynamic_discovery = if buckets.contains_key("dynamic_discovery") {
        Some(disc)
    } else {
        None
    };
    // Tool support is a provider-level fact observed by V2 scenarios (streaming and
    // dynamic discovery both record the capability). If any V2 scenario observed it,
    // surface 1.0 (supported) or 0.0 (declared false).
    let tool_calls: Vec<bool> = outcomes.iter().filter_map(|o| o.tool_calling).collect();
    m.tool_support = if tool_calls.is_empty() {
        None
    } else {
        Some(if tool_calls.iter().any(|t| *t) {
            1.0
        } else {
            0.0
        })
    };
    m.mean_latency_ms = if lat_n > 0 {
        (lat_sum / lat_n as u128) as f64
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
        "fault": o.fault,
        "served_model": o.served_model,
        "routing_decision": o.routing_decision,
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

    // Overall progress = mean of all available (non-None) metrics.
    let mut vals: Vec<f64> = vec![
        m.availability,
        m.completion_success,
        m.json_correctness,
        m.context_handling,
    ];
    if let Some(f) = m.failure_handling_coverage {
        vals.push(f);
    }
    if let Some(t) = m.tool_support {
        vals.push(t);
    }
    if let Some(s) = m.streaming_reliability {
        vals.push(s);
    }
    if let Some(d) = m.dynamic_discovery {
        vals.push(d);
    }
    let overall = if vals.is_empty() {
        0.0
    } else {
        vals.iter().sum::<f64>() / vals.len() as f64
    };

    let fail = match m.failure_handling_coverage {
        Some(f) => pct(f),
        None => "n/a (real mode)".to_string(),
    };
    let tool = match m.tool_support {
        Some(t) => pct(t),
        None => "n/a".to_string(),
    };
    let stream = match m.streaming_reliability {
        Some(s) => pct(s),
        None => "n/a".to_string(),
    };
    let disc = match m.dynamic_discovery {
        Some(d) => pct(d),
        None => "n/a (no V2 target)".to_string(),
    };

    format!(
        "## Provider Qualification (Phase 1.5 + M5.2)\n\n\
         | Metric | Value |\n|---|---|\n\
         | Availability | {} |\n\
         | Completion Success | {} |\n\
         | JSON Correctness | {} |\n\
         | Context Handling | {} |\n\
         | Failure Handling | {} |\n\
         | Tool Support | {} |\n\
         | Streaming Reliability | {} |\n\
         | Dynamic Discovery | {} |\n\
         | Mean Latency | {:.0} ms |\n\
         | Scenarios passed | {}/{} |\n\n\
         Current Qualification: Provider  {} {}\n",
        pct(m.availability),
        pct(m.completion_success),
        pct(m.json_correctness),
        pct(m.context_handling),
        fail,
        tool,
        stream,
        disc,
        m.mean_latency_ms,
        m.passed,
        m.total,
        bar(overall),
        pct(overall),
    )
}
