//! Provider qualification profiles and runtime health.
//!
//! Two deliberately separate layers, never mixed:
//!
//! * [`QualificationProfile`] — an immutable, harness-generated artifact
//!   (offline benchmark data). Raw measurements only; **no composite score**.
//! * [`ProviderHealth`] — mutable operational state maintained by the fabric
//!   (availability, moving latencies, success rate, rate-limit state, last error).
//!
//! The router combines them at runtime via [`routing_score`], so scoring-weight
//! changes never invalidate historical qualification artifacts.

use serde::{Deserialize, Serialize};

use crate::providers::CircuitState;

/// Version of the `QualificationProfile` JSON schema. Bump on any incompatible
/// change (added required field, renamed field, changed semantics). Readers
/// reject profiles whose version they do not understand rather than guess.
pub const QUALIFICATION_SCHEMA_VERSION: u32 = 1;

/// How a qualification run was produced — reproducibility metadata so two runs
/// can be compared and re-benchmarks can be scheduled.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BenchmarkConfig {
    pub prompt_set: String,
    pub iterations: usize,
    pub context_size: usize,
    pub temperature: f64,
    pub gpu_layers: usize,
    pub batch_size: usize,
}

/// Immutable qualification artifact produced by `provider-qual`.
///
/// Raw measurements only. [`routing_score`] derives a composite at runtime,
/// which is why historical profiles never go stale when scoring policy changes.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct QualificationProfile {
    pub schema_version: u32,
    pub provider_id: String,
    pub model_id: String,
    pub benchmark_config: BenchmarkConfig,
    /// Epoch milliseconds at generation time.
    pub generated_at_ms: u64,
    /// Mean time-to-first-token, when the provider surfaced a stream.
    pub ttft_ms: Option<f64>,
    /// Mean generation throughput. Derived from reassembled characters (÷4 token
    /// heuristic) when the provider does not report usage tokens.
    pub tokens_per_second: Option<f64>,
    /// Fraction of JSON-verification prompts that produced schema-valid output.
    pub json_accuracy: Option<f64>,
    /// Tool capability/accuracy. Currently the provider's declared tool-support
    /// fact (1.0/0.0); a graded tool-calling scenario will replace it.
    pub tool_accuracy: Option<f64>,
    /// Peak host memory observed during the run, when the harness could sample it.
    pub peak_ram_mb: Option<u32>,
    /// Peak accelerator memory observed during the run, when measurable.
    pub peak_vram_mb: Option<u32>,
    /// Wall-clock duration of the benchmark run.
    pub benchmark_duration_ms: u64,
}

impl QualificationProfile {
    /// Whether this profile can be consumed by the current reader.
    pub fn is_compatible(&self) -> bool {
        self.schema_version == QUALIFICATION_SCHEMA_VERSION
    }
}

/// Mutable operational state maintained by the fabric at runtime.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ProviderHealth {
    /// Epoch milliseconds of the last observation.
    pub last_updated_ms: u64,
    /// Fraction of recent checks/requests that succeeded (0.0..=1.0).
    pub availability: f64,
    /// Exponentially smoothed time-to-first-token, when observed.
    pub moving_ttft_ms: Option<f64>,
    /// Exponentially smoothed generation throughput, when observed.
    pub moving_tokens_per_second: Option<f64>,
    /// Fraction of recent completions that succeeded (0.0..=1.0).
    pub success_rate: f64,
    /// Current rate-limit budget (requests), when the provider reports one.
    pub current_rate_limit: Option<u32>,
    /// Last error observed, if any.
    pub last_error: Option<String>,
    pub circuit_state: CircuitState,
}

impl Default for ProviderHealth {
    fn default() -> Self {
        Self {
            last_updated_ms: 0,
            availability: 0.0,
            moving_ttft_ms: None,
            moving_tokens_per_second: None,
            success_rate: 0.0,
            current_rate_limit: None,
            last_error: None,
            circuit_state: CircuitState::Closed,
        }
    }
}

/// How strongly a request needs a model capability.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum Requirement {
    /// Not needed — no effect on selection.
    #[default]
    None,
    /// Nice to have — matching candidates get a score boost.
    Preferred,
    /// Hard requirement — non-matching candidates are excluded by the fabric.
    Required,
}

/// How strongly a request needs local execution (privacy).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum Preference {
    /// Location-agnostic.
    #[default]
    Any,
    /// Prefer a local model when one is available.
    Preferred,
    /// Must run locally — remote candidates are excluded by the fabric.
    Required,
}

/// Cost budget for a request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum Budget {
    /// Free-tier providers and local models only.
    #[default]
    Free,
    /// Paid/bounded spend allowed — every registered provider is eligible.
    Bounded,
}

