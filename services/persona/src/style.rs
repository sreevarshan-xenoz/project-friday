//! `StyleProfile` — the dimensional behavioral spec of a persona.
//!
//! A profile is a vector of named dimensions, **never prompt text**. The LLM
//! receives the *rendered* spec from the system (via
//! [`crate::engine::PersonaEngine::render_instruction`]), so swapping the
//! underlying model changes the identity layer zero times.

use serde::{Deserialize, Serialize};

use crate::mood::Mood;

/// A persona's style: continuous dimensions (all `0.0..=1.0`) plus two
/// boolean switches. Pure data — no instructions, no examples, no prose.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct StyleProfile {
    /// Warmth / closeness of the relationship expression.
    pub warmth: f32,
    /// Willingness to be funny.
    pub humor: f32,
    /// 0.0 = terse, 1.0 = expansive.
    pub verbosity: f32,
    /// Pushes back on the user's assumptions (0 = accommodating, 1 = challenges hard).
    pub challenge: f32,
    /// Demands exactness in claims (0 = loose, 1 = exact).
    pub precision: f32,
    /// Attunement to the user's emotional state.
    pub empathy: f32,
    /// Uses narrative and examples over bare bullet lists.
    pub storytelling: f32,
    /// Playfulness (teasing, silliness, memes).
    pub playfulness: f32,
    /// Whether to use emoji.
    pub emoji: bool,
    /// Whether to cite sources.
    pub citations: bool,
    /// Freedom to speculate beyond evidence (0 = forbidden, 1 = free).
    pub speculation: f32,
}

impl StyleProfile {
    /// The neutral resting profile: calm, observant, low-key.
    pub fn neutral() -> Self {
        Self {
            warmth: 0.45,
            humor: 0.20,
            verbosity: 0.50,
            challenge: 0.50,
            precision: 0.60,
            empathy: 0.40,
            storytelling: 0.30,
            playfulness: 0.20,
            emoji: false,
            citations: false,
            speculation: 0.50,
        }
    }

    /// Weighted blend toward `other` (`w` in `0.0..=1.0`). Booleans follow the
    /// weighted-average threshold (>= 0.5 wins), so a 60/40 blend flips a
    /// switch only when the majority persona disagrees.
    pub fn blend(&self, other: &Self, w: f32) -> Self {
        let w = w.clamp(0.0, 1.0);
        let mix = |a: f32, b: f32| a + (b - a) * w;
        let truth = |a: bool, b: bool| mix(a as u8 as f32, b as u8 as f32) >= 0.5;
        Self {
            warmth: mix(self.warmth, other.warmth).clamp(0.0, 1.0),
            humor: mix(self.humor, other.humor).clamp(0.0, 1.0),
            verbosity: mix(self.verbosity, other.verbosity).clamp(0.0, 1.0),
            challenge: mix(self.challenge, other.challenge).clamp(0.0, 1.0),
            precision: mix(self.precision, other.precision).clamp(0.0, 1.0),
            empathy: mix(self.empathy, other.empathy).clamp(0.0, 1.0),
            storytelling: mix(self.storytelling, other.storytelling).clamp(0.0, 1.0),
            playfulness: mix(self.playfulness, other.playfulness).clamp(0.0, 1.0),
            emoji: truth(self.emoji, other.emoji),
            citations: truth(self.citations, other.citations),
            speculation: mix(self.speculation, other.speculation).clamp(0.0, 1.0),
        }
    }

    /// Mood modulates expression (the consciousness-paper seam): pleasure
    /// warms the tone, arousal expands pacing, dominance sharpens autonomy.
    /// Deltas are small and clamped so a passing mood never rewrites a
    /// persona's character.
    pub fn adjusted_for_mood(&self, mood: &Mood) -> Self {
        Self {
            warmth: (self.warmth + mood.pleasure * 0.10).clamp(0.0, 1.0),
            playfulness: (self.playfulness + mood.pleasure * 0.08).clamp(0.0, 1.0),
            verbosity: (self.verbosity + mood.arousal * 0.15).clamp(0.0, 1.0),
            challenge: (self.challenge + mood.dominance * 0.15).clamp(0.0, 1.0),
            precision: (self.precision + mood.dominance * 0.05).clamp(0.0, 1.0),
            ..*self
        }
    }

    /// Render the profile as a model-agnostic behavioral spec block — the text
    /// the planner hands to whichever LLM is active. The LLM never decides
    /// this; it receives it.
    pub fn render(&self) -> String {
        use std::fmt::Write as _;
        let mut out = String::new();
        let _ = writeln!(out, "  warmth: {:.2}", self.warmth);
        let _ = writeln!(out, "  humor: {:.2}", self.humor);
        let _ = writeln!(out, "  verbosity: {:.2}", self.verbosity);
        let _ = writeln!(out, "  challenge: {:.2}", self.challenge);
        let _ = writeln!(out, "  precision: {:.2}", self.precision);
        let _ = writeln!(out, "  empathy: {:.2}", self.empathy);
        let _ = writeln!(out, "  storytelling: {:.2}", self.storytelling);
        let _ = writeln!(out, "  playfulness: {:.2}", self.playfulness);
        let _ = writeln!(out, "  speculation: {:.2}", self.speculation);
        let _ = writeln!(
            out,
            "  emoji: {}",
            if self.emoji { "enabled" } else { "disabled" }
        );
        let _ = writeln!(
            out,
            "  citations: {}",
            if self.citations {
                "required"
            } else {
                "optional"
            }
        );
        out
    }
}

impl Default for StyleProfile {
    fn default() -> Self {
        Self::neutral()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn neutral_is_midpoint_and_sane() {
        let n = StyleProfile::neutral();
        assert!(!n.emoji && !n.citations);
        assert!((0.0..=1.0).contains(&n.warmth));
        assert_eq!(n, StyleProfile::default());
    }

    #[test]
    fn blend_moves_toward_other() {
        let a = StyleProfile::neutral();
        let b = StyleProfile {
            warmth: 1.0,
            humor: 1.0,
            ..StyleProfile::neutral()
        };
        let half = a.blend(&b, 0.5);
        assert!((half.warmth - 0.725).abs() < 1e-3, "warmth {}", half.warmth);
        assert!((half.humor - 0.6).abs() < 1e-3, "humor {}", half.humor);
    }

    #[test]
    fn blend_boolean_threshold() {
        let off = StyleProfile::neutral(); // emoji false
        let on = StyleProfile {
            emoji: true,
            ..StyleProfile::neutral()
        };
        assert!(!off.blend(&on, 0.4).emoji);
        assert!(off.blend(&on, 0.6).emoji);
    }

    #[test]
    fn mood_adjusts_expression_without_rewriting_character() {
        let base = StyleProfile::neutral();
        let high = Mood {
            pleasure: 1.0,
            arousal: 1.0,
            dominance: 1.0,
        };
        let adjusted = base.adjusted_for_mood(&high);
        assert!(adjusted.warmth > base.warmth);
        assert!(adjusted.verbosity > base.verbosity);
        assert!(adjusted.challenge > base.challenge);
        // Still bounded.
        assert!((0.0..=1.0).contains(&adjusted.warmth));
    }

    #[test]
    fn render_is_a_spec_block_not_prose() {
        let r = StyleProfile::neutral().render();
        assert!(r.contains("warmth:"));
        assert!(r.contains("emoji: disabled"));
        assert!(!r.contains("you should"));
    }
}
