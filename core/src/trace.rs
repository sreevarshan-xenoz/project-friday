//! ============================================================================
//! FRIDAY Cognitive Trace v1 — FROZEN (2026-08-19)
//! ----------------------------------------------------------------------------
//! The semantic meaning of every type in this module is LOCKED. Future
//! components implement this contract; they do not redefine it.
//! Enforced by: friday-agent-runtime/tests/director.rs::full_cognitive_episode_trace
//! Contract law: docs/architecture/cognitive-trace-v1.md
//! Do not change the meaning of: TraceId, SpanId, EventId, SessionId, GoalId,
//! TraceContext, CognitiveTraceContext, CognitiveTrace, CognitiveTracer,
//! TraceCollector, AgentStep, StepKind, ModelUsage.
//! ============================================================================
//!
//! FRIDAY Cognitive Trace — the canonical telemetry schema.
//!
//! Every cognitive operation gets a structured trace that can be queried,
//! aggregated, and visualized. This is NOT "distributed tracing" in the
//! microservice sense; it's **cognitive telemetry** — the data FRIDAY needs
//! to understand its own behavior over time.
//!
//! The identity model:
//!
//! ```text
//! Session (conversation lifetime)
//!   └── Goal (one user objective)
//!         └── Trace (one execution / cognitive episode)
//!               └── Span (one operation)
//!                     └── EventId (optional causality link)
//! ```
//!
//! This module lives in `friday-core` so both `friday-planner` and
//! `friday-cognitive` can depend on the same semantic schema without
//! creating a dependency cycle.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use uuid::Uuid;

// ── Identity Types ────────────────────────────────────────────────────

/// Unique identifier for one cognitive trace (a user goal's complete lifecycle).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TraceId(pub Uuid);

impl Default for TraceId {
    fn default() -> Self {
        Self(Uuid::new_v4())
    }
}

impl From<Uuid> for TraceId {
    fn from(uuid: Uuid) -> Self {
        Self(uuid)
    }
}

impl From<TraceId> for Uuid {
    fn from(id: TraceId) -> Self {
        id.0
    }
}

impl std::fmt::Display for TraceId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Span identifier (one operation within a trace).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SpanId(pub Uuid);

impl Default for SpanId {
    fn default() -> Self {
        Self(Uuid::new_v4())
    }
}

impl From<Uuid> for SpanId {
    fn from(uuid: Uuid) -> Self {
        Self(uuid)
    }
}

impl From<SpanId> for Uuid {
    fn from(id: SpanId) -> Self {
        id.0
    }
}

impl std::fmt::Display for SpanId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Event identifier (causality link between events).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct EventId(pub Uuid);

impl Default for EventId {
    fn default() -> Self {
        Self(Uuid::new_v4())
    }
}

impl From<Uuid> for EventId {
    fn from(uuid: Uuid) -> Self {
        Self(uuid)
    }
}

impl From<EventId> for Uuid {
    fn from(id: EventId) -> Self {
        id.0
    }
}

impl std::fmt::Display for EventId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// User session identifier (aggregates traces across goals).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SessionId(pub Uuid);

impl Default for SessionId {
    fn default() -> Self {
        Self(Uuid::new_v4())
    }
}

impl From<Uuid> for SessionId {
    fn from(uuid: Uuid) -> Self {
        Self(uuid)
    }
}

impl From<SessionId> for Uuid {
    fn from(id: SessionId) -> Self {
        id.0
    }
}

impl std::fmt::Display for SessionId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// User goal identifier (tracks one user request through FRIDAY).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct GoalId(pub Uuid);

impl Default for GoalId {
    fn default() -> Self {
        Self(Uuid::new_v4())
    }
}

impl From<Uuid> for GoalId {
    fn from(uuid: Uuid) -> Self {
        Self(uuid)
    }
}

impl From<GoalId> for Uuid {
    fn from(id: GoalId) -> Self {
        id.0
    }
}

impl std::fmt::Display for GoalId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

// ── Strong Type Definitions ──────────────────────────────────────────

/// Model usage information (strongly typed to avoid metadata junk drawer).
///
/// ```text
/// Model identity
/// ──────────────
///   model            e.g. "gpt-4o", "claude-sonnet-4-20250514"
///   provider         e.g. "openrouter", "openai", "ollama"
///
/// Model usage
/// ──────────────
///   tokens_in        input / prompt tokens
///   tokens_out       output / completion tokens
///   cached_tokens    tokens read from prompt cache (optional)
///   reasoning_tokens tokens used for chain-of-thought (optional)
///   cost_usd         total cost in USD (optional; computed by the backend)
///
/// Model result
/// ──────────────
///   finish_reason    why the model stopped ("stop", "length", etc.)
/// ```
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModelUsage {
    pub model: String,
    pub provider: String,
    pub tokens_in: Option<u64>,
    pub tokens_out: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cached_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_tokens: Option<u64>,
    pub cost_usd: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finish_reason: Option<String>,
}

/// Verification information (strongly typed).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VerificationInfo {
    pub status: VerificationStatus,
    pub verifier: Option<String>,
    pub evidence_count: Option<u32>,
    pub checks: Option<serde_json::Value>,
}

// ── Result & Verification Types ──────────────────────────────────────

