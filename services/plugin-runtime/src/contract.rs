use std::collections::HashMap;

/// A correlation ID that links events across a single logical request.
///
/// Every stage of a capability invocation carries the same correlation
/// ID, making it possible to reconstruct the full execution tree from
/// the event stream.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CorrelationId(pub String);

impl CorrelationId {
    pub fn new() -> Self {
        Self(uuid_v4())
    }

    pub fn from_string(s: impl Into<String>) -> Self {
        Self(s.into())
    }
}

impl Default for CorrelationId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for CorrelationId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

fn uuid_v4() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    format!("{:016x}-{:016x}", nanos >> 64, nanos & 0xffffffffffffffff)
}

/// A capability — what a tool can do.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Capability {
    ExecuteCommand,
    ReadFile,
    WriteFile,
    DeleteFile,
    ListDirectory,
    OpenBrowser,
    SearchWeb,
    SearchMemory,
    Custom(String),
}

impl Capability {
    pub fn as_str(&self) -> &str {
        match self {
            Self::ExecuteCommand => "execute_command",
            Self::ReadFile => "read_file",
            Self::WriteFile => "write_file",
            Self::DeleteFile => "delete_file",
            Self::ListDirectory => "list_directory",
            Self::OpenBrowser => "open_browser",
            Self::SearchWeb => "search_web",
            Self::SearchMemory => "search_memory",
            Self::Custom(s) => s,
        }
    }
}

/// A category grouping for capabilities.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CapabilityCategory {
    Filesystem,
    Browser,
    System,
    Network,
    Knowledge,
    Custom(String),
}

/// Safety level for a capability.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SafetyLevel {
    Safe,
    Moderate,
    Sensitive,
    Critical,
}

/// Full metadata defining what a capability is, its semantics, and requirements.
///
/// This is the catalog entry. Providers register against these descriptors.
#[derive(Debug, Clone)]
pub struct CapabilityDescriptor {
    pub capability: Capability,
    pub version: String,
    pub category: CapabilityCategory,
    pub description: String,
    pub required_permissions: Vec<String>,
    pub estimated_cost: f64,
    pub expected_latency_ms: u64,
    pub safety_level: SafetyLevel,
    pub tags: Vec<String>,
}

/// Static behavioral characteristics of a provider.
///
/// Unlike health (which changes at runtime), the profile describes
/// intrinsic properties that inform selection — e.g. whether a
/// provider works offline, produces deterministic output, or
/// mutates external state.
#[derive(Debug, Clone)]
pub struct ProviderProfile {
    pub plugin_id: String,
    pub deterministic: bool,
    pub offline_capable: bool,
    pub streaming: bool,
    pub has_side_effects: bool,
    pub requires_network: bool,
    pub max_concurrency: u32,
    pub preferred_contexts: Vec<String>,
}

impl ProviderProfile {
    pub fn default_for(plugin_id: &str) -> Self {
        Self {
            plugin_id: plugin_id.to_string(),
            deterministic: true,
            offline_capable: true,
            streaming: false,
            has_side_effects: false,
            requires_network: false,
            max_concurrency: 4,
            preferred_contexts: Vec::new(),
        }
    }
}

/// Static metadata about a plugin.
#[derive(Debug, Clone)]
pub struct PluginDescriptor {
    pub id: String,
    pub name: String,
    pub version: String,
    pub author: String,
    pub capabilities: Vec<Capability>,
    pub permissions: Vec<String>,
    pub profile: ProviderProfile,
}

/// How urgently a capability execution is needed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Urgency {
    Low,
    Normal,
    High,
    Critical,
}

impl Default for Urgency {
    fn default() -> Self {
        Self::Normal
    }
}

/// Security sensitivity level for a request context.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SecurityLevel {
    Low,
    Medium,
    High,
    Maximum,
}

impl Default for SecurityLevel {
    fn default() -> Self {
        Self::Medium
    }
}

/// Contextual information about a capability request.
///
/// Carries environmental signals that influence provider selection:
/// urgency, budget, offline mode, security requirements, and
/// user-specified preferences. The resolver uses this to bias
/// toward providers whose profile matches the situation.
#[derive(Debug, Clone)]
pub struct SelectionContext {
    pub urgency: Urgency,
    pub budget: f64,
    pub offline: bool,
    pub security_level: SecurityLevel,
    pub user_preferences: HashMap<String, String>,
    pub preferred_providers: Vec<String>,
}

