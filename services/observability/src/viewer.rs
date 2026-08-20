//! Phase 6D — Cognitive Trace Viewer.
//!
//! Built directly from the frozen `CognitiveTrace` records via
//! `reconstruct_trace_tree()` (friday-core) and `CognitiveMetrics` (6C). The
//! viewer does NOT introduce a second trace model, a second hierarchy, or any
//! OpenTelemetry dependency. The same records that feed OTel (6A/6B) and the
//! metrics (6C) feed the viewer — there is one semantic implementation.
//!
//! ```text
//!                CognitiveTrace records
//!                        │
//!              ┌─────────┴─────────┐
//!              ▼                   ▼
//!      reconstruct_trace_tree()   CognitiveMetrics
//!              │                   │
//!              └─────────┬─────────┘
//!                        ▼
//!                   Trace Viewer
//! ```
//!
//! The tree answers "what caused what?"; the timeline answers "what happened
//! when?". Both derive from existing fields only (`started_at`, `duration_ms`).

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use friday_core::trace::{
    reconstruct_trace_tree, CognitiveTrace, ResultType, TraceId, TraceTreeNode,
};
use serde::Serialize;

use crate::metrics::CognitiveMetrics;

/// A single span projected for the parallel-execution timeline. This is NOT a
/// trace model — it is a scheduling view derived purely from the existing
/// `started_at` / `duration_ms` fields of `CognitiveTrace`.
#[derive(Debug, Clone, Serialize)]
pub struct TimelineEntry {
    pub span_id: friday_core::trace::SpanId,
    pub label: String,
    pub organ: String,
    pub start_offset_ms: i64,
    pub duration_ms: u64,
}

/// One cognitive episode (all spans sharing one `trace_id`), rendered as the
/// real reconstructed tree plus the 6C metrics for exactly this episode.
#[derive(Debug, Clone)]
pub struct EpisodeView {
    pub trace_id: TraceId,
    /// The real reconstructed hierarchy (friday-core type — no new model).
    pub root: TraceTreeNode,
    /// Per-episode metrics, produced by the same 6C engine (no recalculation).
    pub metrics: crate::metrics::CognitiveMetricsSnapshot,
    pub timeline: Vec<TimelineEntry>,
}

impl EpisodeView {
    /// Episode outcome is taken from the ROOT span's result — never inferred
    /// from child existence.
    pub fn status(&self) -> ResultType {
        self.root.trace.result
    }

    /// Whether the root was verified (from root verification_info only).
    pub fn verified(&self) -> bool {
        self.root.trace.verification().is_passed()
    }

    /// Explorer-style one-line summary, computed from 6C metrics.
    pub fn summary(&self) -> EpisodeSummary {
        EpisodeSummary {
            trace_id: self.trace_id,
            status: self.status(),
            verified: self.verified(),
            total_duration_ms: self.metrics.total_duration_ms,
            total_cost_usd: self.metrics.total_cost_usd,
        }
    }
}

/// A compact episode row for the Trace Explorer listing.
#[derive(Debug, Clone, Serialize)]
pub struct EpisodeSummary {
    pub trace_id: TraceId,
    pub status: ResultType,
    pub verified: bool,
    pub total_duration_ms: u64,
    pub total_cost_usd: f64,
}

/// The full viewer model handed to the frontend: the per-episode trees and the
/// global (cross-episode) metrics. One semantic implementation; the frontend
/// receives `{ episodes: [{tree, metrics, timeline}], global_metrics }`.
#[derive(Debug, Clone)]
pub struct ViewerModel {
    pub episodes: Vec<EpisodeView>,
    pub global_metrics: crate::metrics::CognitiveMetricsSnapshot,
}