/// The planner-facing constraint set — a "Model Budget". Callers express what
/// a request needs (cost, latency, privacy, quality); the fabric routes within
/// those constraints. Defaults encode the **free-first policy**: local models
/// preferred, free-tier budget, no hard capability requirements.
///
/// `RequestRequirements` is constraints, never provider names — the planner
/// says "vision, prefer local, free", never "use gpt-4o". Swapping a provider
/// or re-weighting scoring never changes this contract.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RequestRequirements {
    /// Reasoning depth demanded by the task.
    pub reasoning: Requirement,
    /// Hard latency bound in milliseconds. Slower candidates are excluded.
    pub latency_ms: Option<u32>,
    /// Local-execution / privacy preference.
    pub privacy: Preference,
    /// Cost budget.
    pub budget: Budget,
    pub vision: Requirement,
    pub long_context: Requirement,
    /// Tool-calling requirement (the planner needs tool calling, so this is
    /// usually at least `Preferred`).
    pub tools: Requirement,
}

impl RequestRequirements {
    /// Convert a capability preference list into routing requirements. `Local`
    /// raises the privacy preference; `Vision`/`LongContext`/`Tools` become
    /// preferred. The bridge takes *this crate's* capability vocabulary — a
    /// caller holding intelligence's `ModelCapability` (e.g. the planner's
    /// `InferenceBackend`) can turn preferences into constraints directly.
    /// Note: `friday-agent-runtime`'s self-contained `ModelCapability` (which
    /// adds `Coding`/`Reasoning`/`Fast`) has no intelligence dependency, so
    /// `AgentSpec.preferred_models` remains a reserved contract until an
    /// LLM-using driver maps it through this bridge.
    pub fn from_capabilities(caps: &[crate::providers::ModelCapability]) -> Self {
        use crate::providers::ModelCapability as C;
        let mut req = Self::default();
        for c in caps {
            match c {
                C::Local => req.privacy = Preference::Preferred,
                C::Vision => req.vision = Requirement::Preferred,
                C::LongContext => req.long_context = Requirement::Preferred,
                C::Tools => req.tools = Requirement::Preferred,
                C::Json => {}
            }
        }
        req
    }
}

/// Capability/latency facts about one routing candidate, extracted from its
/// `ModelInfo` so [`routing_score`] can apply requirement adjustments without
/// depending on the fabric's provider types.
#[derive(Debug, Clone)]
pub struct CandidateView {
    pub provider: String,
    pub local: bool,
    pub vision: bool,
    pub long_context: bool,
    pub tools: bool,
    pub latency_ms: u64,
}

/// Combine an immutable qualification profile with current runtime health into a
/// single routing score (0.0..=1.0), then adjust for the request's
/// requirements.
///
/// Weights are policy and live HERE, never in the profile file, so re-weighting
/// never invalidates historical benchmark data. Missing measurements fall back
/// to a neutral midpoint rather than biasing the score. A missing profile
/// (`None`) also falls back to the midpoint — candidates without qualification
/// evidence are scored neutrally, never invented.
///
/// Requirement adjustments implement the free-first policy: privacy is the
/// dominant lever (private code stays local even when a remote free model is
/// faster), then capability matches add smaller boosts.
pub fn routing_score(
    profile: Option<&QualificationProfile>,
    health: &ProviderHealth,
    request: &RequestRequirements,
    view: &CandidateView,
) -> f64 {
    let quality = profile
        .map(|p| p.json_accuracy.unwrap_or(0.5))
        .unwrap_or(0.5);
    let speed = speed_component(profile, health);
    let live = (health.success_rate * 0.5 + health.availability * 0.5).clamp(0.0, 1.0);
    let mut score = (quality * 0.5) + (speed * 0.3) + (live * 0.2);

    match request.privacy {
        Preference::Any => {}
        Preference::Preferred => score += if view.local { 0.25 } else { -0.15 },
        Preference::Required => score += if view.local { 0.40 } else { -0.40 },
    }
    if request.vision == Requirement::Preferred && view.vision {
        score += 0.10;
    }
    if request.long_context == Requirement::Preferred && view.long_context {
        score += 0.10;
    }
    if request.tools == Requirement::Preferred && view.tools {
        score += 0.10;
    }
    if request.reasoning == Requirement::Preferred || request.reasoning == Requirement::Required {
        // Reasoning-heavy tasks weight measured quality harder.
        score += 0.05 * quality;
    }

    score.clamp(0.0, 1.0)
}

