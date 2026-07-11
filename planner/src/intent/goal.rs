use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;

/// A resolved user goal. The Intent Engine produces this; everything downstream
/// consumes it. `intent` is a stable capability key (e.g. `browser.search`);
/// `entities` are the extracted parameters.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Intent {
    pub intent: String,
    pub entities: HashMap<String, Value>,
    pub raw: String,
}

impl Intent {
    pub fn new(intent: impl Into<String>, raw: impl Into<String>) -> Self {
        Self {
            intent: intent.into(),
            entities: HashMap::new(),
            raw: raw.into(),
        }
    }

    pub fn with_entity(mut self, key: &str, value: Value) -> Self {
        self.entities.insert(key.to_string(), value);
        self
    }

    pub fn entity_str(&self, key: &str) -> Option<&str> {
        self.entities.get(key).and_then(|v| v.as_str())
    }
}