/// The outcome of a cognitive operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ResultType {
    Success,
    Failure,
    Pending,
    Skipped,
}

impl std::fmt::Display for ResultType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ResultType::Success => write!(f, "success"),
            ResultType::Failure => write!(f, "failure"),
            ResultType::Pending => write!(f, "pending"),
            ResultType::Skipped => write!(f, "skipped"),
        }
    }
}

/// Whether this operation was verified, and how.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "type", content = "value")]
pub enum VerificationStatus {
    /// Not verified (not applicable or not yet verified).
    None,
    /// Passed verification.
    Passed,
    /// Failed verification with optional reason.
    Failed { reason: String },
    /// Verification is in progress.
    InProgress,
}

impl VerificationStatus {
    pub fn is_passed(&self) -> bool {
        matches!(self, VerificationStatus::Passed)
    }

    pub fn is_failed(&self) -> bool {
        matches!(self, VerificationStatus::Failed { .. })
    }
}

impl std::fmt::Display for VerificationStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            VerificationStatus::None => write!(f, "none"),
            VerificationStatus::Passed => write!(f, "passed"),
            VerificationStatus::Failed { reason } => write!(f, "failed: {}", reason),
            VerificationStatus::InProgress => write!(f, "in_progress"),
        }
    }
}

// ── Cognitive Trace Entry ────────────────────────────────────────────

/// One cognitive operation's telemetry record.
///
/// ```text
/// Trace (root)
///   ├─ trace_id: GOAL-001 (same for all operations in one goal)
///   │
///   └─ Spans (operations within the trace)
///        ├─ span_id: SPAN-01 (this operation)
///        ├─ parent_span_id: None (root has no parent)
///        │
///        └─ Child span
///             ├─ span_id: SPAN-02
///             ├─ parent_span_id: SPAN-01
///             └─ ...
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CognitiveTrace {
    /// Trace identifier (same for all operations in one user goal).
    pub trace_id: TraceId,
    /// Session identifier (aggregates multiple goals).
    pub session_id: SessionId,
    /// Goal identifier (one user request).
    pub goal_id: GoalId,
    /// Span identifier (THIS operation within the trace).
    pub span_id: SpanId,
    /// Parent span identifier (cognitive hierarchy).
    /// `None` for root spans.
    pub parent_span_id: Option<SpanId>,
    /// Parent event identifier (event causality).
    /// `None` means no parent event — absence means no parent, not "zero ID".
    pub parent_event_id: Option<EventId>,
    /// The organ that performed this operation (see `friday_cognitive::Organ`).
    pub organ: String,
    /// The specific operation name (e.g., "search", "plan", "verify").
    pub operation: String,
    /// Model usage (strongly typed, avoids metadata junk drawer).
    pub model_usage: Option<ModelUsage>,
    /// Verification info (strongly typed).
    pub verification_info: Option<VerificationInfo>,
    /// When this operation started.
    pub started_at: DateTime<Utc>,
    /// How long it took (milliseconds).
    pub duration_ms: u64,
    /// Result type.
    pub result: ResultType,
    /// Confidence score [0.0, 1.0] (if applicable).
    pub confidence: Option<f32>,
    /// Error message (if failure).
    pub error: Option<String>,
    /// Metadata (reserved for genuinely operation-specific information).
    pub metadata: serde_json::Value,
}

impl CognitiveTrace {
    /// Create a new trace entry.
    pub fn new(
        session_id: SessionId,
        goal_id: GoalId,
        span_id: SpanId,
        organ: impl Into<String>,
        operation: impl Into<String>,
    ) -> Self {
        Self {
            trace_id: TraceId::default(),
            session_id,
            goal_id,
            span_id,
            parent_span_id: None,
            parent_event_id: None,
            organ: organ.into(),
            operation: operation.into(),
            model_usage: None,
            verification_info: None,
            started_at: Utc::now(),
            duration_ms: 0,
            result: ResultType::Pending,
            confidence: None,
            error: None,
            metadata: serde_json::Value::Object(serde_json::Map::new()),
        }
    }

    /// Set the trace identifier (same for all operations in one goal).
    pub fn with_trace_id(mut self, trace_id: TraceId) -> Self {
        self.trace_id = trace_id;
        self
    }

    /// Set the parent event identifier (event causality).
    pub fn with_parent_event(mut self, parent_event_id: Option<EventId>) -> Self {
        self.parent_event_id = parent_event_id;
        self
    }

    /// Set the parent span identifier (cognitive hierarchy).
    /// Use `None` for root spans.
    pub fn with_parent_span(mut self, parent_span_id: Option<SpanId>) -> Self {
        self.parent_span_id = parent_span_id;
        self
    }

    /// Set model usage (replaces the entire ModelUsage).
    pub fn with_model_usage(mut self, model_usage: ModelUsage) -> Self {
        self.model_usage = Some(model_usage);
        self
    }

