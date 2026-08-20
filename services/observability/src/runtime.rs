//! Phase 6B — real OpenTelemetry runtime wiring.
//!
//! Builds a *real* `TracerProvider` with a non-blocking `BatchSpanProcessor`
//! draining into a `SpanExporter`. The OTLP exporter is the production sink and
//! is wired behind the `otlp` feature; tests use the SDK's in-memory exporter
//! so they stay deterministic and network-free.
//!
//! This module only *supplies a concrete `Tracer`* to the frozen-adapter
//! [`OtelTraceCollector`]. It does **not** touch `CognitiveTrace`,
//! `TraceContext`, or any frozen type.
//!
//! Export is non-blocking: `record()` enqueues a span and returns immediately.
//! A telemetry export failure is contained on the exporter's worker thread and
//! never propagates to FRIDAY cognition.

use std::sync::Arc;

use opentelemetry::trace::TracerProvider as _;
use opentelemetry_sdk::export::trace::SpanExporter;
use opentelemetry_sdk::trace::TracerProvider;
use opentelemetry_sdk::Resource;

use friday_core::trace::TraceCollector;
use opentelemetry::KeyValue;

use crate::OtelTraceCollector;

/// Build a `TracerProvider` with a `BatchSpanProcessor` draining into `exporter`.
///
/// The batch processor makes span export asynchronous and bounded: `record()`
/// never waits on the network. `exporter` is any `SpanExporter` — the
/// in-memory one for tests, or `OtlpSpanExporter` for production (see the `otlp`
/// feature).
pub fn build_batch_provider<E>(exporter: E, resource: Resource) -> TracerProvider
where
    E: SpanExporter + 'static,
{
    TracerProvider::builder()
        .with_batch_exporter(exporter, opentelemetry_sdk::runtime::Tokio)
        .with_resource(resource)
        .build()
}

/// FRIDAY's telemetry resource (service identity for the backend).
pub fn friday_resource() -> Resource {
    Resource::new(vec![KeyValue::new("service.name", "friday")])
}

/// Owns a `TracerProvider` and yields `TraceCollector`s bound to its tracer.
///
/// Dropping this (or calling [`OtelRuntime::shutdown`]) flushes and stops
/// telemetry. Constructing it is pure local setup — no live collector required.
pub struct OtelRuntime {
    provider: TracerProvider,
}

impl OtelRuntime {
    pub fn new(provider: TracerProvider) -> Self {
        Self { provider }
    }

    pub fn provider(&self) -> &TracerProvider {
        &self.provider
    }

    pub fn tracer(&self) -> opentelemetry_sdk::trace::Tracer {
        self.provider.tracer("friday")
    }

    /// A drop-in `Arc<dyn TraceCollector>` for any FRIDAY subsystem.
    pub fn collector(&self) -> Arc<dyn TraceCollector> {
        Arc::new(OtelTraceCollector::new(Arc::new(self.tracer())))
    }

    pub fn flush(&self) {
        let _ = self.provider.force_flush();
    }

    pub fn shutdown(&self) {
        let _ = self.provider.shutdown();
    }
}

