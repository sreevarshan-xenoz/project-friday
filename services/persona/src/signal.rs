//! `Signal` — everything the Persona Engine observes. Two kinds today
//! (conversation intents and environment/activity contexts); the engine
//! treats both identically via the signal id plus an optional OCC appraisal.

use serde::{Deserialize, Serialize};

use crate::mood::Appraisal;

/// A signal into the Persona Engine.
///
/// `id` is matched against each persona's `affinity` map (e.g. `"coding"`,
/// `"casual"`, `"build_failed"`). An optional [`Appraisal`] rides along for
/// turns that carry emotional content (OCC → PAD). Serializes like the rest of
/// the engine's data so signals can cross the wire or be recorded.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Signal {
    pub id: String,
    pub appraisal: Option<Appraisal>,
}

impl Signal {
    /// A conversation-intent signal (from the intent detector).
    pub fn conversation(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            appraisal: None,
        }
    }

    /// An environment/activity signal (from the activity/environment detector:
    /// focused app, build result, media playing, …).
    pub fn activity(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            appraisal: None,
        }
    }

    /// A conversation signal carrying an emotional appraisal (OCC → PAD).
    pub fn emotional(id: impl Into<String>, appraisal: Appraisal) -> Self {
        Self {
            id: id.into(),
            appraisal: Some(appraisal),
        }
    }
}
