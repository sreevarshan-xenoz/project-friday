//! FRIDAY Cognitive Trace v1 → OpenTelemetry adapter (Phase 6A).
//!
//! This crate is an **export adapter**, not a new source of truth. The frozen
//! `friday_core::trace::CognitiveTrace` remains the canonical semantic model.
//! OpenTelemetry spans are produced *from* `CognitiveTrace` records via the
//! [`TraceCollector`] implementation [`OtelTraceCollector`]. Nothing here
//! changes the meaning of the frozen types.
//!
//! Mapping (see docs/architecture/cognitive-trace-v1.md):
//!
//! ```text
//! CognitiveTrace.trace_id        -> OTel trace_id
//! CognitiveTrace.span_id         -> OTel span_id   (128-bit Uuid -> low 8 bytes)
//! CognitiveTrace.parent_span_id  -> OTel parent (parent SpanContext placed in
//!                                   the export Context — a true parent-child
//!                                   edge, NOT a Link)
//! CognitiveTrace.operation        -> span name
//! CognitiveTrace.organ           -> attribute "friday.organ"
//! CognitiveTrace.result          -> status (Failure -> Error)
//! CognitiveTrace.model_usage     -> attributes (model/provider/tokens/cost)
//! CognitiveTrace.verification_info -> attributes
//! CognitiveTrace.started_at/duration_ms -> start/end timing
//! ```
//!
//! FRIDAY-specific information is carried as `friday.*` attributes; the
//! semantic model is never reshaped around OTel.

use std::borrow::Cow;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use friday_core::trace::{CognitiveTrace, ResultType, TraceCollector};
use opentelemetry::trace::{
    Span, SpanBuilder, SpanContext, SpanId, SpanKind, Status, TraceContextExt, TraceFlags, TraceId,
    TraceState, Tracer,
};
use opentelemetry::{Context, KeyValue};

/// Convert a FRIDAY 128-bit `TraceId` (Uuid) to an OTel 128-bit `TraceId`.
pub fn to_otel_trace_id(id: &friday_core::trace::TraceId) -> TraceId {
    TraceId::from_bytes(*id.0.as_bytes())
}

/// Convert a FRIDAY 128-bit `SpanId` (Uuid) to an OTel 64-bit `SpanId`.
///
/// OpenTelemetry span IDs are 64 bits; FRIDAY span IDs are 128-bit UUIDs.
/// We take the low 8 bytes. This is lossy but deterministic and sufficient
/// for an export adapter (OTel only needs locally-unique parent linkage).
pub fn to_otel_span_id(id: &friday_core::trace::SpanId) -> SpanId {
    let b = id.0.as_bytes();
    SpanId::from_bytes([b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]])
}