    /// Set model/provider info with token counts and cost.
    pub fn with_model(mut self, model: impl Into<String>, provider: impl Into<String>) -> Self {
        let existing = self.model_usage.take();
        self.model_usage = Some(ModelUsage {
            model: model.into(),
            provider: provider.into(),
            tokens_in: existing.as_ref().and_then(|m| m.tokens_in),
            tokens_out: existing.as_ref().and_then(|m| m.tokens_out),
            cached_tokens: existing.as_ref().and_then(|m| m.cached_tokens),
            reasoning_tokens: existing.as_ref().and_then(|m| m.reasoning_tokens),
            cost_usd: existing.as_ref().and_then(|m| m.cost_usd),
            finish_reason: existing.as_ref().and_then(|m| m.finish_reason.clone()),
        });
        self
    }

    /// Set verification info.
    pub fn with_verification_info(mut self, verification_info: VerificationInfo) -> Self {
        self.verification_info = Some(verification_info);
        self
    }

    /// Set timing.
    pub fn with_timing(mut self, started_at: DateTime<Utc>, duration_ms: u64) -> Self {
        self.started_at = started_at;
        self.duration_ms = duration_ms;
        self
    }

    /// Set token counts and cost (creates ModelUsage if needed).
    pub fn with_tokens(mut self, tokens_in: u64, tokens_out: u64, cost_usd: f64) -> Self {
        let existing = self.model_usage.take();
        self.model_usage = Some(ModelUsage {
            model: existing
                .as_ref()
                .map(|m| m.model.clone())
                .unwrap_or_default(),
            provider: existing
                .as_ref()
                .map(|m| m.provider.clone())
                .unwrap_or_default(),
            tokens_in: Some(tokens_in),
            tokens_out: Some(tokens_out),
            cached_tokens: existing.as_ref().and_then(|m| m.cached_tokens),
            reasoning_tokens: existing.as_ref().and_then(|m| m.reasoning_tokens),
            cost_usd: Some(cost_usd),
            finish_reason: existing.as_ref().and_then(|m| m.finish_reason.clone()),
        });
        self
    }

    /// Set result.
    pub fn with_result(mut self, result: ResultType) -> Self {
        self.result = result;
        self
    }

    /// Set verification (creates VerificationInfo).
    pub fn with_verification(mut self, status: VerificationStatus) -> Self {
        self.verification_info = Some(VerificationInfo {
            status,
            verifier: None,
            evidence_count: None,
            checks: None,
        });
        self
    }

    /// Set verification with verifier and evidence.
    pub fn with_verification_full(
        mut self,
        verifier: impl Into<String>,
        evidence_count: u32,
        status: VerificationStatus,
        checks: serde_json::Value,
    ) -> Self {
        self.verification_info = Some(VerificationInfo {
            status,
            verifier: Some(verifier.into()),
            evidence_count: Some(evidence_count),
            checks: Some(checks),
        });
        self
    }

    /// Set confidence.
    pub fn with_confidence(mut self, confidence: f32) -> Self {
        self.confidence = Some(confidence.clamp(0.0, 1.0));
        self
    }

    /// Set error.
    pub fn with_error(mut self, error: impl Into<String>) -> Self {
        self.error = Some(error.into());
        self
    }

    /// Set metadata.
    pub fn with_metadata(mut self, metadata: serde_json::Value) -> Self {
        self.metadata = metadata;
        self
    }

    // ── Accessors for convenience ──

    /// Get the model name, if set.
    pub fn model(&self) -> Option<&str> {
        self.model_usage.as_ref().map(|m| m.model.as_str())
    }

    /// Get the provider name, if set.
    pub fn provider(&self) -> Option<&str> {
        self.model_usage.as_ref().map(|m| m.provider.as_str())
    }

    /// Get tokens_in, if set.
    pub fn tokens_in(&self) -> Option<u64> {
        self.model_usage.as_ref().and_then(|m| m.tokens_in)
    }

    /// Get tokens_out, if set.
    pub fn tokens_out(&self) -> Option<u64> {
        self.model_usage.as_ref().and_then(|m| m.tokens_out)
    }

    /// Get cost_usd, if set.
    pub fn cost_usd(&self) -> Option<f64> {
        self.model_usage.as_ref().and_then(|m| m.cost_usd)
    }

    /// Get cached_tokens, if set.
    pub fn cached_tokens(&self) -> Option<u64> {
        self.model_usage.as_ref().and_then(|m| m.cached_tokens)
    }

    /// Get reasoning_tokens, if set.
    pub fn reasoning_tokens(&self) -> Option<u64> {
        self.model_usage.as_ref().and_then(|m| m.reasoning_tokens)
    }

    /// Get finish_reason, if set.
    pub fn finish_reason(&self) -> Option<&str> {
        self.model_usage
            .as_ref()
            .and_then(|m| m.finish_reason.as_deref())
    }

    /// Set cached token count.
    pub fn with_cached_tokens(mut self, cached_tokens: u64) -> Self {
        if let Some(ref mut usage) = self.model_usage {
            usage.cached_tokens = Some(cached_tokens);
        } else {
            self.model_usage = Some(ModelUsage {
                model: String::new(),
                provider: String::new(),
                tokens_in: None,
                tokens_out: None,
                cached_tokens: Some(cached_tokens),
                reasoning_tokens: None,
                cost_usd: None,
                finish_reason: None,
            });
        }
        self
    }