impl ViewerModel {
    /// Build the viewer model from raw `CognitiveTrace` records.
    ///
    /// Groups by `trace_id` into episodes, reconstructs each tree, and runs the
    /// 6C `CognitiveMetrics` over both the per-episode and global scope. No
    /// semantic logic lives here that is not already in `reconstruct_trace_tree`
    /// or `CognitiveMetrics`.
    pub fn build(traces: &[CognitiveTrace]) -> Self {
        let mut by_trace: HashMap<TraceId, Vec<CognitiveTrace>> = HashMap::new();
        for t in traces {
            by_trace.entry(t.trace_id).or_default().push(t.clone());
        }

        let mut episodes = Vec::new();
        let global = CognitiveMetrics::new();
        for (trace_id, eps) in by_trace {
            let tree = reconstruct_trace_tree(&eps);
            let local = CognitiveMetrics::new();
            for t in &eps {
                local.record(t);
                global.record(t);
            }
            let timeline = build_timeline(&eps);
            // An episode has exactly one root (enforced by reconstruct_trace_tree).
            let root = tree
                .into_iter()
                .next()
                .expect("episode must contain a root span");
            episodes.push(EpisodeView {
                trace_id,
                root,
                metrics: local.snapshot(),
                timeline,
            });
        }
        // Deterministic ordering for stable output / tests.
        episodes.sort_by_key(|e| e.trace_id.to_string());
        ViewerModel {
            episodes,
            global_metrics: global.snapshot(),
        }
    }

    /// JSON/API payload consumed by the frontend: `{ episodes, global_metrics }`.
    /// The tree is serialized via the existing `CognitiveTrace` records — no
    /// second schema.
    pub fn to_json(&self) -> String {
        let episodes: Vec<serde_json::Value> = self
            .episodes
            .iter()
            .map(|ep| {
                serde_json::json!({
                    "trace_id": ep.trace_id.to_string(),
                    "status": ep.status().to_string(),
                    "verified": ep.verified(),
                    "tree": tree_to_json(&ep.root),
                    "metrics": &ep.metrics,
                    "timeline": &ep.timeline,
                })
            })
            .collect();
        serde_json::json!({
            "episodes": episodes,
            "global_metrics": &self.global_metrics,
        })
        .to_string()
    }

    /// Trace Explorer listing (recent episodes, summary rows).
    pub fn render_explorer(&self) -> String {
        let mut out = String::from("Recent cognitive episodes\n\n");
        for ep in &self.episodes {
            let s = ep.summary();
            let mark = match s.status {
                ResultType::Success => "✓",
                ResultType::Failure => "✗",
                _ => "·",
            };
            out.push_str(&format!(
                "{} {}  {:>7.1?}s  ${:.4}  {}\n",
                mark,
                s.trace_id,
                s.total_duration_ms as f64 / 1000.0,
                s.total_cost_usd,
                if s.verified { "VERIFIED" } else { "" },
            ));
        }
        out
    }

    /// Trace Detail: the hierarchical tree with status markers + metrics summary.
    pub fn render_detail(&self, trace_id: &TraceId) -> String {
        let ep = self
            .episodes
            .iter()
            .find(|e| &e.trace_id == trace_id)
            .expect("episode exists in model");
        let mut out = String::new();
        out.push_str(&format!(
            "GOAL {}  {}\n",
            ep.trace_id,
            if ep.verified() { "VERIFIED" } else { "" }
        ));
        out.push_str(&format!(
            "Total: {:.1?}s   Cost: ${:.4}\n",
            ep.metrics.total_duration_ms as f64 / 1000.0,
            ep.metrics.total_cost_usd
        ));
        out.push_str(&format!(
            "Tokens: {}   Agents: {}   Tools: {}   Verification: {}\n\n",
            ep.metrics.total_tokens_in + ep.metrics.total_tokens_out,
            ep.metrics.agent_runs,
            ep.metrics.tool_calls,
            if ep.metrics.verification_failures > 0 {
                "FAIL"
            } else {
                "PASS"
            }
        ));
        render_tree(&ep.root, 0, &mut out);
        out
    }

    /// Parallel-execution timeline derived from `started_at` / `duration_ms`.
    /// Exposes serial bottlenecks, parallel execution, and slow spans.
    pub fn render_timeline(&self, trace_id: &TraceId) -> String {
        let ep = self
            .episodes
            .iter()
            .find(|e| &e.trace_id == trace_id)
            .expect("episode exists in model");
        let width: u64 = 40;
        let max_end = ep
            .timeline
            .iter()
            .map(|t| t.start_offset_ms as u64 + t.duration_ms)
            .max()
            .unwrap_or(1)
            .max(1);

        let mut out = String::new();
        for entry in &ep.timeline {
            let pad = (entry.start_offset_ms as u64 * width / max_end) as usize;
            let len = ((entry.duration_ms * width / max_end) as usize).max(1);
            let bar = format!("{:>pad$}{}", "", "█".repeat(len), pad = pad);
            out.push_str(&format!("{:<12} {}\n", entry.organ, bar));
        }
        out
    }
}