impl Default for SelectionContext {
    fn default() -> Self {
        Self {
            urgency: Urgency::Normal,
            budget: f64::MAX,
            offline: false,
            security_level: SecurityLevel::Medium,
            user_preferences: HashMap::new(),
            preferred_providers: Vec::new(),
        }
    }
}

impl SelectionContext {
    pub fn urgent() -> Self {
        Self {
            urgency: Urgency::Critical,
            budget: f64::MAX,
            offline: false,
            security_level: SecurityLevel::Low,
            user_preferences: HashMap::new(),
            preferred_providers: Vec::new(),
        }
    }

    pub fn offline() -> Self {
        Self {
            urgency: Urgency::Normal,
            budget: f64::MAX,
            offline: true,
            security_level: SecurityLevel::Medium,
            user_preferences: HashMap::new(),
            preferred_providers: Vec::new(),
        }
    }

    pub fn with_preferred(mut self, provider_id: &str) -> Self {
        self.preferred_providers.push(provider_id.to_string());
        self
    }
}

/// A request to execute a capability.
#[derive(Debug, Clone)]
pub struct CapabilityRequest {
    pub capability: Capability,
    pub parameters: HashMap<String, String>,
    pub permissions: Vec<String>,
    pub correlation_id: CorrelationId,
    pub context: SelectionContext,
}

impl CapabilityRequest {
    pub fn new(capability: Capability) -> Self {
        Self {
            capability,
            parameters: HashMap::new(),
            permissions: Vec::new(),
            correlation_id: CorrelationId::new(),
            context: SelectionContext::default(),
        }
    }

    pub fn with_correlation(mut self, id: CorrelationId) -> Self {
        self.correlation_id = id;
        self
    }

    pub fn with_context(mut self, ctx: SelectionContext) -> Self {
        self.context = ctx;
        self
    }

    pub fn with_parameter(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.parameters.insert(key.into(), value.into());
        self
    }

    pub fn parameter(&self, key: &str) -> Option<&String> {
        self.parameters.get(key)
    }
}

/// Telemetry collected during a single execution.
#[derive(Debug, Clone)]
pub struct ExecutionTelemetry {
    pub started_at_ms: u64,
    pub finished_at_ms: u64,
    pub plugin_id: String,
    pub success: bool,
    pub correlation_id: CorrelationId,
}

impl ExecutionTelemetry {
    pub fn duration_ms(&self) -> u64 {
        self.finished_at_ms.saturating_sub(self.started_at_ms)
    }
}

/// The result returned from executing a capability.
#[derive(Debug, Clone)]
pub struct CapabilityResult {
    pub success: bool,
    pub output: String,
    pub metadata: HashMap<String, String>,
    pub telemetry: ExecutionTelemetry,
}

/// Health status of a plugin.
#[derive(Debug, Clone, PartialEq)]
pub enum PluginStatus {
    Installed,
    Loaded,
    Initialized,
    Healthy,
    Busy,
    Failed,
    Stopped,
    Unloaded,
}

/// Continuously-updated health metrics for a plugin.
#[derive(Debug, Clone)]
pub struct PluginHealth {
    pub plugin_id: String,
    pub status: PluginStatus,
    pub availability: f64,
    pub reliability: f64,
    pub average_latency_ms: f64,
    pub success_rate: f64,
    pub total_executions: u64,
    pub failure_count: u64,
    pub last_error: Option<String>,
    pub last_execution_ms: Option<u64>,
}

/// The resolved choice of which plugin should handle a capability.
#[derive(Debug, Clone)]
pub struct ResolvedCapability {
    pub plugin_id: String,
    pub capability: Capability,
    pub confidence: f64,
    pub rationale: String,
    pub correlation_id: CorrelationId,
}

/// Scoring weights for the capability resolver.
///
/// Each weight determines how much a signal contributes to the
/// composite provider score. All weights should be in [0, 1].
#[derive(Debug, Clone)]
pub struct ScoringWeights {
    pub reliability: f64,
    pub availability: f64,
    pub latency: f64,
    pub cost: f64,
    pub policy: f64,
    pub profile_match: f64,
    pub context_fit: f64,
}

