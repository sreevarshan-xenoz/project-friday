//! FRIDAY Persona Engine — the **character layer** between perception and the
//! planner.
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
//! The engine decides **how FRIDAY expresses herself**, never *what to do*
//! (the planner owns that). It is fully deterministic — there is no LLM
//! inside, and the LLM never decides the persona: it **receives** the rendered
//! behavioral spec, so identity survives a brain transplant (Gemma today,
//! Qwen/Claude/GPT tomorrow).
//!
//! One identity, many personas, one continuous mood:
//!
//! * [`Identity`] — who FRIDAY is. Immutable: core values (truthful,
//!   protective, curious, competent, loyal) are never overridden by any
//!   persona or mood. "Still truthful" holds even when playfulness is 1.0.
//! * [`Persona`] — a behavioral profile. **Pure data** (exactly like skills:
//!   no prompts, no personality text — only dimensions, signal affinities and
//!   a cooldown). Blends weight the final style; nothing is a hard switch.
//! * [`Mood`] — a continuous PAD vector (Pleasure, Arousal, Dominance) fed by
//!   OCC-style [`Appraisal`]s (the consciousness-paper mechanism) and decaying
//!   toward neutral. Mood subtly modulates the blended style (voice, pacing,
//!   autonomy) without rewriting the persona's character.
//!
//! The engine's [`PersonaEngine::render_instruction`] emits the
//! model-agnostic behavioral block the planner passes to whichever LLM is
//! active.
//!
//! ```rust,no_run
//! use friday_persona::{PersonaEngine, Signal};
//!
//! let mut engine = PersonaEngine::friday();
//! engine.observe(&Signal::activity("ide_focused"), 0);
//! engine.observe(&Signal::conversation("coding"), 1_000);
//! let spec = engine.select();
//! assert_eq!(spec.persona, "engineer");
//! ```

pub mod engine;
pub mod identity;
pub mod mood;
pub mod persona;
pub mod signal;
pub mod style;

pub use engine::{PersonaEngine, StyleSpec};
pub use identity::Identity;
pub use mood::{Appraisal, Mood};
pub use persona::{default_personas, Persona};
pub use signal::Signal;
pub use style::StyleProfile;