    /// Set reasoning token count.
    pub fn with_reasoning_tokens(mut self, reasoning_tokens: u64) -> Self {
        if let Some(ref mut usage) = self.model_usage {
            usage.reasoning_tokens = Some(reasoning_tokens);
        } else {
            self.model_usage = Some(ModelUsage {
                model: String::new(),
                provider: String::new(),
                tokens_in: None,
                tokens_out: None,
                cached_tokens: None,
                reasoning_tokens: Some(reasoning_tokens),
                cost_usd: None,
                finish_reason: None,
            });
        }
        self
    }

    /// Set finish reason.
    pub fn with_finish_reason(mut self, finish_reason: impl Into<String>) -> Self {
        if let Some(ref mut usage) = self.model_usage {
            usage.finish_reason = Some(finish_reason.into());
        } else {
            self.model_usage = Some(ModelUsage {
                model: String::new(),
                provider: String::new(),
                tokens_in: None,
                tokens_out: None,
                cached_tokens: None,
                reasoning_tokens: None,
                cost_usd: None,
                finish_reason: Some(finish_reason.into()),
            });
        }
        self
    }

    /// Get the verification status, if set.
    pub fn verification(&self) -> VerificationStatus {
        self.verification_info
            .as_ref()
            .map(|v| v.status.clone())
            .unwrap_or(VerificationStatus::None)
    }
}

// ── CognitiveTraceContext ────────────────────────────────────────────

/// Context for hierarchical tracing. Holds the current trace identifiers and
/// provides span creation for parent/child relationships.
#[derive(Debug, Clone)]
pub struct CognitiveTraceContext {
    /// Trace identifier (same for all operations in one user goal).
    pub trace_id: TraceId,
    /// Session identifier (aggregates multiple goals).
    pub session_id: SessionId,
    /// Goal identifier (one user request).
    pub goal_id: GoalId,
    /// Current parent span (for child spans). `None` for root spans.
    pub parent_span_id: Option<SpanId>,
}

impl CognitiveTraceContext {
    /// Create a new context for a goal (root span).
    pub fn for_goal(trace_id: TraceId, session_id: SessionId, goal_id: GoalId) -> Self {
        Self {
            trace_id,
            session_id,
            goal_id,
            parent_span_id: None,
        }
    }

    /// Create a child span context.
    pub fn child(&self, parent_span_id: SpanId) -> Self {
        Self {
            trace_id: self.trace_id,
            session_id: self.session_id,
            goal_id: self.goal_id,
            parent_span_id: Some(parent_span_id),
        }
    }

    /// Create a new trace from context.
    pub fn trace(
        &self,
        span_id: SpanId,
        organ: impl Into<String>,
        operation: impl Into<String>,
    ) -> CognitiveTrace {
        let mut trace =
            CognitiveTrace::new(self.session_id, self.goal_id, span_id, organ, operation)
                .with_trace_id(self.trace_id);
        if let Some(parent) = self.parent_span_id {
            trace = trace.with_parent_span(Some(parent));
        }
        trace
    }
}

// ── TraceContext (boundary propagation payload) ───────────────────────
//
// `CognitiveTraceContext` is the *live* span factory used within a single
// module. `TraceContext` is the *serializable boundary token* that carries
// cognitive identity across an asynchronous seam — the event bus, a task
// handoff, a cross-module dispatch. It is deliberately a plain value (no
// behaviour) so it can be embedded in events or passed through channels.
//
// Critically it carries `parent_span_id`, so the receiving module can
// materialize a *child* `CognitiveTraceContext` (not merely inherit a trace
// id) and preserve causal hierarchy:
//
// ```text
// Planner span
//    │  TraceContext { parent_span_id: Some(planner_span) }
//    ▼
// Director span (parent_span_id == planner_span)
// ```
//
// Threading only `trace_id` would recreate two correlated-but-separate trace
// trees. Threading the full `TraceContext` (with `parent_span_id`) yields one
// causal episode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct TraceContext {
    /// Same for every operation in one user goal.
    pub trace_id: TraceId,
    /// The upstream span this context should attach under. `None` for roots.
    pub parent_span_id: Option<SpanId>,
    /// Aggregates multiple goals.
    pub session_id: SessionId,
    /// One user request.
    pub goal_id: GoalId,
}

impl TraceContext {
    /// Build a propagation payload from a live context. The `parent_span_id`
    /// is taken verbatim so the receiver attaches as a child of whatever span
    /// the sender last recorded under.
    pub fn from_context(ctx: &CognitiveTraceContext) -> Self {
        Self {
            trace_id: ctx.trace_id,
            parent_span_id: ctx.parent_span_id,
            session_id: ctx.session_id,
            goal_id: ctx.goal_id,
        }
    }

    /// Materialize into a live [`CognitiveTraceContext`] carrying the same
    /// identity (same trace, same parent span) so the receiver can create
    /// child spans under it.
    pub fn into_context(self) -> CognitiveTraceContext {
        CognitiveTraceContext {
            trace_id: self.trace_id,
            session_id: self.session_id,
            goal_id: self.goal_id,
            parent_span_id: self.parent_span_id,
        }
    }
}

impl From<&CognitiveTraceContext> for TraceContext {
    fn from(ctx: &CognitiveTraceContext) -> Self {
        Self::from_context(ctx)
    }
}

