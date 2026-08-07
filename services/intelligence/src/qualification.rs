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

/// Combine an immutable qualification profile with current runtime health into a
/// single routing score (0.0..=1.0).
///
/// Weights are policy and live HERE, never in the profile file, so re-weighting
/// never invalidates historical benchmark data. Missing measurements fall back
/// to a neutral midpoint rather than biasing the score.
pub fn routing_score(profile: &QualificationProfile, health: &ProviderHealth) -> f64 {
    let quality = profile.json_accuracy.unwrap_or(0.5);
    let speed = speed_component(profile, health);
    let live = (health.success_rate * 0.5 + health.availability * 0.5).clamp(0.0, 1.0);
    (quality * 0.5) + (speed * 0.3) + (live * 0.2)
}

fn speed_component(profile: &QualificationProfile, health: &ProviderHealth) -> f64 {
    let ttft = health.moving_ttft_ms.or(profile.ttft_ms);
    let tps = health
        .moving_tokens_per_second
        .or(profile.tokens_per_second);
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
        let good = routing_score(&p, &h);
        p.json_accuracy = Some(0.0);
        let bad = routing_score(&p, &h);
        assert!(good > bad);
    }

    #[test]
    fn better_health_raises_score() {
        let h = ProviderHealth {
            success_rate: 1.0,
            availability: 1.0,
            ..ProviderHealth::default()
        };
        let good = routing_score(&profile(), &h);
        let bad = routing_score(&profile(), &ProviderHealth::default());
        assert!(good > bad);
    }

    #[test]
    fn missing_measurements_do_not_panic_and_stay_finite() {
        let mut p = profile();
        p.json_accuracy = None;
        p.ttft_ms = None;
        p.tokens_per_second = None;
        let s = routing_score(&p, &ProviderHealth::default());
        assert!(s.is_finite());
        assert!((0.0..=1.0).contains(&s));
    }

    #[test]
    fn profile_round_trips_through_json() {
        let json = serde_json::to_string(&profile()).unwrap();
        let back: QualificationProfile = serde_json::from_str(&json).unwrap();
        assert_eq!(back, profile());
    }
}
