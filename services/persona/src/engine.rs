//! `PersonaEngine` — the character layer between perception and the planner.
//!
//! ```text
//! Sensors → Perception → Intent / Activity Detectors
//!                                    │  Signal
//!                                    ▼
//!                            PersonaEngine
//!                                    │  StyleSpec (deterministic)
//!                                    ▼
//!                           Planner → LLM → Actions
//! ```
//!
//! The engine decides **how FRIDAY expresses herself**, never *what to do*.
//! It is fully deterministic — no LLM inside. The LLM receives the rendered
//! behavioral spec (see [`PersonaEngine::render_instruction`]), so identity
//! survives a brain transplant.
//!
//! ## Dynamics (all smooth — never a light switch)
//!
//! * **Energy** — each persona accumulates activation energy from signals
//!   (`affinity × [`SIGNAL_WEIGHT`]`) and leaks it with **elapsed time**
//!   ([`LEAK`] per second), so a long silence cools a persona the way it cools
//!   a mood. Repeated signals build a long-lived context (the deep-research
//!   state); a single off-topic line only nudges.
//! * **Hold (cooldown)** — the last-dominant persona leaks at [`HOLD_LEAK`]
//!   for its `cooldown_ms` window, so it decays gradually instead of vanishing
//!   on the next unrelated signal, and cannot re-spike instantly.
//! * **Mood** — OCC appraisals move the PAD vector ([`crate::mood::Mood`]),
//!   which decays toward neutral and subtly modulates the blended style.

use serde::{Deserialize, Serialize};

use crate::identity::Identity;
use crate::mood::{Appraisal, Mood};
use crate::persona::Persona;
use crate::signal::Signal;
use crate::style::StyleProfile;

/// How much activation one signal contributes (`affinity × SIGNAL_WEIGHT`).
const SIGNAL_WEIGHT: f32 = 0.5;
/// Per-second leak of idle persona energy. Energy decays with **elapsed time**
/// (`LEAK^(dt_ms/1000)`), not with observation count, so a long silence cools
/// a persona the same way it cools a mood — the engine has one clock.
const LEAK: f32 = 0.8;
/// Per-second leak for the recently-dominant persona during its cooldown
/// window — it holds its character (~7%/s instead of ~20%/s).
const HOLD_LEAK: f32 = 0.93;
/// The reference cadence the leak constants are expressed over (1 second).
const LEAK_REFERENCE_MS: f32 = 1_000.0;

/// The deterministic output handed to the planner/LLM.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StyleSpec {
    /// Dominant persona id.
    pub persona: String,
    /// Normalized blend weights, descending (e.g. `engineer 0.70` …). Personas
    /// below a 2% share are dropped and the kept set renormalized, so the
    /// blend reads like a real character mix, not a 4-item dump.
    pub blend: Vec<(String, f32)>,
    /// The blended, mood-adjusted style.
    pub style: StyleProfile,
    /// Current mood.
    pub mood: Mood,
}

/// Engine state: identity (immutable) + persona energy + mood + dominance clock.
pub struct PersonaEngine {
    identity: Identity,
    personas: Vec<Persona>,
    /// Per-persona activation energy (0.0..=1.0).
    energy: Vec<f32>,
    /// The last-dominant persona index and when it became dominant.
    last_dominant: Option<(usize, u64)>,
    mood: Mood,
    last_ts_ms: u64,
}

impl PersonaEngine {
    pub fn new(identity: Identity, personas: Vec<Persona>) -> Self {
        let n = personas.len();
        Self {
            identity,
            personas,
            energy: vec![0.0; n],
            last_dominant: None,
            mood: Mood::neutral(),
            last_ts_ms: 0,
        }
    }

    /// The default FRIDAY engine: default identity + the built-in personas.
    pub fn friday() -> Self {
        Self::new(Identity::friday(), crate::persona::default_personas())
    }

    /// Identity is immutable — the engine only ever exposes it by reference.
    pub fn identity(&self) -> &Identity {
        &self.identity
    }

    pub fn mood(&self) -> Mood {
        self.mood
    }

