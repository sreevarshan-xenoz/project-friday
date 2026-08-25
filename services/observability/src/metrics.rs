//! Phase 6C — backend-neutral semantic metrics over `CognitiveTrace`.
//!
//! `CognitiveMetrics` aggregates `CognitiveTrace` records into a flat, semantic
//! set of counters. It has **no OpenTelemetry dependency**: it is a pure
//! projection of the frozen contract, fanning out alongside the OTel adapter
//! (6A/6B) and the viewer (6D). The same `CognitiveTrace` stream can be fed to
//! all three.
//!
//! ```text
//!                CognitiveTrace
//!                /      |      \
//!               ▼       ▼       ▼
//!           OTel     Metrics   Viewer
//!            6A/6B      6C       6D
//! ```
//!
//! Semantic aggregation rules (so a `goal → planner → llm_call` episode is NOT
//! triple-counted):
//!
//! ```text
//! tokens / cost  -> ONLY spans carrying model_usage   (model calls)
//! tool count     -> ONLY `tool_call` spans
//! agent count    -> ONLY `agent_supervise` spans
//! verification   -> ONLY spans carrying verification_info
//! goals/outcome  -> ONLY `goal` spans (goal-centric success/failure)
//! ```
//!
//! `CognitiveTrace` carries no step-kind field (it is frozen — only `AgentStep`
//! has `StepKind`), so classification is derived from `model_usage`,
//! `verification_info`, and `operation`. FRIDAY subsystems should emit the
//! canonical operation names below for accurate metrics; anything else lands in
//! `Other` and does not perturb the semantic counters.

use std::sync::Mutex;

use friday_core::trace::{CognitiveTrace, ResultType, TraceCollector};
use serde::{Deserialize, Serialize};

/// The flat, backend-neutral semantic metrics model.
///
/// This is what backends serialize — see `CognitiveMetrics::to_json` and
/// `CognitiveMetrics::to_prometheus`. It is plain data: cloneable, serializable,
/// and equality-comparable for tests.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct CognitiveMetricsSnapshot {
    /// Goal spans recorded (every goal attempt).
    pub goals: u64,
    /// Goal spans that succeeded.
    pub successes: u64,
    /// Goal spans that failed.
    pub failures: u64,
    /// Verification spans that failed.
    pub verification_failures: u64,
    /// Sum of all recorded span durations (raw aggregate, may overlap in wall-clock).
    pub total_duration_ms: u64,
    /// Tokens-in summed over model-call spans only.
    pub total_tokens_in: u64,
    /// Tokens-out summed over model-call spans only.
    pub total_tokens_out: u64,
    /// USD cost summed over model-call spans only.
    pub total_cost_usd: f64,
    /// Model/llm_call spans recorded.
    pub model_calls: u64,
    /// Tool spans recorded.
    pub tool_calls: u64,
    /// Agent spans recorded.
    pub agent_runs: u64,
    /// Verification spans recorded.
    pub verification_attempts: u64,
}

/// Where a span lands, derived purely from frozen `CognitiveTrace` fields.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SpanClass {
    Goal,
    Agent,
    Tool,
    Model,
    Verification,
    Other,
}

fn classify(trace: &CognitiveTrace) -> SpanClass {
    // Key on the canonical operation name first. A goal span may also carry
    // `verification_info` (the goal-level verdict) without ceasing to be the
    // goal; a verification gate may carry model usage context without ceasing
    // to be a verification span. Operation is the primary discriminator.
    match trace.operation.as_str() {
        "goal" | "resolve" => SpanClass::Goal,
        "agent_supervise" | "execute" => SpanClass::Agent,
        "tool_call" => SpanClass::Tool,
        "verification_gate" | "verify" => SpanClass::Verification,
        _ => {
            // Fall back to payload-driven classification for non-canonical ops.
            if trace.model_usage.is_some() {
                SpanClass::Model
            } else if trace.verification_info.is_some() {
                SpanClass::Verification
            } else {
                SpanClass::Other
            }
        }
    }
}

/// Concurrent, backend-neutral accumulator over `CognitiveTrace`.
///
/// `record` is lock-protected (the simplest correct mechanism for FRIDAY's
/// expected parallelism — no lock-free science project). It is also a
/// `TraceCollector`, so it drops straight into any subsystem that already
/// accepts `Arc<dyn TraceCollector>`.
pub struct CognitiveMetrics {
    inner: Mutex<CognitiveMetricsSnapshot>,
}

impl Default for CognitiveMetrics {
    fn default() -> Self {
        Self::new()
    }
}