fn render_tree(node: &TraceTreeNode, depth: usize, out: &mut String) {
    let mark = match node.trace.result {
        ResultType::Success => "✓",
        ResultType::Failure => "✗",
        _ => "·",
    };
    let indent = "  ".repeat(depth);
    out.push_str(&format!(
        "{}▼ {} [{}] {}  {:.1?}s\n",
        indent,
        node.trace.operation,
        node.trace.organ,
        mark,
        node.trace.duration_ms as f64 / 1000.0
    ));
    for child in &node.children {
        render_tree(child, depth + 1, out);
    }
}

fn build_timeline(eps: &[CognitiveTrace]) -> Vec<TimelineEntry> {
    let min_start: Option<DateTime<Utc>> = eps.iter().map(|t| t.started_at).min();
    match min_start {
        Some(min) => eps
            .iter()
            .map(|t| {
                let offset = (t.started_at - min).num_milliseconds();
                TimelineEntry {
                    span_id: t.span_id,
                    label: t.operation.clone(),
                    organ: t.organ.clone(),
                    start_offset_ms: offset,
                    duration_ms: t.duration_ms,
                }
            })
            .collect(),
        None => Vec::new(),
    }
}

fn tree_to_json(node: &TraceTreeNode) -> serde_json::Value {
    serde_json::json!({
        "trace": &node.trace,
        "children": node.children.iter().map(tree_to_json).collect::<Vec<_>>(),
    })
}

/// Click-a-span detail: a flat key/value projection of the `CognitiveTrace`
/// fields. Model and verification fields appear only when present on the span —
/// the viewer never synthesizes them.
pub fn describe_span(trace: &CognitiveTrace) -> Vec<(String, String)> {
    let mut rows: Vec<(String, String)> = vec![
        ("Operation".into(), trace.operation.clone()),
        ("Organ".into(), trace.organ.clone()),
        ("Result".into(), trace.result.to_string()),
        ("Duration (ms)".into(), trace.duration_ms.to_string()),
        (
            "Parent span".into(),
            trace
                .parent_span_id
                .map(|p| p.to_string())
                .unwrap_or_else(|| "none".into()),
        ),
        ("Trace ID".into(), trace.trace_id.to_string()),
        ("Span ID".into(), trace.span_id.to_string()),
        ("Capability".into(), trace.operation.clone()),
        ("Outcome".into(), trace.result.to_string()),
        ("Metadata".into(), trace.metadata.to_string()),
        (
            "Error".into(),
            trace.error.clone().unwrap_or_else(|| "none".into()),
        ),
    ];

    if let Some(m) = &trace.model_usage {
        rows.push(("Model".into(), m.model.clone()));
        rows.push(("Provider".into(), m.provider.clone()));
        rows.push(("Tokens in".into(), m.tokens_in.unwrap_or(0).to_string()));
        rows.push(("Tokens out".into(), m.tokens_out.unwrap_or(0).to_string()));
        rows.push(("Cost (USD)".into(), format!("{:.6}", m.cost_usd.unwrap_or(0.0))));
        rows.push((
            "Finish reason".into(),
            m.finish_reason
                .clone()
                .unwrap_or_else(|| "none".into()),
        ));
    }

    if let Some(v) = &trace.verification_info {
        rows.push(("Verification status".into(), v.status.to_string()));
        rows.push(("Verdict".into(), v.status.to_string()));
        rows.push((
            "Evidence".into(),
            v.evidence_count
                .map(|e| e.to_string())
                .unwrap_or_else(|| "none".into()),
        ));
    }

    rows
}