    /// Advance time and apply an observation: energy leaks with elapsed time,
    /// an optional appraisal moves the PAD vector, and each persona's energy
    /// accumulates the signal's affinity.
    pub fn observe(&mut self, signal: &Signal, now_ms: u64) {
        self.tick(now_ms);
        if let Some(a) = &signal.appraisal {
            self.mood.appraise(a);
        }
        for (i, p) in self.personas.iter().enumerate() {
            let mut e = self.energy[i];
            e += p.affinity_for(&signal.id) * SIGNAL_WEIGHT;
            self.energy[i] = e.clamp(0.0, 1.0);
        }
        // Track dominance (only when something is actually activated, so a
        // signal that activates nothing does not crown a persona).
        if self.energy.iter().any(|e| *e > 0.0) {
            let argmax = argmax_index(&self.energy);
            if Some(argmax) != self.last_dominant.map(|(i, _)| i) {
                self.last_dominant = Some((argmax, now_ms));
            }
        }
    }

    /// Direct OCC appraisal → mood, without a persona signal.
    pub fn appraise(&mut self, a: &Appraisal) {
        self.mood.appraise(a);
    }

    /// Advance the internal clock: mood decays toward neutral and every
    /// persona's energy leaks exponentially with elapsed time (idle time cools
    /// energy the same way it cools a mood). The recently-dominant persona
    /// leaks at the slower hold rate while inside its cooldown window.
    pub fn tick(&mut self, now_ms: u64) {
        let dt_ms = now_ms.saturating_sub(self.last_ts_ms);
        self.last_ts_ms = now_ms;
        if dt_ms == 0 {
            return;
        }
        self.mood.decay(dt_ms, 30_000);
        // Precompute hold flags so the mutable energy loop never borrows self.
        let hold_flags: Vec<bool> = (0..self.personas.len())
            .map(|i| self.in_hold(i, now_ms))
            .collect();
        for (i, e) in self.energy.iter_mut().enumerate() {
            let rate = if hold_flags.get(i).copied().unwrap_or(false) {
                HOLD_LEAK
            } else {
                LEAK
            };
            let factor = rate.powf(dt_ms as f32 / LEAK_REFERENCE_MS);
            *e *= factor;
        }
    }

    /// The current deterministic output: dominant persona, normalized blend,
    /// identity-baselined and mood-adjusted style.
    pub fn select(&self) -> StyleSpec {
        let total: f32 = self.energy.iter().sum();
        if total <= 0.0 {
            // Nothing has activated yet (fresh engine or full cool-down): there
            // is no dominant persona. Report the identity baseline rather than
            // crowning the first persona at 100%.
            return StyleSpec {
                persona: String::new(),
                blend: Vec::new(),
                style: self.identity.style_baseline.adjusted_for_mood(&self.mood),
                mood: self.mood,
            };
        }
        let mut ranked: Vec<(usize, f32)> = self.energy.iter().copied().enumerate().collect();
        ranked.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

        // Blended style: identity baseline weighted toward each persona's
        // profile by its share of energy.
        let mut style = self.identity.style_baseline;
        for (i, e) in &ranked {
            style = style.blend(&self.personas[*i].style, e / total);
        }
        let style = style.adjusted_for_mood(&self.mood);

        // Reportable blend: drop shares < 2%, renormalize the kept set.
        let full: Vec<(String, f32)> = ranked
            .iter()
            .map(|(i, e)| (self.personas[*i].id.clone(), e / total))
            .collect();
        let kept: Vec<(String, f32)> = full.iter().filter(|(_, w)| *w >= 0.02).cloned().collect();
        let kept_total: f32 = kept.iter().map(|(_, w)| *w).sum();
        let blend: Vec<(String, f32)> = if kept_total > 0.0 {
            kept.into_iter()
                .map(|(id, w)| (id, w / kept_total))
                .collect()
        } else {
            vec![(full[0].0.clone(), 1.0)]
        };

        let persona = blend[0].0.clone();
        StyleSpec {
            persona,
            blend,
            style,
            mood: self.mood,
        }
    }

