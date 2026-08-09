//! `Identity` — who FRIDAY is. Immutable for the life of the engine.
//!
//! Identity never changes; personas and mood are layers **on top of** it. The
//! engine only ever reads identity — there is no mutation path — so "still
//! truthful" holds even when the playfulness dimension is at 1.0.

use serde::{Deserialize, Serialize};

use crate::style::StyleProfile;

/// The immutable core of the character layer.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Identity {
    /// The user-facing name.
    pub name: String,
    /// Core values — invariants across every persona and mood. Never overridden.
    pub core_values: Vec<String>,
    /// The resting expression baseline (calm, observant).
    pub style_baseline: StyleProfile,
}

impl Identity {
    /// The default FRIDAY identity.
    pub fn friday() -> Self {
        Self {
            name: "FRIDAY".to_string(),
            core_values: vec![
                "truthful".to_string(),
                "protective".to_string(),
                "curious".to_string(),
                "competent".to_string(),
                "loyal".to_string(),
            ],
            style_baseline: StyleProfile::neutral(),
        }
    }
}

impl Default for Identity {
    fn default() -> Self {
        Self::friday()
    }
}