impl Default for ScoringWeights {
    fn default() -> Self {
        Self {
            reliability: 0.25,
            availability: 0.20,
            latency: 0.15,
            cost: 0.10,
            policy: 0.10,
            profile_match: 0.10,
            context_fit: 0.10,
        }
    }
}

/// A candidate provider with its score breakdown.
#[derive(Debug, Clone)]
pub struct ScoredCandidate {
    pub plugin_id: String,
    pub plugin_name: String,
    pub reliability_score: f64,
    pub availability_score: f64,
    pub latency_penalty: f64,
    pub cost_penalty: f64,
    pub policy_bonus: f64,
    pub profile_match_score: f64,
    pub context_fit_score: f64,
    pub weighted_score: f64,
}

/// The rationale for a capability selection decision.
#[derive(Debug, Clone)]
pub struct SelectionRationale {
    pub capability: Capability,
    pub candidates: Vec<ScoredCandidate>,
    pub selected_plugin_id: String,
    pub composite_score: f64,
    pub explanation: String,
    pub correlation_id: CorrelationId,
}

/// Errors returned by the plugin runtime.
#[derive(Debug)]
pub enum RuntimeError {
    PluginNotFound(String),
    CapabilityNotSupported(Capability),
    ExecutionFailed(String),
    PluginNotReady(String),
    NoProviderFound(Capability),
}

impl std::fmt::Display for RuntimeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::PluginNotFound(id) => write!(f, "plugin not found: {}", id),
            Self::CapabilityNotSupported(cap) => {
                write!(f, "capability not supported: {}", cap.as_str())
            }
            Self::ExecutionFailed(msg) => write!(f, "execution failed: {}", msg),
            Self::PluginNotReady(id) => write!(f, "plugin not ready: {}", id),
            Self::NoProviderFound(cap) => {
                write!(f, "no provider found for capability: {}", cap.as_str())
            }
        }
    }
}

// ── Telemetry ──────────────────────────────────────────────────────────────

/// A structured event emitted by the runtime.
///
/// These are the runtime's "flight recorder" — every significant decision
/// produces one, tagged with a correlation ID so the full execution tree
/// can be reconstructed from the event stream.
#[derive(Debug, Clone)]
pub enum TelemetryEvent {
    CapabilityRequested(Capability),
    ProviderResolved {
        plugin_id: String,
        confidence: f64,
        candidate_count: usize,
    },
    PluginInvoked {
        plugin_id: String,
        capability: Capability,
    },
    PluginCompleted {
        plugin_id: String,
        duration_ms: u64,
        success: bool,
    },
    PluginFailed {
        plugin_id: String,
        error: String,
        duration_ms: u64,
    },
    HealthChanged {
        plugin_id: String,
        old_status: PluginStatus,
        new_status: PluginStatus,
    },
    PolicyEvaluated {
        capability: Capability,
        candidates_before: usize,
        candidates_after: usize,
    },
    ResolverScored {
        capability: Capability,
        candidates: Vec<ScoredCandidate>,
        selected: String,
    },
    RetryAttempted {
        plugin_id: String,
        attempt: u32,
        reason: String,
    },
    RuntimeWarning(String),
}

/// Envelope wrapping every telemetry event with common metadata.
#[derive(Debug, Clone)]
pub struct TelemetryEnvelope {
    pub timestamp_ms: u64,
    pub correlation_id: CorrelationId,
    pub execution_id: Option<String>,
    pub event: TelemetryEvent,
    pub duration_ms: Option<u64>,
    pub metadata: HashMap<String, String>,
}

impl TelemetryEnvelope {
    pub fn new(correlation_id: CorrelationId, event: TelemetryEvent) -> Self {
        Self {
            timestamp_ms: now_ms(),
            correlation_id,
            execution_id: None,
            event,
            duration_ms: None,
            metadata: HashMap::new(),
        }
    }
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

// ── Decision Intelligence ───────────────────────────────────────────────────

/// Identifies which evaluator produced a signal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EvaluatorKind {
    Health,
    Policy,
    Profile,
    Context,
    Experience,
    Reputation,
}

/// A single evaluator's contribution to a planning decision.
#[derive(Debug, Clone)]
pub struct EvaluatorSignal {
    pub source: EvaluatorKind,
    pub score: f64,
    pub confidence: f64,
    pub explanation: String,
}