/// Production OTLP wiring. Plugs the real gRPC sink into the same batch pipeline
/// the tests exercise. One line of glue; no frozen type is involved.
#[cfg(feature = "otlp")]
pub fn build_otlp_provider(
    endpoint: &str,
) -> Result<TracerProvider, opentelemetry::trace::TraceError> {
    use opentelemetry_otlp::WithExportConfig;
    let exporter = opentelemetry_otlp::SpanExporter::builder()
        .with_tonic()
        .with_endpoint(endpoint)
        .build()?;
    Ok(build_batch_provider(exporter, friday_resource()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use friday_core::trace::{
        CognitiveTrace, GoalId, ModelUsage, ResultType, SessionId, SpanId as FridaySpanId, TraceId,
    };
    use opentelemetry::trace::{SpanId as OtelSpanId, Status, TraceError};
    use opentelemetry_sdk::export::trace::SpanData;
    use opentelemetry_sdk::testing::trace::InMemorySpanExporter;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use std::time::Duration;

    fn sample(
        trace_id: TraceId,
        operation: &str,
        result: ResultType,
        parent: Option<FridaySpanId>,
    ) -> CognitiveTrace {
        let session = SessionId::default();
        let goal = GoalId::default();
        let span = FridaySpanId::default();
        let mut t = CognitiveTrace::new(session, goal, span, "researcher", operation.to_string())
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
            .with_timing(chrono::Utc::now(), 50)
            .with_result(result);
        if let Some(p) = parent {
            t = t.with_parent_span(Some(p));
        }
        t
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn provider_initialization() {
        // Real TracerProvider + BatchSpanProcessor construction, no network.
        let provider = build_batch_provider(InMemorySpanExporter::default(), friday_resource());
        let tracer = provider.tracer("friday");
        // The adapter must accept the SDK tracer without touching frozen types.
        let collector: Arc<dyn TraceCollector> =
            Arc::new(OtelTraceCollector::new(Arc::new(tracer)));
        assert!(Arc::strong_count(&collector) > 0);
        let _ = provider.force_flush();
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn export_preserves_span_fields() {
        let exporter = InMemorySpanExporter::default();
        let provider = build_batch_provider(exporter.clone(), friday_resource());
        let collector: Arc<dyn TraceCollector> =
            Arc::new(OtelTraceCollector::new(Arc::new(provider.tracer("friday"))));

        let episode = TraceId::default();
        let root = sample(episode, "plan", ResultType::Success, None);
        let root_span_id = root.span_id;
        let child = sample(episode, "web.search", ResultType::Success, Some(root_span_id));

        let expected_episode_trace = crate::to_otel_trace_id(&episode);
        let expected_root_span = crate::to_otel_span_id(&root_span_id);
        let expected_child_span = crate::to_otel_span_id(&child.span_id);
        let expected_parent = crate::to_otel_span_id(&root_span_id);
        let expected_start = std::time::SystemTime::from(root.started_at);
        let expected_end = expected_start + Duration::from_millis(root.duration_ms);

        collector.record(root);
        collector.record(child);
        provider.force_flush();

        let spans: Vec<SpanData> = exporter.get_finished_spans().unwrap();
        assert_eq!(spans.len(), 2, "both spans should be exported");

        let root_span = spans.iter().find(|s| s.name == "plan").expect("root exported");
        let child_span = spans
            .iter()
            .find(|s| s.name == "web.search")
            .expect("child exported");

        // Trace ID preserved & shared across the episode.
        assert_eq!(root_span.span_context.trace_id(), expected_episode_trace);
        assert_eq!(child_span.span_context.trace_id(), expected_episode_trace);
        assert_eq!(
            root_span.span_context.trace_id(),
            child_span.span_context.trace_id()
        );

        // Span IDs preserved.
        assert_eq!(root_span.span_context.span_id(), expected_root_span);
        assert_eq!(child_span.span_context.span_id(), expected_child_span);

        // Real OTel parent-child, not a Link.
        assert_eq!(root_span.parent_span_id, OtelSpanId::INVALID);
        assert_eq!(child_span.parent_span_id, expected_parent);

        // Operation -> span name.
        assert_eq!(root_span.name, "plan");
        assert_eq!(child_span.name, "web.search");

        // Status preserved.
        assert_eq!(root_span.status, Status::Ok);
        assert_eq!(child_span.status, Status::Ok);

        // Attributes preserved (FRIDAY-namespaced).
        let keys: Vec<&str> = root_span.attributes.iter().map(|kv| kv.key.as_str()).collect();
        assert!(keys.iter().any(|k| *k == "friday.model"));
        assert!(keys.iter().any(|k| *k == "friday.operation"));

        // Timestamps preserved exactly.
        assert_eq!(root_span.start_time, expected_start);
        assert_eq!(root_span.end_time, expected_end);

        let _ = provider.shutdown();
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn telemetry_failure_isolated_from_cognition() {
        // An exporter that always fails to export.
        #[derive(Debug)]
        struct FailingExporter;
        impl SpanExporter for FailingExporter {
            fn export(
                &mut self,
                _batch: Vec<SpanData>,
            ) -> std::pin::Pin<
                Box<
                    dyn std::future::Future<Output = Result<(), TraceError>>
                        + Send
                        + 'static,
                >,
            > {
                Box::pin(async move {
                    Err(TraceError::Other(Box::new(std::io::Error::new(
                        std::io::ErrorKind::Other,
                        "simulated telemetry export failure",
                    ))))
                })
            }
        }

        let provider = build_batch_provider(FailingExporter, friday_resource());
        let collector: Arc<dyn TraceCollector> =
            Arc::new(OtelTraceCollector::new(Arc::new(provider.tracer("friday"))));

        // FRIDAY cognition must proceed regardless of telemetry health.
        let cognition = Arc::new(AtomicUsize::new(0));
        cognition.fetch_add(1, Ordering::SeqCst); // cognitive work
        collector.record(sample(TraceId::default(), "plan", ResultType::Success, None)); // telemetry may fail
        cognition.fetch_add(1, Ordering::SeqCst); // continue despite failure

        assert_eq!(
            cognition.load(Ordering::SeqCst),
            2,
            "cognition must not be blocked by telemetry failure"
        );
        let _ = provider.force_flush();
    }
}