impl From<TraceContext> for CognitiveTraceContext {
    fn from(payload: TraceContext) -> Self {
        payload.into_context()
    }
}

// ── Trace Collector ──────────────────────────────────────────────────

/// A collector that receives cognitive traces for aggregation/telemetry.
///
/// Uses `&self` for concurrency safety; implementations use internal
/// synchronization (Mutex, channel, etc.).
pub trait TraceCollector: Send + Sync {
    /// Record a cognitive trace.
    fn record(&self, trace: CognitiveTrace);
}

/// A no-op collector (for when telemetry is disabled).
pub struct NoopTraceCollector;

impl TraceCollector for NoopTraceCollector {
    fn record(&self, _trace: CognitiveTrace) {
        // Do nothing
    }
}

/// An in-memory collector for testing.
#[derive(Clone)]
pub struct InMemoryTraceCollector {
    traces: Arc<std::sync::Mutex<Vec<CognitiveTrace>>>,
}

impl InMemoryTraceCollector {
    pub fn new() -> Self {
        Self {
            traces: Arc::new(std::sync::Mutex::new(Vec::new())),
        }
    }

    pub fn traces(&self) -> std::sync::MutexGuard<'_, Vec<CognitiveTrace>> {
        self.traces.lock().expect("trace collector poisoned")
    }

    /// Consume the collector and return all collected traces.
    pub fn into_traces(self) -> Vec<CognitiveTrace> {
        Arc::try_unwrap(self.traces)
            .expect("collector still borrowed")
            .into_inner()
            .expect("trace collector poisoned")
    }
}

impl Default for InMemoryTraceCollector {
    fn default() -> Self {
        Self::new()
    }
}

impl TraceCollector for InMemoryTraceCollector {
    fn record(&self, trace: CognitiveTrace) {
        self.traces
            .lock()
            .expect("trace collector poisoned")
            .push(trace);
    }
}

// ── Cognitive Tracer ─────────────────────────────────────────────────

/// A lightweight instrumentation interface for emitting cognitive traces.
///
/// This trait lets subsystems (planner, agents, verifier, etc.) record
/// spans without depending on the full cognitive runtime. The trait is
/// object-safe and typically implemented with a `TraceCollector` wrapper
/// or a concrete type that holds an `Arc<dyn TraceCollector>`.
pub trait CognitiveTracer: Send + Sync {
    /// Record a completed span.
    fn record(&self, trace: CognitiveTrace);

    /// Start a new span, returning its ID.
    /// The caller is responsible for finishing the span.
    fn start_span(&self, ctx: &CognitiveTraceContext, organ: &str, operation: &str) -> SpanId {
        let span_id = SpanId::default();
        let trace = ctx.trace(span_id, organ, operation);
        self.record(trace);
        span_id
    }

    /// Finish a span with updated timing and result.
    fn finish_span(
        &self,
        ctx: &CognitiveTraceContext,
        span_id: SpanId,
        organ: &str,
        operation: &str,
        duration_ms: u64,
        result: ResultType,
    ) {
        let trace = ctx
            .trace(span_id, organ, operation)
            .with_timing(
                Utc::now() - chrono::Duration::milliseconds(duration_ms as i64),
                duration_ms,
            )
            .with_result(result);
        self.record(trace);
    }
}

/// Wrapper that implements `CognitiveTracer` for any `TraceCollector`.
/// Use this to adapt a `TraceCollector` into a `CognitiveTracer`.
pub struct TraceCollectorTracer<C: TraceCollector>(pub C);

impl<C: TraceCollector> CognitiveTracer for TraceCollectorTracer<C> {
    fn record(&self, trace: CognitiveTrace) {
        self.0.record(trace);
    }
}

// ── Trace Hierarchy Reconstruction ──────────────────────────────────

/// A trace tree node for reconstructing the hierarchy from flat records.
#[derive(Debug, Clone)]
pub struct TraceTreeNode {
    pub trace: CognitiveTrace,
    pub children: Vec<TraceTreeNode>,
}

/// Reconstruct a trace hierarchy from a flat list of traces.
///
/// Returns the root spans (parent_span_id = None) with their full
/// descendant trees. Panics if a child references a nonexistent parent.
pub fn reconstruct_trace_tree(traces: &[CognitiveTrace]) -> Vec<TraceTreeNode> {
    use std::collections::HashMap;

    // Index by span_id for O(1) lookup.
    let mut by_span_id: HashMap<SpanId, usize> = HashMap::new();
    for (i, t) in traces.iter().enumerate() {
        by_span_id.insert(t.span_id, i);
    }

    // Build children map.
    let mut children_of: HashMap<SpanId, Vec<usize>> = HashMap::new();
    let mut roots = Vec::new();

    for (i, t) in traces.iter().enumerate() {
        if let Some(parent_id) = t.parent_span_id {
            children_of.entry(parent_id).or_default().push(i);
        } else {
            roots.push(i);
        }
    }

    // Recursively build trees.
    fn build_tree(
        idx: usize,
        traces: &[CognitiveTrace],
        children_of: &HashMap<SpanId, Vec<usize>>,
        _by_span_id: &HashMap<SpanId, usize>,
    ) -> TraceTreeNode {
        let trace = traces[idx].clone();
        let children = children_of
            .get(&trace.span_id)
            .map(|child_indices| {
                child_indices
                    .iter()
                    .map(|&ci| build_tree(ci, traces, children_of, _by_span_id))
                    .collect()
            })
            .unwrap_or_default();
        TraceTreeNode { trace, children }
    }

    roots
        .iter()
        .map(|&ri| build_tree(ri, traces, &children_of, &by_span_id))
        .collect()
}