/// Map all strongly-typed `CognitiveTrace` fields to FRIDAY-namespaced
/// OTel attributes. Provider-absent fields stay `None` (never fabricated).
pub fn span_attributes(span: &CognitiveTrace) -> Vec<KeyValue> {
    let mut attrs: Vec<KeyValue> = vec![
        KeyValue::new("friday.organ", span.organ.clone()),
        KeyValue::new("friday.operation", span.operation.clone()),
        KeyValue::new("friday.session_id", span.session_id.to_string()),
        KeyValue::new("friday.goal_id", span.goal_id.to_string()),
        KeyValue::new("friday.trace_id", span.trace_id.to_string()),
        KeyValue::new("friday.span_id", span.span_id.to_string()),
        KeyValue::new("friday.result", span.result.to_string()),
        KeyValue::new("friday.trace_version", "1"),
    ];

    if let Some(c) = span.confidence {
        attrs.push(KeyValue::new("friday.confidence", c as f64));
    }
    if let Some(e) = &span.error {
        attrs.push(KeyValue::new("friday.error", e.clone()));
    }
    if let Some(v) = &span.verification_info {
        attrs.push(KeyValue::new(
            "friday.verification.status",
            format!("{:?}", v.status),
        ));
        if let Some(verifier) = &v.verifier {
            attrs.push(KeyValue::new(
                "friday.verification.verifier",
                verifier.clone(),
            ));
        }
        if let Some(n) = v.evidence_count {
            attrs.push(KeyValue::new(
                "friday.verification.evidence_count",
                n as i64,
            ));
        }
    }
    if let Some(m) = &span.model_usage {
        attrs.push(KeyValue::new("friday.model", m.model.clone()));
        attrs.push(KeyValue::new("friday.provider", m.provider.clone()));
        if let Some(t) = m.tokens_in {
            attrs.push(KeyValue::new("friday.tokens_in", t as i64));
        }
        if let Some(t) = m.tokens_out {
            attrs.push(KeyValue::new("friday.tokens_out", t as i64));
        }
        if let Some(t) = m.cached_tokens {
            attrs.push(KeyValue::new("friday.cached_tokens", t as i64));
        }
        if let Some(t) = m.reasoning_tokens {
            attrs.push(KeyValue::new("friday.reasoning_tokens", t as i64));
        }
        if let Some(c) = m.cost_usd {
            attrs.push(KeyValue::new("friday.cost_usd", c));
        }
        if let Some(f) = &m.finish_reason {
            attrs.push(KeyValue::new("friday.finish_reason", f.clone()));
        }
    }
    attrs
}

/// Map the `result` to an OTel status. Failure is Error; success is Ok;
/// pending/skipped are left Unset (never fabricated as success).
pub fn status_for(span: &CognitiveTrace) -> Status {
    match span.result {
        ResultType::Failure => Status::error(Cow::Owned(
            span.error.clone().unwrap_or_else(|| "failure".to_string()),
        )),
        ResultType::Success => Status::Ok,
        _ => Status::Unset,
    }
}

/// Build an OTel span from a `CognitiveTrace` and emit it through the given
/// tracer. The original FRIDAY IDs, parentage, timing, attributes, and status
/// are preserved — this is a projection, not a re-source.
pub fn export_span<T: Tracer>(tracer: &T, span: &CognitiveTrace) {
    let mut builder: SpanBuilder = tracer.span_builder("friday.cognitive.span");
    builder.name = Cow::Owned(span.operation.clone());
    builder.trace_id = Some(to_otel_trace_id(&span.trace_id));
    builder.span_id = Some(to_otel_span_id(&span.span_id));
    // Establish a true OTel parent-child relationship by placing the parent's
    // SpanContext into the export Context. This keeps FRIDAY's causal model
    // intact: the OTel parent is an *export representation* of FRIDAY's
    // semantic parent, never a reinterpretation of it. (OpenTelemetry 0.27 has
    // no `parent_span_id` builder field, so we use `with_remote_span_context`
    // rather than a Link — a Link is related context, not a parent.)
    let cx = match span.parent_span_id {
        Some(parent) => Context::current().with_remote_span_context(SpanContext::new(
            to_otel_trace_id(&span.trace_id),
            to_otel_span_id(&parent),
            TraceFlags::SAMPLED,
            true,
            TraceState::NONE,
        )),
        None => Context::current(),
    };
    builder.span_kind = Some(SpanKind::Internal);
    builder.attributes = Some(span_attributes(span));

    let start = SystemTime::from(span.started_at);
    let end = start + Duration::from_millis(span.duration_ms);
    builder.start_time = Some(start);
    builder.end_time = Some(end);
    builder.status = status_for(span);

    let mut otel_span = tracer.build_with_context(builder, &cx);
    otel_span.end_with_timestamp(end);
}

/// A [`TraceCollector`] that projects every recorded `CognitiveTrace` into an
/// OpenTelemetry tracer. Drop-in adapter: hand an `Arc<OtelTraceCollector<T>>`
/// to any subsystem that already accepts `Arc<dyn TraceCollector>`.
pub struct OtelTraceCollector<T: Tracer + Send + Sync> {
    tracer: Arc<T>,
}

