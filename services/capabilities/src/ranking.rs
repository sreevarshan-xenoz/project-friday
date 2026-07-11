//! Provider ranking. Collapses the rich capability fields into a single 0..100
//! score. Weights are centralized and tunable; the planner just sorts by score.

use crate::model::Capability;

/// Higher is better. See `docs/CapabilityIntelligence.md` for the worked example
/// (Browser `open_url` → 98, Windows `open_url` → 81).
///
/// ```text
/// score = reliability * 100 - latency_ms * 0.05 - cost * 1.0
/// ```
///
/// `confidence` is tracked for availability decisions, not the score, keeping
/// the formula transparent and tunable.
pub fn score(cap: &Capability) -> f64 {
    let s = cap.reliability * 100.0 - cap.latency_ms as f64 * 0.05 - cap.cost * 1.0;
    s.clamp(0.0, 100.0)
}