impl CognitiveMetrics {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(CognitiveMetricsSnapshot::default()),
        }
    }

    /// Fold one `CognitiveTrace` into the running metrics, applying the
    /// semantic rules so no counter is double-counted.
    pub fn record(&self, trace: &CognitiveTrace) {
        let mut m = self.inner.lock().expect("cognitive metrics mutex poisoned");
        m.total_duration_ms = m.total_duration_ms.saturating_add(trace.duration_ms);

        match classify(trace) {
            SpanClass::Goal => {
                m.goals += 1;
                match trace.result {
                    ResultType::Success => m.successes += 1,
                    ResultType::Failure => m.failures += 1,
                    _ => {}
                }
            }
            SpanClass::Agent => m.agent_runs += 1,
            SpanClass::Tool => m.tool_calls += 1,
            SpanClass::Model => {
                m.model_calls += 1;
                if let Some(u) = &trace.model_usage {
                    m.total_tokens_in = m.total_tokens_in.saturating_add(u.tokens_in.unwrap_or(0));
                    m.total_tokens_out =
                        m.total_tokens_out.saturating_add(u.tokens_out.unwrap_or(0));
                    if let Some(c) = u.cost_usd {
                        m.total_cost_usd += c;
                    }
                }
            }
            SpanClass::Verification => {
                m.verification_attempts += 1;
                if let Some(v) = &trace.verification_info {
                    if v.status.is_failed() {
                        m.verification_failures += 1;
                    }
                }
            }
            SpanClass::Other => {}
        }
    }

    /// A point-in-time, serializable copy of the counters.
    pub fn snapshot(&self) -> CognitiveMetricsSnapshot {
        self.inner
            .lock()
            .expect("cognitive metrics mutex poisoned")
            .clone()
    }

    /// JSON/API backend representation (backend-neutral core → JSON string).
    pub fn to_json(&self) -> String {
        serde_json::to_string(&*self.inner.lock().expect("poisoned"))
            .expect("cognitive metrics serialize to json")
    }

    /// Prometheus backend representation (exposition format). No `prometheus`
    /// crate required — the semantic model stays independent of any backend.
    pub fn to_prometheus(&self) -> String {
        let m = self.inner.lock().expect("poisoned");
        let rows = [
            ("friday_goals", m.goals as f64),
            ("friday_successes", m.successes as f64),
            ("friday_failures", m.failures as f64),
            (
                "friday_verification_failures",
                m.verification_failures as f64,
            ),
            ("friday_total_duration_ms", m.total_duration_ms as f64),
            ("friday_total_tokens_in", m.total_tokens_in as f64),
            ("friday_total_tokens_out", m.total_tokens_out as f64),
            ("friday_total_cost_usd", m.total_cost_usd),
            ("friday_model_calls", m.model_calls as f64),
            ("friday_tool_calls", m.tool_calls as f64),
            ("friday_agent_runs", m.agent_runs as f64),
            (
                "friday_verification_attempts",
                m.verification_attempts as f64,
            ),
        ];
        let mut out = String::new();
        for (name, value) in rows {
            out.push_str(&format!("# TYPE {} counter\n{} {}\n", name, name, value));
        }
        out
    }
}