    /// The model-agnostic behavioral block handed to whichever LLM is active.
    /// Identity (never overridden) + blend + mood-adjusted style spec.
    pub fn render_instruction(&self) -> String {
        use std::fmt::Write as _;
        let spec = self.select();
        let mut out = String::new();
        let _ = writeln!(out, "You are {}.", self.identity.name);
        let _ = writeln!(
            out,
            "Core values (never overridden): {}",
            self.identity.core_values.join(", ")
        );
        let _ = writeln!(out, "Behavioral spec for this response:");
        let _ = writeln!(out, "  persona: {}", spec.persona);
        let blend: Vec<String> = spec
            .blend
            .iter()
            .map(|(id, w)| format!("{id} {w:.2}"))
            .collect();
        let _ = writeln!(out, "  blend: {}", blend.join(", "));
        let _ = write!(out, "{}", spec.style.render());
        let _ = writeln!(
            out,
            "  mood: pleasure {:.2} / arousal {:.2} / dominance {:.2}",
            spec.mood.pleasure, spec.mood.arousal, spec.mood.dominance
        );
        out
    }

    /// Whether `idx` is inside its cooldown window since last becoming dominant.
    fn in_hold(&self, idx: usize, now_ms: u64) -> bool {
        match self.last_dominant {
            Some((dom, at)) if dom == idx => {
                let cd = self.personas[idx].cooldown_ms;
                cd > 0 && now_ms.saturating_sub(at) < cd
            }
            _ => false,
        }
    }
}