/// A dimensional cost estimate for a single operator.
#[derive(Debug, Clone)]
pub struct CostEstimate {
    pub operator_id: String,
    pub estimated_latency_ms: u64,
    pub estimated_cost: f64,
    pub estimated_success_probability: f64,
    pub estimated_risk: f64,
    pub confidence: f64,
}

/// What actually happened during execution.
#[derive(Debug, Clone)]
pub struct PlanOutcome {
    pub latency_ms: u64,
    pub cost: f64,
    pub success: bool,
    pub error: Option<String>,
    pub completed_steps: u64,
    pub cancelled: bool,
    pub timeout: bool,
    pub retries: u32,
}

/// Rationale for a planning decision.
///
/// Evolved from SelectionRationale. Carries evidence from each evaluator
/// instead of a single composite score.
#[derive(Debug, Clone)]
pub struct PlanningRationale {
    pub candidates: Vec<ScoredCandidate>,
    pub selected_operator_id: String,
    pub composite_score: f64,
    pub explanation: String,
    pub correlation_id: CorrelationId,
    pub evaluator_evidence: Vec<EvaluatorSignal>,
}

/// An execution plan produced by the planner.
///
/// Immutable once created. If assumptions change, the planner produces
/// a new plan with an incremented version.
#[derive(Debug, Clone)]
pub struct ExecutionPlan {
    pub plan_id: CorrelationId,
    pub version: u32,
    pub capability: Capability,
    pub operator_id: String,
    pub estimated_latency_ms: u64,
    pub estimated_cost: f64,
    pub estimated_success_probability: f64,
    pub estimated_risk: f64,
    pub confidence: f64,
    pub fallback_chain: Vec<ExecutionPlan>,
    pub rationale: PlanningRationale,
}

/// Post-hoc analysis of an execution.
#[derive(Debug, Clone)]
pub struct DecisionReflection {
    pub latency_variance: f64,
    pub cost_variance: f64,
    pub success_accurate: bool,
    pub failure_reason: Option<String>,
    pub lessons: Vec<String>,
}

/// A complete record of a planning-and-execution cycle.
///
/// Immutable, append-only. The central primitive from which experience,
/// reputation, and cost models derive.
#[derive(Debug, Clone)]
pub struct DecisionRecord {
    pub id: CorrelationId,
    pub timestamp_ms: u64,
    pub capability: Capability,
    pub context: SelectionContext,
    pub parameters: HashMap<String, String>,
    pub plan: ExecutionPlan,
    pub outcome: PlanOutcome,
    pub reflection: Option<DecisionReflection>,
}

/// Feedback from execution back to the cost model.
///
/// Consumed by the CostModel, never by the planner directly.
#[derive(Debug, Clone)]
pub struct PlannerFeedback {
    pub capability: Capability,
    pub operator_id: String,
    pub latency_error_ms: i64,
    pub cost_error: f64,
    pub success_bias: f64,
    pub latency_adjustment_ms: f64,
    pub cost_adjustment: f64,
    pub success_adjustment: f64,
    pub confidence_adjustment: f64,
    pub trigger_model_update: bool,
    pub timestamp_ms: u64,
}

impl PlannerFeedback {
    /// Quantify the gap between prediction and reality.
    ///
    /// Pure comparison — no mutation, no stores, no learning.
    ///
    /// # Fields
    ///
    /// | Field                 | Derivation                                  |
    /// |----------------------|---------------------------------------------|
    /// | `latency_error_ms`   | estimated − actual                          |
    /// | `cost_error`         | estimated − actual                          |
    /// | `success_bias`       | estimated_success_probability − outcome     |
    /// | `latency_adjustment` | raw error (CostModel decides how to apply)  |
    /// | `cost_adjustment`    | raw error                                   |
    /// | `success_adjustment` | raw bias                                    |
    /// | `confidence_adjust`  | +0.1 on correct, −0.3 on surprise failure   |
    /// | `trigger_update`     | true if error exceeds threshold             |
    pub fn compare(plan: &ExecutionPlan, outcome: &PlanOutcome) -> Self {
        let now = now_ms();
        let success_actual = if outcome.success { 1.0 } else { 0.0 };

        let latency_error_ms = plan.estimated_latency_ms as i64 - outcome.latency_ms as i64;
        let cost_error = plan.estimated_cost - outcome.cost;
        let success_bias = plan.estimated_success_probability - success_actual;

        let latency_adjustment_ms = latency_error_ms as f64;
        let cost_adjustment = cost_error;
        let success_adjustment = success_bias;

        let correct = outcome.success == (plan.estimated_success_probability >= 0.5);
        let confidence_adjustment = if correct {
            0.1
        } else if plan.confidence > 0.7 {
            -0.3
        } else {
            -0.1
        };

        let trigger_model_update = !correct
            || latency_error_ms.unsigned_abs() > (plan.estimated_latency_ms / 2)
            || cost_error.abs() > 1.0;

        Self {
            capability: plan.capability.clone(),
            operator_id: plan.operator_id.clone(),
            latency_error_ms,
            cost_error,
            success_bias,
            latency_adjustment_ms,
            cost_adjustment,
            success_adjustment,
            confidence_adjustment,
            trigger_model_update,
            timestamp_ms: now,
        }
    }
}