impl TraceCollector for CognitiveMetrics {
    fn record(&self, trace: CognitiveTrace) {
        self.record(&trace);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use friday_core::trace::{
        GoalId, ModelUsage, SessionId, SpanId, VerificationInfo, VerificationStatus,
    };

    fn span(operation: &str, result: ResultType) -> CognitiveTrace {
        CognitiveTrace::new(
            SessionId::default(),
            GoalId::default(),
            SpanId::default(),
            "test",
            operation,
        )
        .with_result(result)
    }

    fn goal(result: ResultType) -> CognitiveTrace {
        span("goal", result)
    }

    fn model_call(tokens_in: u64, tokens_out: u64, cost: f64) -> CognitiveTrace {
        CognitiveTrace::new(
            SessionId::default(),
            GoalId::default(),
            SpanId::default(),
            "planner",
            "llm_call",
        )
        .with_model_usage(ModelUsage {
            model: "gpt-4o".into(),
            provider: "openai".into(),
            tokens_in: Some(tokens_in),
            tokens_out: Some(tokens_out),
            cached_tokens: None,
            reasoning_tokens: None,
            cost_usd: Some(cost),
            finish_reason: Some("stop".into()),
        })
        .with_result(ResultType::Success)
    }

    fn verify(status: VerificationStatus) -> CognitiveTrace {
        CognitiveTrace::new(
            SessionId::default(),
            GoalId::default(),
            SpanId::default(),
            "verification",
            "verify",
        )
        .with_verification_info(VerificationInfo {
            status,
            verifier: Some("checker".into()),
            evidence_count: Some(2),
            checks: None,
        })
    }

    #[test]
    fn success_aggregation_exact_counters() {
        let m = CognitiveMetrics::new();
        m.record(&goal(ResultType::Success));
        m.record(&goal(ResultType::Success));
        m.record(&goal(ResultType::Failure));
        for _ in 0..3 {
            m.record(&span("agent_supervise", ResultType::Success));
        }
        for _ in 0..5 {
            m.record(&span("tool_call", ResultType::Success));
        }
        for _ in 0..2 {
            m.record(&model_call(10, 10, 0.001));
        }

        let s = m.snapshot();
        assert_eq!(s.goals, 3);
        assert_eq!(s.successes, 2);
        assert_eq!(s.failures, 1);
        assert_eq!(s.agent_runs, 3);
        assert_eq!(s.tool_calls, 5);
        assert_eq!(s.model_calls, 2);
        // Tokens counted once per model span, not per episode.
        assert_eq!(s.total_tokens_in, 20);
        assert_eq!(s.total_tokens_out, 20);
    }

    #[test]
    fn cost_aggregation_sums_model_spans_only() {
        let m = CognitiveMetrics::new();
        m.record(&model_call(100, 50, 0.002));
        m.record(&model_call(200, 100, 0.004));

        let s = m.snapshot();
        assert_eq!(s.total_tokens_in, 300);
        assert_eq!(s.total_tokens_out, 150);
        assert!((s.total_cost_usd - 0.006).abs() < 1e-12);
        assert_eq!(s.model_calls, 2);

        // A model-less span must not move the cost counters.
        m.record(&goal(ResultType::Success));
        let s2 = m.snapshot();
        assert_eq!(s2.total_tokens_in, 300);
        assert!((s2.total_cost_usd - 0.006).abs() < 1e-12);
    }

    #[test]
    fn failure_semantics_isolate_verification_from_goals() {
        let m = CognitiveMetrics::new();
        // A goal that succeeded.
        m.record(&goal(ResultType::Success));
        // A verification gate that FAILED.
        m.record(&verify(VerificationStatus::Failed {
            reason: "evidence missing".into(),
        }));

        let s = m.snapshot();
        // Verification failure does NOT mark the goal as failed or as a success.
        assert_eq!(s.successes, 1);
        assert_eq!(s.failures, 0);
        // Verification counters reflect the gate outcome independently.
        assert_eq!(s.verification_attempts, 1);
        assert_eq!(s.verification_failures, 1);

        // Conversely, a failed goal with a PASSED verification: failure counts,
        // but it must not be attributed to verification failures.
        let m2 = CognitiveMetrics::new();
        m2.record(&goal(ResultType::Failure));
        m2.record(&verify(VerificationStatus::Passed));
        let s2 = m2.snapshot();
        assert_eq!(s2.failures, 1);
        assert_eq!(s2.successes, 0);
        assert_eq!(s2.verification_attempts, 1);
        assert_eq!(s2.verification_failures, 0);
    }

    #[test]
    fn no_double_counting_across_hierarchy() {
        // goal -> planner -> llm_call must count cost once (model span only).
        let m = CognitiveMetrics::new();
        m.record(&goal(ResultType::Success));
        m.record(&span("create_plan", ResultType::Success));
        m.record(&model_call(500, 250, 0.010));

        let s = m.snapshot();
        assert_eq!(s.goals, 1);
        assert_eq!(s.model_calls, 1);
        assert_eq!(s.total_tokens_in, 500);
        assert_eq!(s.total_tokens_out, 250);
        assert!((s.total_cost_usd - 0.010).abs() < 1e-12);
    }

    #[test]
    fn concurrency_has_no_lost_increments() {
        use std::sync::Arc;
        let m = Arc::new(CognitiveMetrics::new());
        let threads = 8u64;
        let per_thread = 1_000u64;

        let mut handles = Vec::new();
        for _ in 0..threads {
            let m = Arc::clone(&m);
            handles.push(std::thread::spawn(move || {
                for _ in 0..per_thread {
                    m.record(&goal(ResultType::Success));
                    m.record(&span("tool_call", ResultType::Success));
                    m.record(&model_call(1, 1, 0.001));
                }
            }));
        }
        for h in handles {
            h.join().expect("worker thread panicked");
        }

        let s = m.snapshot();
        assert_eq!(s.goals, threads * per_thread);
        assert_eq!(s.successes, threads * per_thread);
        assert_eq!(s.tool_calls, threads * per_thread);
        assert_eq!(s.model_calls, threads * per_thread);
        assert_eq!(s.total_tokens_in, threads * per_thread);
        assert_eq!(s.total_tokens_out, threads * per_thread);
        // Floating-point cost sum stays within tolerance.
        assert!((s.total_cost_usd - (threads * per_thread) as f64 * 0.001).abs() < 1e-6);
    }
}