/// Index of the highest value (first on ties).
fn argmax_index(values: &[f32]) -> usize {
    values
        .iter()
        .enumerate()
        .max_by(|a, b| a.1.partial_cmp(b.1).unwrap_or(std::cmp::Ordering::Equal))
        .map(|(i, _)| i)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Appraisal;

    fn research_context(engine: &mut PersonaEngine) {
        for t in [0, 1_000, 2_000] {
            engine.observe(&Signal::conversation("research"), t);
        }
    }

    #[test]
    fn one_unrelated_signal_does_not_flip_dominance() {
        let mut engine = PersonaEngine::friday();
        research_context(&mut engine);
        // "lol that's crazy" right after a deep-research session.
        engine.observe(&Signal::conversation("casual"), 3_000);
        assert_eq!(
            engine.select().persona,
            "researcher",
            "a single casual line must not rip the engineer/researcher out of deep research"
        );
    }

    #[test]
    fn transition_to_a_new_persona_is_gradual() {
        let mut engine = PersonaEngine::friday();
        research_context(&mut engine);
        engine.observe(&Signal::conversation("casual"), 3_000);
        assert_eq!(
            engine.select().persona,
            "researcher",
            "first casual: still research"
        );
        engine.observe(&Signal::conversation("casual"), 4_000);
        assert_eq!(
            engine.select().persona,
            "researcher",
            "second casual: still research"
        );
        engine.observe(&Signal::conversation("casual"), 5_000);
        assert_eq!(
            engine.select().persona,
            "companion",
            "third casual: warm drift completes"
        );
    }

    #[test]
    fn cooldown_prevents_instant_reengagement() {
        let mut engine = PersonaEngine::friday();
        // Companion becomes dominant.
        for t in [0, 1_000, 2_000, 3_000] {
            engine.observe(&Signal::conversation("casual"), t);
        }
        assert_eq!(engine.select().persona, "companion");
        // One research signal within companion's cooldown: no instant flip.
        engine.observe(&Signal::conversation("research"), 4_000);
        assert_eq!(
            engine.select().persona,
            "companion",
            "a single research line must not yank the companion away instantly"
        );
        // Sustained research wins once the hold decays.
        for t in [5_000, 6_000, 7_000] {
            engine.observe(&Signal::conversation("research"), t);
        }
        assert_eq!(engine.select().persona, "researcher");
    }

    #[test]
    fn identity_is_immutable_under_abuse() {
        let mut engine = PersonaEngine::friday();
        for t in 0..50 {
            engine.observe(
                &Signal::emotional("coding", Appraisal::new(1.0, 1.0, 1.0)),
                t * 100,
            );
        }
        let id = engine.identity();
        assert_eq!(id.name, "FRIDAY");
        assert_eq!(id.core_values.len(), 5);
        assert!(id.core_values.contains(&"truthful".to_string()));
    }

    #[test]
    fn still_truthful_even_under_pookie() {
        let mut engine = PersonaEngine::friday();
        for t in [0, 1_000, 2_000, 3_000] {
            engine.observe(&Signal::activity("gaming"), t);
        }
        assert_eq!(engine.select().persona, "pookie");
        let instruction = engine.render_instruction();
        assert!(
            instruction.contains("Core values (never overridden): truthful"),
            "identity core values must survive playfulness=1.0:\n{instruction}"
        );
        assert!(engine
            .identity()
            .core_values
            .contains(&"truthful".to_string()));
    }

    #[test]
    fn blend_weights_normalize() {
        let mut engine = PersonaEngine::friday();
        research_context(&mut engine);
        engine.observe(&Signal::conversation("casual"), 3_000);
        let spec = engine.select();
        let sum: f32 = spec.blend.iter().map(|(_, w)| *w).sum();
        assert!(
            (sum - 1.0).abs() < 1e-3,
            "blend must normalize to 1.0, got {sum}: {:?}",
            spec.blend
        );
        assert_eq!(spec.blend[0].0, "researcher");
    }

    #[test]
    fn mood_moves_on_appraisal_and_decays_via_tick() {
        let mut engine = PersonaEngine::friday();
        engine.observe(
            &Signal::emotional("farewell", Appraisal::new(0.8, 0.5, 0.3)),
            0,
        );
        let happy = engine.mood().pleasure;
        assert!(happy > 0.0, "positive appraisal must raise pleasure");
        engine.tick(30_000); // one half-life
        let after = engine.mood().pleasure;
        assert!(
            after.abs() < happy.abs(),
            "mood must decay toward neutral: {happy} -> {after}"
        );
    }

    #[test]
    fn environment_signal_shifts_persona() {
        let mut engine = PersonaEngine::friday();
        // VS Code focused, build failed, terminal active.
        engine.observe(&Signal::activity("ide_focused"), 0);
        engine.observe(&Signal::activity("build_failed"), 1_000);
        engine.observe(&Signal::activity("terminal"), 2_000);
        assert_eq!(
            engine.select().persona,
            "engineer",
            "the environment itself must be a persona signal"
        );
    }

    #[test]
    fn render_instruction_carries_identity_blend_and_mood() {
        let mut engine = PersonaEngine::friday();
        research_context(&mut engine);
        engine.observe(
            &Signal::emotional("research", Appraisal::new(0.5, 0.4, 0.2)),
            3_000,
        );
        let out = engine.render_instruction();
        assert!(out.contains("You are FRIDAY."));
        assert!(out.contains("persona: researcher"));
        assert!(out.contains("blend:"));
        assert!(
            out.contains("citations: required"),
            "researcher demands citations:\n{out}"
        );
        assert!(out.contains("mood: pleasure"));
    }

    #[test]
    fn long_silence_cools_energy_like_mood() {
        let mut engine = PersonaEngine::friday();
        research_context(&mut engine);
        // A ~1-minute gap: energy must leak with elapsed time (one clock),
        // not stay frozen at the last signal's value.
        engine.observe(&Signal::conversation("casual"), 60_000);
        assert_eq!(
            engine.select().persona,
            "companion",
            "the deep-research energy must have cooled during the silence"
        );
    }

    #[test]
    fn fresh_engine_has_no_phantom_dominant() {
        let engine = PersonaEngine::friday();
        let spec = engine.select();
        assert_eq!(spec.persona, "", "nothing observed → no dominant persona");
        assert!(spec.blend.is_empty());
    }

    #[test]
    fn empty_engine_select_is_safe() {
        let engine = PersonaEngine::new(Identity::friday(), vec![]);
        let spec = engine.select();
        assert_eq!(spec.persona, "");
        assert!(spec.blend.is_empty());
    }

    #[test]
    fn signal_round_trips_json() {
        let s = Signal::emotional("farewell", Appraisal::new(0.5, 0.2, 0.1));
        let json = serde_json::to_string(&s).unwrap();
        let back: Signal = serde_json::from_str(&json).unwrap();
        assert_eq!(back.id, "farewell");
        assert_eq!(back.appraisal.unwrap(), Appraisal::new(0.5, 0.2, 0.1));
    }
}