impl<T: Tracer + Send + Sync> OtelTraceCollector<T> {
    pub fn new(tracer: Arc<T>) -> Self {
        Self { tracer }
    }
}

impl<T: Tracer + Send + Sync> TraceCollector for OtelTraceCollector<T> {
    fn record(&self, trace: CognitiveTrace) {
        export_span(&*self.tracer, &trace);
    }
}

pub mod metrics;
pub mod runtime;
pub mod viewer;

#[cfg(test)]
mod tests {
    use super::{
        export_span, span_attributes, status_for, to_otel_span_id, to_otel_trace_id,
        OtelTraceCollector,
    };
    use friday_core::trace::{
        CognitiveTrace, GoalId, ModelUsage, ResultType, SessionId, SpanId as FridaySpanId, TraceId,
        VerificationInfo, VerificationStatus,
    };
    use opentelemetry::trace::{noop::NoopTracer, SpanId as OtelSpanId, Status};
    use std::sync::Arc;

    fn sample() -> CognitiveTrace {
        let session = SessionId::default();
        let goal = GoalId::default();
        let trace_id = TraceId::default();
        let span = FridaySpanId::default();
        CognitiveTrace::new(session, goal, span, "researcher", "web.search")
            .with_trace_id(trace_id)
            .with_model_usage(ModelUsage {
                model: "gpt-4o".into(),
                provider: "openai".into(),
                tokens_in: Some(100),
                tokens_out: Some(20),
                cached_tokens: None,
                reasoning_tokens: None,
                cost_usd: Some(0.001),
                finish_reason: Some("stop".into()),
            })
            .with_verification_info(VerificationInfo {
                status: VerificationStatus::Passed,
                verifier: Some("verifier".into()),
                evidence_count: Some(3),
                checks: None,
            })
            .with_timing(chrono::Utc::now(), 50)
            .with_result(ResultType::Success)
    }

    #[test]
    fn maps_friday_attributes_and_status() {
        let t = sample();
        let attrs = span_attributes(&t);
        let keys: Vec<&str> = attrs.iter().map(|kv| kv.key.as_str()).collect();
        assert!(keys.contains(&"friday.organ"));
        assert!(keys.contains(&"friday.operation"));
        assert!(keys.contains(&"friday.model"));
        assert!(keys.contains(&"friday.provider"));
        assert!(keys.contains(&"friday.tokens_in"));
        assert!(keys.contains(&"friday.cost_usd"));
        assert!(keys.contains(&"friday.verification.status"));
        assert!(keys.contains(&"friday.verification.evidence_count"));
        assert_eq!(status_for(&t), Status::Ok);

        let failed = sample().with_result(ResultType::Failure).with_error("boom");
        assert!(
            matches!(status_for(&failed), Status::Error { .. }),
            "failure must map to OTel Error status"
        );

        // Pending stays Unset (never fabricated as success).
        let pending = sample().with_result(ResultType::Pending);
        assert_eq!(status_for(&pending), Status::Unset);
    }

    #[test]
    fn converts_ids_deterministically() {
        let t = sample();
        let otel_trace = to_otel_trace_id(&t.trace_id);
        assert_eq!(otel_trace, to_otel_trace_id(&t.trace_id));
        let b = t.span_id.0.as_bytes();
        let expected = OtelSpanId::from_bytes([b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]]);
        assert_eq!(to_otel_span_id(&t.span_id), expected);
    }

    #[test]
    fn exports_through_noop_tracer_without_panic() {
        // Direct projection through a tracer.
        export_span(&NoopTracer::new(), &sample());

        // Adapter used as a TraceCollector (the production integration point).
        let collector: Arc<dyn friday_core::trace::TraceCollector> =
            Arc::new(OtelTraceCollector::new(Arc::new(NoopTracer::new())));
        // Root + child: parentage must survive the projection.
        let child = sample().with_parent_span(Some(FridaySpanId::default()));
        collector.record(sample());
        collector.record(child);
    }
}