/// Format `describe_span` output as a readable panel.
pub fn render_span_detail(trace: &CognitiveTrace) -> String {
    let mut out = String::new();
    for (k, v) in describe_span(trace) {
        out.push_str(&format!("{:<18} {}\n", k, v));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Duration as ChronoDuration, TimeZone, Utc};
    use friday_core::trace::{
        CognitiveTraceContext, GoalId, ModelUsage, SessionId, SpanId, TraceId,
        VerificationStatus,
    };

    /// Deterministic viewer fixture mirroring the 5E golden episode shape: a
    /// goal (Director) with three agent subtrees (Researcher, Analyst, Verifier),
    /// where the Verifier branch fails. Built entirely from real `CognitiveTrace`
    /// records via the canonical `CognitiveTraceContext` builder — no fake
    /// trace format.
    fn golden_episode() -> Vec<CognitiveTrace> {
        let session = SessionId::default();
        let goal = GoalId::default();
        let trace_id = TraceId::default();
        let base = Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap();
        let at = |ms: i64| base + ChronoDuration::milliseconds(ms);

        let ctx = CognitiveTraceContext::for_goal(trace_id, session, goal);

        // Root goal (Director) — success + verified.
        let goal_span = SpanId::default();
        let mut traces = vec![ctx
            .trace(goal_span, "goal", "resolve")
            .with_timing(at(0), 12_400)
            .with_result(ResultType::Success)
            .with_verification(VerificationStatus::Passed)];

        let agent_ctx = ctx.child(goal_span);

        // Researcher — success.
        let r = SpanId::default();
        traces.push(
            agent_ctx
                .trace(r, "researcher", "execute")
                .with_timing(at(100), 4_100)
                .with_result(ResultType::Success),
        );
        let r_ctx = agent_ctx.child(r);
        traces.push(
            r_ctx
                .trace(SpanId::default(), "researcher", "llm_call")
                .with_timing(at(200), 1_800)
                .with_model_usage(ModelUsage {
                    model: "gpt-4o".into(),
                    provider: "openai".into(),
                    tokens_in: Some(1200),
                    tokens_out: Some(380),
                    cached_tokens: None,
                    reasoning_tokens: None,
                    cost_usd: Some(0.0041),
                    finish_reason: Some("stop".into()),
                })
                .with_result(ResultType::Success),
        );
        traces.push(
            r_ctx
                .trace(SpanId::default(), "web", "tool_call")
                .with_timing(at(2100), 1_200)
                .with_result(ResultType::Success),
        );
        traces.push(
            r_ctx
                .trace(SpanId::default(), "verification", "verification_gate")
                .with_timing(at(3400), 300)
                .with_verification(VerificationStatus::Passed)
                .with_result(ResultType::Success),
        );

        // Analyst — success.
        let a = SpanId::default();
        traces.push(
            agent_ctx
                .trace(a, "analyst", "execute")
                .with_timing(at(150), 2_700)
                .with_result(ResultType::Success),
        );
        let a_ctx = agent_ctx.child(a);
        traces.push(
            a_ctx
                .trace(SpanId::default(), "analyst", "tool_call")
                .with_timing(at(300), 1_100)
                .with_result(ResultType::Success),
        );
        traces.push(
            a_ctx
                .trace(SpanId::default(), "verification", "verification_gate")
                .with_timing(at(1500), 250)
                .with_verification(VerificationStatus::Passed)
                .with_result(ResultType::Success),
        );

        // Verifier — FAILURE branch.
        let v = SpanId::default();
        traces.push(
            agent_ctx
                .trace(v, "verifier", "execute")
                .with_timing(at(200), 2_900)
                .with_result(ResultType::Failure),
        );
        let v_ctx = agent_ctx.child(v);
        traces.push(
            v_ctx
                .trace(SpanId::default(), "verifier", "tool_call")
                .with_timing(at(400), 1_400)
                .with_result(ResultType::Failure)
                .with_error("tool failed"),
        );
        traces.push(
            v_ctx
                .trace(SpanId::default(), "verification", "verification_gate")
                .with_timing(at(1900), 300)
                .with_verification(VerificationStatus::Failed {
                    reason: "evidence mismatch".into(),
                })
                .with_result(ResultType::Failure),
        );

        traces
    }

    #[test]
    fn viewer_receives_golden_episode_structure() {
        let traces = golden_episode();
        let model = ViewerModel::build(&traces);

        // 1 trace / 1 episode.
        assert_eq!(model.episodes.len(), 1);
        let ep = &model.episodes[0];

        // 1 root.
        assert_eq!(ep.root.trace.operation, "resolve");
        // 3 agents directly under the root.
        assert_eq!(ep.root.children.len(), 3);
        let organs: Vec<&str> = ep.root.children.iter().map(|c| c.trace.organ.as_str()).collect();
        assert!(organs.contains(&"researcher"));
        assert!(organs.contains(&"analyst"));
        assert!(organs.contains(&"verifier"));

        // Metrics computed by 6C (model + metrics, no frontend recalculation).
        let m = &ep.metrics;
        assert_eq!(m.goals, 1);
        assert_eq!(m.successes, 1);
        assert_eq!(m.failures, 0);
        assert_eq!(m.agent_runs, 3);
        assert_eq!(m.model_calls, 1);
        assert_eq!(m.tool_calls, 3);
        assert_eq!(m.verification_attempts, 3);
        assert_eq!(m.verification_failures, 1);

        // Failure branch preserved from frozen semantics.
        let verifier = ep
            .root
            .children
            .iter()
            .find(|c| c.trace.organ == "verifier")
            .expect("verifier branch exists");
        assert_eq!(verifier.trace.result, ResultType::Failure);
        let gate = verifier
            .children
            .iter()
            .find(|c| c.trace.operation == "verification_gate")
            .expect("verifier verification gate exists");
        assert!(gate.trace.verification().is_failed());

        // Timeline has one entry per span.
        assert_eq!(ep.timeline.len(), traces.len());

        // JSON payload shape.
        let json = model.to_json();
        assert!(json.contains("\"episodes\""));
        assert!(json.contains("\"global_metrics\""));
        assert!(json.contains("\"tree\""));
    }

    #[test]
    fn status_comes_from_root_not_children() {
        let traces = golden_episode();
        let model = ViewerModel::build(&traces);
        let ep = &model.episodes[0];
        // Root is Success even though the verifier child failed.
        assert_eq!(ep.status(), ResultType::Success);
        assert!(ep.verified());
    }

    #[test]
    fn detail_renders_failure_marker() {
        let traces = golden_episode();
        let model = ViewerModel::build(&traces);
        let ep = &model.episodes[0];
        let detail = model.render_detail(&ep.trace_id);
        // The failed verifier branch must be visible.
        assert!(detail.contains('✗'));
        assert!(detail.contains("verifier"));
    }

    #[test]
    fn timeline_shows_parallel_execution() {
        let traces = golden_episode();
        let model = ViewerModel::build(&traces);
        let ep = &model.episodes[0];
        let tl = model.render_timeline(&ep.trace_id);
        // Each organ that participated appears on its own row.
        assert!(tl.contains("researcher"));
        assert!(tl.contains("analyst"));
        assert!(tl.contains("verifier"));
        assert!(tl.contains("goal"));
    }

    #[test]
    fn span_detail_reveals_model_and_verification_fields() {
        let traces = golden_episode();

        let model_trace = traces
            .iter()
            .find(|t| t.operation == "llm_call")
            .expect("model span exists");
        let keys: Vec<String> = describe_span(model_trace).into_iter().map(|(k, _)| k).collect();
        assert!(keys.iter().any(|k| k == "Model"));
        assert!(keys.iter().any(|k| k == "Provider"));
        assert!(keys.iter().any(|k| k == "Tokens in"));
        assert!(keys.iter().any(|k| k == "Cost (USD)"));

        let vtrace = traces
            .iter()
            .find(|t| {
                t.operation == "verification_gate" && t.verification().is_failed()
            })
            .expect("failed verification gate exists");
        let keys: Vec<String> = describe_span(vtrace).into_iter().map(|(k, _)| k).collect();
        assert!(keys.iter().any(|k| k == "Verification status"));
        assert!(keys.iter().any(|k| k == "Verdict"));
        assert!(keys.iter().any(|k| k == "Evidence"));
    }

    #[test]
    fn explorer_lists_episodes() {
        let traces = golden_episode();
        let model = ViewerModel::build(&traces);
        let explorer = model.render_explorer();
        assert!(explorer.contains("Recent cognitive episodes"));
        assert!(explorer.contains("✓"));
    }
}