/// Key for looking up experience data.
///
/// Designed for hierarchical decomposition. The key can gain new context
/// dimensions without changing the aggregate type.
#[derive(Debug, Clone, Hash, Eq, PartialEq)]
pub struct ExperienceKey {
    pub capability: Capability,
    pub operator_id: String,
}

/// Statistical aggregates derived from DecisionRecords.
#[derive(Debug, Clone)]
pub struct Experience {
    pub key: ExperienceKey,
    pub sample_size: u64,
    pub success_rate: f64,
    pub success_count: u64,
    pub failure_count: u64,
    pub average_latency_ms: f64,
    pub p50_latency_ms: u64,
    pub p95_latency_ms: u64,
    pub p99_latency_ms: u64,
    pub last_execution_ms: Option<u64>,
    pub last_failure_ms: Option<u64>,
    pub last_failure_reason: Option<String>,
    pub consecutive_successes: u64,
    pub consecutive_failures: u64,
    pub since_ms: u64,
}

/// Long-term trust derived from Experience with time decay.
#[derive(Debug, Clone)]
pub struct Reputation {
    pub operator_id: String,
    pub reliability: f64,
    pub confidence: f64,
    pub total_observations: u64,
    pub since_ms: u64,
    pub last_updated_ms: u64,
    pub decay_factor: f64,
}

impl Default for Reputation {
    fn default() -> Self {
        Self {
            operator_id: String::new(),
            reliability: 0.5,
            confidence: 0.0,
            total_observations: 0,
            since_ms: 0,
            last_updated_ms: 0,
            decay_factor: 0.95,
        }
    }
}

#[cfg(test)]
mod planner_feedback_tests {
    use super::*;

    fn make_plan(
        estimated_latency_ms: u64,
        estimated_cost: f64,
        estimated_success_probability: f64,
        confidence: f64,
    ) -> ExecutionPlan {
        ExecutionPlan {
            plan_id: CorrelationId::new(),
            version: 1,
            capability: Capability::ExecuteCommand,
            operator_id: "op_test".into(),
            estimated_latency_ms,
            estimated_cost,
            estimated_success_probability,
            estimated_risk: 1.0 - estimated_success_probability,
            confidence,
            fallback_chain: vec![],
            rationale: PlanningRationale {
                candidates: vec![],
                selected_operator_id: "op_test".into(),
                composite_score: 0.8,
                explanation: "test".into(),
                correlation_id: CorrelationId::new(),
                evaluator_evidence: vec![],
            },
        }
    }

    fn make_outcome(latency_ms: u64, cost: f64, success: bool) -> PlanOutcome {
        PlanOutcome {
            latency_ms,
            cost,
            success,
            error: if success { None } else { Some("fail".into()) },
            completed_steps: 1,
            cancelled: !success,
            timeout: false,
            retries: 0,
        }
    }

    #[test]
    fn compare_latency_overestimate() {
        let plan = make_plan(200, 0.5, 0.9, 0.8);
        let outcome = make_outcome(50, 0.3, true);
        let fb = PlannerFeedback::compare(&plan, &outcome);
        assert_eq!(fb.latency_error_ms, 150);
        assert_eq!(fb.latency_adjustment_ms, 150.0);
    }

    #[test]
    fn compare_latency_underestimate() {
        let plan = make_plan(50, 0.5, 0.9, 0.8);
        let outcome = make_outcome(200, 0.7, true);
        let fb = PlannerFeedback::compare(&plan, &outcome);
        assert_eq!(fb.latency_error_ms, -150);
        assert_eq!(fb.latency_adjustment_ms, -150.0);
    }