/// Validate that a set of traces forms a valid hierarchy:
/// - all spans have the same trace_id
/// - every child has a valid parent_span_id pointing to an existing span
/// - root spans have no parent
/// - no cycles
pub fn validate_trace_hierarchy(traces: &[CognitiveTrace]) -> Result<(), String> {
    use std::collections::{HashMap, HashSet};

    if traces.is_empty() {
        return Ok(());
    }

    let trace_id = traces[0].trace_id;
    let mut by_span_id: HashMap<SpanId, &CognitiveTrace> = HashMap::new();

    for t in traces {
        if t.trace_id != trace_id {
            return Err(format!(
                "mixed trace_ids: expected {} got {}",
                trace_id, t.trace_id
            ));
        }
        if by_span_id.contains_key(&t.span_id) {
            return Err(format!("duplicate span_id: {}", t.span_id));
        }
        by_span_id.insert(t.span_id, t);
    }

    // Check parent references.
    let mut visited = HashSet::new();
    for t in traces {
        if let Some(parent_id) = t.parent_span_id {
            if !by_span_id.contains_key(&parent_id) {
                return Err(format!(
                    "span {} references nonexistent parent {}",
                    t.span_id, parent_id
                ));
            }
            // Cycle detection via traversal.
            let mut current = Some(parent_id);
            let mut path = HashSet::new();
            path.insert(t.span_id);
            while let Some(id) = current {
                if !path.insert(id) {
                    return Err(format!("cycle detected involving span {}", id));
                }
                if let Some(parent_trace) = by_span_id.get(&id) {
                    current = parent_trace.parent_span_id;
                } else {
                    break;
                }
            }
        }
        visited.insert(t.span_id);
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_types_are_distinct() {
        let trace_id = TraceId::default();
        let span_id = SpanId::default();
        let event_id = EventId::default();
        let session_id = SessionId::default();
        let goal_id = GoalId::default();

        // They wrap the same Uuid type but are different newtypes.
        assert_ne!(format!("{}", trace_id), format!("{}", span_id));
        assert_ne!(format!("{}", event_id), format!("{}", session_id));
        assert_ne!(format!("{}", session_id), format!("{}", goal_id));
    }

    #[test]
    fn trace_structure() {
        let trace = CognitiveTrace::new(
            SessionId::default(),
            GoalId::default(),
            SpanId::default(),
            "planner",
            "plan",
        )
        .with_model("openrouter/gpt-4o", "openrouter")
        .with_tokens(100, 20, 0.003)
        .with_result(ResultType::Success)
        .with_verification(VerificationStatus::Passed)
        .with_confidence(0.95);

        assert_eq!(trace.organ, "planner");
        assert_eq!(trace.operation, "plan");
        assert_eq!(trace.model(), Some("openrouter/gpt-4o"));
        assert_eq!(trace.provider(), Some("openrouter"));
        assert_eq!(trace.tokens_in(), Some(100));
        assert_eq!(trace.tokens_out(), Some(20));
        assert_eq!(trace.cost_usd(), Some(0.003));
        assert_eq!(trace.result, ResultType::Success);
        assert!(trace.verification().is_passed());
        assert_eq!(trace.confidence, Some(0.95));
    }

    #[test]
    fn trace_display() {
        assert_eq!(ResultType::Success.to_string(), "success");
        assert_eq!(ResultType::Failure.to_string(), "failure");
        assert_eq!(VerificationStatus::Passed.to_string(), "passed");
        assert_eq!(
            VerificationStatus::Failed {
                reason: "timeout".to_string()
            }
            .to_string(),
            "failed: timeout"
        );
    }

    #[test]
    fn parent_absence_means_none() {
        let trace = CognitiveTrace::new(
            SessionId::default(),
            GoalId::default(),
            SpanId::default(),
            "planner",
            "plan",
        );
        assert!(trace.parent_span_id.is_none());
        assert!(trace.parent_event_id.is_none());
    }

    #[test]
    fn hierarchy_reconstruction() {
        let root_span = SpanId::default();
        let child_a = SpanId::default();
        let child_b = SpanId::default();

        let session = SessionId::default();
        let goal = GoalId::default();
        let trace_id = TraceId::default();

        let root = CognitiveTrace::new(session, goal, root_span, "goal", "resolve")
            .with_trace_id(trace_id);
        let a = CognitiveTrace::new(session, goal, child_a, "planner", "plan")
            .with_trace_id(trace_id)
            .with_parent_span(Some(root_span));
        let b = CognitiveTrace::new(session, goal, child_b, "agent", "execute")
            .with_trace_id(trace_id)
            .with_parent_span(Some(root_span));

        let trees = reconstruct_trace_tree(&[root, a, b]);
        assert_eq!(trees.len(), 1);
        assert_eq!(trees[0].children.len(), 2);
        assert_eq!(trees[0].trace.organ, "goal");
    }

    #[test]
    fn validate_valid_hierarchy() {
        let root_span = SpanId::default();
        let child = SpanId::default();
        let session = SessionId::default();
        let goal = GoalId::default();
        let trace_id = TraceId::default();

        let root = CognitiveTrace::new(session, goal, root_span, "goal", "resolve")
            .with_trace_id(trace_id);
        let child_trace = CognitiveTrace::new(session, goal, child, "planner", "plan")
            .with_trace_id(trace_id)
            .with_parent_span(Some(root_span));

        assert!(validate_trace_hierarchy(&[root, child_trace]).is_ok());
    }

    #[test]
    fn validate_detects_bogus_parent() {
        let root_span = SpanId::default();
        let child = SpanId::default();
        let session = SessionId::default();
        let goal = GoalId::default();
        let trace_id = TraceId::default();

        let root = CognitiveTrace::new(session, goal, root_span, "goal", "resolve")
            .with_trace_id(trace_id);
        let child_trace = CognitiveTrace::new(session, goal, child, "planner", "plan")
            .with_trace_id(trace_id)
            .with_parent_span(Some(SpanId::default())); // bogus parent

        let result = validate_trace_hierarchy(&[root, child_trace]);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("nonexistent parent"));
    }

    #[test]
    fn validate_detects_mixed_trace_ids() {
        let session = SessionId::default();
        let goal = GoalId::default();
        let s1 = SpanId::default();
        let s2 = SpanId::default();

        let t1 =
            CognitiveTrace::new(session, goal, s1, "a", "op1").with_trace_id(TraceId::default());
        let t2 =
            CognitiveTrace::new(session, goal, s2, "b", "op2").with_trace_id(TraceId::default());

        let result = validate_trace_hierarchy(&[t1, t2]);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("mixed trace_ids"));
    }

    #[test]
    fn collector_is_concurrency_safe() {
        let collector = InMemoryTraceCollector::new();
        let trace = CognitiveTrace::new(
            SessionId::default(),
            GoalId::default(),
            SpanId::default(),
            "test",
            "op",
        );
        TraceCollector::record(&collector, trace);
        assert_eq!(collector.traces().len(), 1);
    }

    #[test]
    fn noop_collector_does_not_panic() {
        let collector = NoopTraceCollector;
        let trace = CognitiveTrace::new(
            SessionId::default(),
            GoalId::default(),
            SpanId::default(),
            "test",
            "op",
        );
        TraceCollector::record(&collector, trace);
    }

    #[test]
    fn cognitive_trace_context_creates_hierarchy() {
        let session = SessionId::default();
        let goal = GoalId::default();
        let trace_id = TraceId::default();

        let root_ctx = CognitiveTraceContext::for_goal(trace_id, session, goal);
        let root_span = SpanId::default();
        let root_trace = root_ctx.trace(root_span, "goal", "resolve");
        assert!(root_trace.parent_span_id.is_none());

        let child_ctx = root_ctx.child(root_span);
        let child_span = SpanId::default();
        let child_trace = child_ctx.trace(child_span, "planner", "plan");
        assert_eq!(child_trace.parent_span_id, Some(root_span));
        assert_eq!(child_trace.trace_id, trace_id);
    }

    // ── End-to-End Cognitive Episode Test ─────────────────────────────
    //
    // This test exercises the full cognitive trace lifecycle:
    //   Goal → Intent → Planner → Model → Agent → Verification
    //
    // It verifies:
    //   1. All spans share the same trace_id
    //   2. Every child has a valid parent_span_id
    //   3. Root spans have no parent
    //   4. No parent points to a nonexistent span
    //   5. The full episode can be reconstructed from trace records

    #[test]
    fn e2e_cognitive_episode_reconstruction() {
        let collector = InMemoryTraceCollector::new();

        // Identity for this episode.
        let session_id = SessionId::default();
        let goal_id = GoalId::default();
        let trace_id = TraceId::default();

        // Root context.
        let ctx = CognitiveTraceContext::for_goal(trace_id, session_id, goal_id);

        // ── 1. Goal Resolution (root span) ──
        let goal_span = SpanId::default();
        let goal_trace = ctx.trace(goal_span, "goal", "resolve");
        assert!(
            goal_trace.parent_span_id.is_none(),
            "root must have no parent"
        );
        TraceCollector::record(&collector, goal_trace);

        // ── 2. Intent Classification ──
        let intent_ctx = ctx.child(goal_span);
        let intent_span = SpanId::default();
        let intent_trace = intent_ctx
            .trace(intent_span, "intent", "classify")
            .with_result(ResultType::Success)
            .with_confidence(0.97);
        assert_eq!(intent_trace.parent_span_id, Some(goal_span));
        TraceCollector::record(&collector, intent_trace);

        // ── 3. Planning ──
        let plan_ctx = intent_ctx.child(intent_span);
        let plan_span = SpanId::default();
        let plan_trace = plan_ctx
            .trace(plan_span, "planner", "create_plan")
            .with_model("openrouter/gpt-4o", "openrouter")
            .with_tokens(1200, 340, 0.008)
            .with_result(ResultType::Success);
        assert_eq!(plan_trace.parent_span_id, Some(intent_span));
        TraceCollector::record(&collector, plan_trace);

        // ── 4. Model Call (child of planning) ──
        let model_ctx = plan_ctx.child(plan_span);
        let model_span = SpanId::default();
        let model_trace = model_ctx
            .trace(model_span, "planner", "llm_call")
            .with_model("gpt-4o", "openai")
            .with_tokens(2400, 890, 0.015)
            .with_result(ResultType::Success)
            .with_timing(chrono::Utc::now() - chrono::Duration::seconds(2), 2100);
        assert_eq!(model_trace.parent_span_id, Some(plan_span));
        TraceCollector::record(&collector, model_trace);

        // ── 5. Agent Execution ──
        let agent_ctx = plan_ctx.child(plan_span);
        let agent_span = SpanId::default();
        let agent_trace = agent_ctx
            .trace(agent_span, "agents", "execute")
            .with_result(ResultType::Success);
        assert_eq!(agent_trace.parent_span_id, Some(plan_span));
        TraceCollector::record(&collector, agent_trace);

        // ── 6. Tool Call (child of agent) ──
        let tool_ctx = agent_ctx.child(agent_span);
        let tool_span = SpanId::default();
        let tool_trace = tool_ctx
            .trace(tool_span, "web", "search")
            .with_result(ResultType::Success)
            .with_metadata(serde_json::json!({"query": "Rust async 2026"}));
        assert_eq!(tool_trace.parent_span_id, Some(agent_span));
        TraceCollector::record(&collector, tool_trace);

        // ── 7. Verification ──
        let verify_ctx = plan_ctx.child(plan_span);
        let verify_span = SpanId::default();
        let verify_trace = verify_ctx
            .trace(verify_span, "verification", "verify")
            .with_verification(VerificationStatus::Passed)
            .with_result(ResultType::Success);
        assert_eq!(verify_trace.parent_span_id, Some(plan_span));
        TraceCollector::record(&collector, verify_trace);

        // ── Assertions ──

        let traces = collector.traces();
        let all_traces: Vec<CognitiveTrace> = traces.clone();

        // 1. Validate hierarchy is structurally sound.
        validate_trace_hierarchy(&all_traces).expect("trace hierarchy must be valid");

        // 2. All spans share the same trace_id.
        for t in &all_traces {
            assert_eq!(
                t.trace_id, trace_id,
                "every span must share the episode's trace_id"
            );
        }

        // 3. All spans share the same session and goal.
        for t in &all_traces {
            assert_eq!(t.session_id, session_id);
            assert_eq!(t.goal_id, goal_id);
        }

        // 4. Reconstruct the tree.
        let trees = reconstruct_trace_tree(&all_traces);
        assert_eq!(trees.len(), 1, "must have exactly one root");

        let root = &trees[0];
        assert_eq!(root.trace.span_id, goal_span);
        // goal → intent → plan → (model, agent, verify)
        assert_eq!(root.children.len(), 1, "intent subtree under goal");

        let intent_subtree = &root.children[0];
        assert_eq!(intent_subtree.trace.operation, "classify");
        assert_eq!(
            intent_subtree.children.len(),
            1,
            "plan subtree under intent"
        );

        // Find the planning subtree.
        let plan_subtree = &intent_subtree.children[0];
        assert_eq!(plan_subtree.trace.operation, "create_plan");
        assert_eq!(plan_subtree.children.len(), 3, "model + agent + verify");

        // Verify model call is a child of planning.
        let model_subtree = plan_subtree
            .children
            .iter()
            .find(|c| c.trace.operation == "llm_call")
            .expect("model span must exist");
        assert_eq!(model_subtree.trace.parent_span_id, Some(plan_span));
        assert!(model_subtree.trace.model_usage.is_some());

        // Verify agent has a tool call child.
        let agent_subtree = plan_subtree
            .children
            .iter()
            .find(|c| c.trace.operation == "execute")
            .expect("agent span must exist");
        assert_eq!(agent_subtree.children.len(), 1);
        assert_eq!(agent_subtree.children[0].trace.operation, "search");

        // 5. Model telemetry was captured.
        let model_trace = &model_subtree.trace;
        assert_eq!(model_trace.model(), Some("gpt-4o"));
        assert_eq!(model_trace.provider(), Some("openai"));
        assert_eq!(model_trace.tokens_in(), Some(2400));
        assert_eq!(model_trace.tokens_out(), Some(890));
        assert_eq!(model_trace.cost_usd(), Some(0.015));

        // 6. Verification was captured.
        let verify_subtree = plan_subtree
            .children
            .iter()
            .find(|c| c.trace.operation == "verify")
            .expect("verify span must exist");
        assert!(verify_subtree.trace.verification().is_passed());

        // 7. Duration was captured.
        assert!(model_subtree.trace.duration_ms > 0);

        println!(
            "Full episode reconstructed from {} trace records:",
            all_traces.len()
        );
        println!("  trace_id = {}", trace_id);
        for t in &all_traces {
            let parent = t
                .parent_span_id
                .map(|p| p.to_string())
                .unwrap_or_else(|| "none".to_string());
            println!(
                "  span {} | {} / {} | parent = {}",
                t.span_id, t.organ, t.operation, parent
            );
        }
    }
}