fn speed_component(profile: Option<&QualificationProfile>, health: &ProviderHealth) -> f64 {
    let ttft = health
        .moving_ttft_ms
        .or_else(|| profile.and_then(|p| p.ttft_ms));
    let tps = health
        .moving_tokens_per_second
        .or_else(|| profile.and_then(|p| p.tokens_per_second));
    let mut s = 0.5;
    if let Some(t) = ttft {
        s += 0.25 * (1.0 - (t / 5000.0).clamp(0.0, 1.0));
    }
    if let Some(t) = tps {
        s += 0.25 * (t / 200.0).clamp(0.0, 1.0);
    }
    s.clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn profile() -> QualificationProfile {
        QualificationProfile {
            schema_version: QUALIFICATION_SCHEMA_VERSION,
            provider_id: "llamacpp".into(),
            model_id: "gemma4:e4b".into(),
            benchmark_config: BenchmarkConfig {
                prompt_set: "test".into(),
                iterations: 1,
                context_size: 8192,
                temperature: 0.0,
                gpu_layers: 0,
                batch_size: 1,
            },
            generated_at_ms: 0,
            ttft_ms: Some(100.0),
            tokens_per_second: Some(50.0),
            json_accuracy: Some(0.9),
            tool_accuracy: Some(1.0),
            peak_ram_mb: None,
            peak_vram_mb: None,
            benchmark_duration_ms: 10,
        }
    }

    fn view() -> CandidateView {
        CandidateView {
            provider: "llamacpp".into(),
            local: true,
            vision: false,
            long_context: true,
            tools: true,
            latency_ms: 120,
        }
    }

    /// A remote (non-local) view, otherwise identical to `view()`.
    fn remote_view() -> CandidateView {
        CandidateView {
            local: false,
            ..view()
        }
    }

    #[test]
    fn schema_version_is_1() {
        assert_eq!(QUALIFICATION_SCHEMA_VERSION, 1);
        assert!(profile().is_compatible());
    }

    #[test]
    fn better_quality_raises_score() {
        let h = ProviderHealth::default();
        let mut p = profile();
        p.json_accuracy = Some(1.0);
        let good = routing_score(Some(&p), &h, &RequestRequirements::default(), &view());
        p.json_accuracy = Some(0.0);
        let bad = routing_score(Some(&p), &h, &RequestRequirements::default(), &view());
        assert!(good > bad);
    }

    #[test]
    fn better_health_raises_score() {
        let h = ProviderHealth {
            success_rate: 1.0,
            availability: 1.0,
            ..ProviderHealth::default()
        };
        let good = routing_score(
            Some(&profile()),
            &h,
            &RequestRequirements::default(),
            &view(),
        );
        let bad = routing_score(
            Some(&profile()),
            &ProviderHealth::default(),
            &RequestRequirements::default(),
            &view(),
        );
        assert!(good > bad);
    }

    #[test]
    fn missing_measurements_do_not_panic_and_stay_finite() {
        let mut p = profile();
        p.json_accuracy = None;
        p.ttft_ms = None;
        p.tokens_per_second = None;
        let s = routing_score(
            Some(&p),
            &ProviderHealth::default(),
            &RequestRequirements::default(),
            &view(),
        );
        assert!(s.is_finite());
        assert!((0.0..=1.0).contains(&s));
    }

    #[test]
    fn privacy_preferred_boosts_local_over_remote() {
        let h = ProviderHealth::default();
        let req = RequestRequirements {
            privacy: Preference::Preferred,
            ..RequestRequirements::default()
        };
        let local = routing_score(Some(&profile()), &h, &req, &view());
        let remote = routing_score(Some(&profile()), &h, &req, &remote_view());
        assert!(
            local > remote,
            "privacy=Preferred must rank local above remote (local {local} vs remote {remote})"
        );
    }

    #[test]
    fn privacy_required_strongly_prefers_local() {
        let h = ProviderHealth::default();
        let req = RequestRequirements {
            privacy: Preference::Required,
            ..RequestRequirements::default()
        };
        let local = routing_score(Some(&profile()), &h, &req, &view());
        let remote = routing_score(Some(&profile()), &h, &req, &remote_view());
        assert!(local - remote > 0.7);
    }

    #[test]
    fn preferred_capability_matches_boost_score() {
        let h = ProviderHealth::default();
        let req = RequestRequirements {
            vision: Requirement::Preferred,
            ..RequestRequirements::default()
        };
        let mut with_vision = view();
        with_vision.vision = true;
        let scored_with = routing_score(Some(&profile()), &h, &req, &with_vision);
        let scored_without = routing_score(Some(&profile()), &h, &req, &view());
        assert!(scored_with > scored_without);
    }

    #[test]
    fn default_requirements_are_free_first_and_agnostic() {
        let req = RequestRequirements::default();
        assert_eq!(req.budget, Budget::Free);
        assert_eq!(req.privacy, Preference::Any);
        assert_eq!(req.vision, Requirement::None);
        assert_eq!(req.latency_ms, None);
    }

    #[test]
    fn from_capabilities_maps_agent_preferences() {
        use crate::providers::ModelCapability;
        let req = RequestRequirements::from_capabilities(&[
            ModelCapability::Local,
            ModelCapability::Vision,
            ModelCapability::Tools,
            ModelCapability::LongContext,
        ]);
        assert_eq!(req.privacy, Preference::Preferred);
        assert_eq!(req.vision, Requirement::Preferred);
        assert_eq!(req.tools, Requirement::Preferred);
        assert_eq!(req.long_context, Requirement::Preferred);
        assert_eq!(req.budget, Budget::Free);
    }

    #[test]
    fn profile_round_trips_through_json() {
        let json = serde_json::to_string(&profile()).unwrap();
        let back: QualificationProfile = serde_json::from_str(&json).unwrap();
        assert_eq!(back, profile());
    }
}
