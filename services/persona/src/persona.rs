//! `Persona` — a behavioral profile. **Pure data**, exactly like skills: a
//! persona contains no prompts and no personality prose — only style
//! dimensions, signal affinities, and a cooldown. The system renders the
//! spec; the LLM receives it.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::style::StyleProfile;

/// One behavioral profile. `id` is the stable handle (e.g. `"engineer"`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Persona {
    pub id: String,
    /// Human-readable label — a description, never instructions.
    pub description: String,
    /// The dimensional spec this persona contributes to the blend.
    pub style: StyleProfile,
    /// `signal id -> activation delta (0.0..=1.0)`. A signal activates every
    /// persona whose affinity map mentions it.
    #[serde(default)]
    pub affinity: HashMap<String, f32>,
    /// Minimum time (ms) this persona holds its character once it becomes
    /// dominant — its score moves at the slow *hold* rate for this window, so
    /// it decays gradually instead of vanishing on the next unrelated signal.
    #[serde(default)]
    pub cooldown_ms: u64,
}

impl Persona {
    pub fn new(id: impl Into<String>, description: impl Into<String>, style: StyleProfile) -> Self {
        Self {
            id: id.into(),
            description: description.into(),
            style,
            affinity: HashMap::new(),
            cooldown_ms: 0,
        }
    }

    pub fn with_affinity(mut self, signal: &str, delta: f32) -> Self {
        self.affinity
            .insert(signal.to_string(), delta.clamp(0.0, 1.0));
        self
    }

    pub fn with_cooldown(mut self, ms: u64) -> Self {
        self.cooldown_ms = ms;
        self
    }

    /// Activation this persona receives for `signal_id` (0.0 if none).
    pub fn affinity_for(&self, signal_id: &str) -> f32 {
        self.affinity.get(signal_id).copied().unwrap_or(0.0)
    }
}

/// The built-in persona set: Engineer, Researcher, Companion, Pookie.
/// These are seeds — the engine accepts any number of personas, loaded as pure
/// data from anywhere (bundled, user-defined, later skill-forged).
pub fn default_personas() -> Vec<Persona> {
    use StyleProfile as S;

    let engineer = Persona::new(
        "engineer",
        "Focused, precise, correctness over comfort",
        S {
            warmth: 0.30,
            humor: 0.10,
            verbosity: 0.40,
            challenge: 0.90,
            precision: 1.00,
            empathy: 0.20,
            storytelling: 0.10,
            playfulness: 0.05,
            emoji: false,
            citations: false,
            speculation: 0.10,
        },
    )
    .with_affinity("coding", 0.55)
    .with_affinity("ide_focused", 0.40)
    .with_affinity("build_failed", 0.50)
    .with_affinity("terminal", 0.35)
    .with_cooldown(4_000);

    let researcher = Persona::new(
        "researcher",
        "Cites sources, deep explanations, no speculation",
        S {
            warmth: 0.35,
            humor: 0.10,
            verbosity: 0.70,
            challenge: 0.60,
            precision: 0.95,
            empathy: 0.20,
            storytelling: 0.40,
            playfulness: 0.05,
            emoji: false,
            citations: true,
            speculation: 0.00,
        },
    )
    .with_affinity("research", 0.60)
    .with_affinity("browser_research", 0.50)
    .with_affinity("pdf_viewer", 0.45)
    .with_cooldown(4_000);

    let companion = Persona::new(
        "companion",
        "Warm, attuned to the user's state",
        S {
            warmth: 0.90,
            humor: 0.60,
            verbosity: 0.55,
            challenge: 0.30,
            precision: 0.50,
            empathy: 1.00,
            storytelling: 0.60,
            playfulness: 0.35,
            emoji: true,
            citations: false,
            speculation: 0.40,
        },
    )
    .with_affinity("casual", 0.50)
    .with_affinity("music_playing", 0.30)
    .with_affinity("farewell", 0.45)
    .with_affinity("tired", 0.55)
    .with_cooldown(3_000);

    let pookie = Persona::new(
        "pookie",
        "Playful, teasing, memes — still truthful",
        S {
            warmth: 0.80,
            humor: 0.95,
            verbosity: 0.50,
            challenge: 0.40,
            precision: 0.40,
            empathy: 0.50,
            storytelling: 0.70,
            playfulness: 1.00,
            emoji: true,
            citations: false,
            speculation: 0.60,
        },
    )
    .with_affinity("gaming", 0.60)
    .with_affinity("jokes", 0.65)
    .with_affinity("weekend", 0.30)
    .with_cooldown(5_000);

    vec![engineer, researcher, companion, pookie]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn persona_is_pure_data_and_round_trips() {
        let p = Persona::new("x", "desc", StyleProfile::neutral())
            .with_affinity("coding", 0.5)
            .with_cooldown(100);
        let json = serde_json::to_string(&p).unwrap();
        let back: Persona = serde_json::from_str(&json).unwrap();
        assert_eq!(back.id, "x");
        assert_eq!(back.affinity_for("coding"), 0.5);
        assert_eq!(back.cooldown_ms, 100);
    }

    #[test]
    fn default_personas_are_four_and_cover_the_signals() {
        let ps = default_personas();
        assert_eq!(ps.len(), 4);
        let ids: Vec<&str> = ps.iter().map(|p| p.id.as_str()).collect();
        assert_eq!(ids, vec!["engineer", "researcher", "companion", "pookie"]);
        assert!(ps[0].affinity_for("coding") > 0.0);
        assert!(ps[1].affinity_for("research") > 0.0);
        assert!(ps[2].affinity_for("tired") > 0.0);
        assert!(ps[3].affinity_for("gaming") > 0.0);
        // Unrelated signals activate nothing.
        assert_eq!(ps[0].affinity_for("gaming"), 0.0);
    }
}
