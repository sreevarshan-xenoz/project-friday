//! `Mood` — a continuous PAD emotional vector (Pleasure, Arousal, Dominance),
//! fed by OCC-style appraisals and decaying toward neutral.
//!
//! The consciousness paper (*Simulating Consciousness in AI Agents*)
//! proposes the OCC appraisal model feeding a continuous PAD vector that
//! influences voice, pacing, autonomy and interface behavior. This module is
//! that mechanism, implemented as pure, testable state.

use serde::{Deserialize, Serialize};

/// Continuous three-dimensional affect. Each axis is `-1.0..=1.0`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Mood {
    pub pleasure: f32,
    pub arousal: f32,
    pub dominance: f32,
}

impl Mood {
    pub fn neutral() -> Self {
        Self {
            pleasure: 0.0,
            arousal: 0.0,
            dominance: 0.0,
        }
    }

    /// OCC appraisal → PAD update. The OCC model classifies appraisals into
    /// events (desirability), agents (praiseworthiness) and objects (appeal);
    /// each contributes to the vector with a fixed mapping and intensity.
    pub fn appraise(&mut self, a: &Appraisal) {
        self.pleasure += 0.5 * a.desirability + 0.25 * a.praiseworthiness + 0.35 * a.appeal;
        self.arousal += 0.30 * a.desirability.abs() + 0.35 * a.appeal.abs();
        self.dominance += 0.5 * a.praiseworthiness;
        self.clamp();
    }

    /// Exponential decay toward neutral: after `dt_ms` of half-life
    /// `half_life_ms`, the deviation from neutral halves.
    pub fn decay(&mut self, dt_ms: u64, half_life_ms: u64) {
        if half_life_ms == 0 {
            return;
        }
        let factor = (-(dt_ms as f32) * std::f32::consts::LN_2 / half_life_ms as f32).exp();
        self.pleasure *= factor;
        self.arousal *= factor;
        self.dominance *= factor;
    }

    pub fn clamp(&mut self) {
        self.pleasure = self.pleasure.clamp(-1.0, 1.0);
        self.arousal = self.arousal.clamp(-1.0, 1.0);
        self.dominance = self.dominance.clamp(-1.0, 1.0);
    }
}

impl Default for Mood {
    fn default() -> Self {
        Self::neutral()
    }
}

/// One OCC appraisal — how the system judges the current situation across the
/// three appraisal classes. All axes `-1.0..=1.0`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Appraisal {
    /// How good/bad the event is for FRIDAY's goals (pleased..displeased).
    pub desirability: f32,
    /// How praiseworthy/blameworthy the responsible agent is (approving..disapproving).
    pub praiseworthiness: f32,
    /// How appealing/unappealing the relevant object is (liking..disliking).
    pub appeal: f32,
}

impl Appraisal {
    pub fn new(desirability: f32, praiseworthiness: f32, appeal: f32) -> Self {
        Self {
            desirability: desirability.clamp(-1.0, 1.0),
            praiseworthiness: praiseworthiness.clamp(-1.0, 1.0),
            appeal: appeal.clamp(-1.0, 1.0),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn neutral_is_zero() {
        let m = Mood::neutral();
        assert_eq!(
            m,
            Mood {
                pleasure: 0.0,
                arousal: 0.0,
                dominance: 0.0
            }
        );
    }

    #[test]
    fn good_event_raises_pleasure() {
        let mut m = Mood::neutral();
        m.appraise(&Appraisal::new(1.0, 0.0, 0.0));
        assert!(m.pleasure > 0.0, "desirability must raise pleasure: {m:?}");
        assert!(m.arousal > 0.0);
    }

    #[test]
    fn blame_lowers_dominance() {
        let mut m = Mood::neutral();
        m.appraise(&Appraisal::new(0.0, -1.0, 0.0));
        assert!(
            m.dominance < 0.0,
            "blameworthiness must lower dominance: {m:?}"
        );
    }

    #[test]
    fn decay_returns_toward_neutral() {
        let mut m = Mood::neutral();
        m.appraise(&Appraisal::new(1.0, 1.0, 1.0));
        let before = m.pleasure;
        m.decay(30_000, 30_000); // one half-life
        let after = m.pleasure;
        assert!(after.abs() < before.abs(), "{before} -> {after}");
        assert!(after.abs() > 0.0, "one half-life halves, does not zero");
    }

    #[test]
    fn stays_bounded_under_repeated_appraisals() {
        let mut m = Mood::neutral();
        for _ in 0..100 {
            m.appraise(&Appraisal::new(1.0, 1.0, 1.0));
        }
        assert!(m.pleasure <= 1.0 && m.arousal <= 1.0 && m.dominance <= 1.0);
        assert!(m.pleasure >= -1.0 && m.arousal >= -1.0 && m.dominance >= -1.0);
    }
}