    #[test]
    fn compare_cost_error() {
        let plan = make_plan(100, 1.0, 0.9, 0.8);
        let outcome = make_outcome(100, 0.4, true);
        let fb = PlannerFeedback::compare(&plan, &outcome);
        assert_eq!(fb.cost_error, 0.6);
        assert_eq!(fb.cost_adjustment, 0.6);
    }

    #[test]
    fn compare_success_bias_overconfident() {
        let plan = make_plan(100, 0.5, 0.95, 0.9);
        let outcome = make_outcome(100, 0.5, false);
        let fb = PlannerFeedback::compare(&plan, &outcome);
        assert!((fb.success_bias - 0.95).abs() < 1e-10); // 0.95 - 0.0
        assert!((fb.success_adjustment - 0.95).abs() < 1e-10);
    }

    #[test]
    fn compare_success_bias_underconfident() {
        let plan = make_plan(100, 0.5, 0.3, 0.4);
        let outcome = make_outcome(100, 0.5, true);
        let fb = PlannerFeedback::compare(&plan, &outcome);
        assert!((fb.success_bias - (-0.7)).abs() < 1e-10); // 0.3 - 1.0
    }

    #[test]
    fn compare_confidence_adjustment_correct_prediction() {
        let plan = make_plan(100, 0.5, 0.85, 0.9);
        let outcome = make_outcome(100, 0.5, true);
        let fb = PlannerFeedback::compare(&plan, &outcome);
        assert!((fb.confidence_adjustment - 0.1).abs() < 1e-10);
    }

    #[test]
    fn compare_confidence_adjustment_surprise_failure() {
        let plan = make_plan(100, 0.5, 0.9, 0.8);
        let outcome = make_outcome(100, 0.5, false);
        let fb = PlannerFeedback::compare(&plan, &outcome);
        assert!((fb.confidence_adjustment - (-0.3)).abs() < 1e-10);
    }

    #[test]
    fn compare_confidence_adjustment_expected_failure() {
        let plan = make_plan(100, 0.5, 0.2, 0.9);
        let outcome = make_outcome(100, 0.5, false);
        let fb = PlannerFeedback::compare(&plan, &outcome);
        assert!((fb.confidence_adjustment - 0.1).abs() < 1e-10);
    }

    #[test]
    fn compare_trigger_on_wrong_prediction() {
        let plan = make_plan(100, 0.5, 0.9, 0.9);
        let outcome = make_outcome(100, 0.5, false);
        let fb = PlannerFeedback::compare(&plan, &outcome);
        assert!(fb.trigger_model_update);
    }

    #[test]
    fn compare_trigger_on_large_latency_error() {
        let plan = make_plan(100, 0.5, 0.9, 0.9);
        let outcome = make_outcome(10, 0.3, true);
        let fb = PlannerFeedback::compare(&plan, &outcome);
        assert!(fb.trigger_model_update);
    }

    #[test]
    fn compare_no_trigger_on_accurate_prediction() {
        let plan = make_plan(100, 0.5, 0.85, 0.9);
        let outcome = make_outcome(95, 0.5, true);
        let fb = PlannerFeedback::compare(&plan, &outcome);
        assert!(!fb.trigger_model_update);
    }

    #[test]
    fn compare_preserves_capability_and_operator() {
        let plan = make_plan(100, 0.5, 0.9, 0.8);
        let outcome = make_outcome(100, 0.5, true);
        let fb = PlannerFeedback::compare(&plan, &outcome);
        assert_eq!(fb.capability, Capability::ExecuteCommand);
        assert_eq!(fb.operator_id, "op_test");
    }

    #[test]
    fn compare_timestamp_is_set() {
        let plan = make_plan(100, 0.5, 0.9, 0.8);
        let outcome = make_outcome(100, 0.5, true);
        let fb = PlannerFeedback::compare(&plan, &outcome);
        assert!(fb.timestamp_ms > 0);
    }
}

/// The contract every plugin must implement.
pub trait Plugin: Send + Sync {
    fn descriptor(&self) -> PluginDescriptor;
    fn initialize(&mut self);
    fn execute(&self, request: CapabilityRequest) -> CapabilityResult;
    fn health(&self) -> PluginHealth;
    fn shutdown(&mut self);
}
