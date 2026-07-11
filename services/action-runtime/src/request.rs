//! Normalized request/result contract shared by the Action Runtime and every
//! capability adapter.
//!
//! The runtime knows nothing about what an action *does*; it only ferries an
//! [`ActionRequest`] to the right [`crate::adapters::ActionAdapter`] and normalizes
//! the [`ActionResult`] back onto the bus. This is what keeps terminals, the
//! filesystem, the browser, etc. swappable and the runtime decoupled.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::Duration;
use uuid::Uuid;

mod serde_duration {
    use serde::{Deserialize, Deserializer, Serializer};
    use std::time::Duration;

    pub fn serialize<S: Serializer>(d: &Option<Duration>, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_some(&d.map(|d| d.as_millis() as u64))
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Option<Duration>, D::Error> {
        let opt = Option::<u64>::deserialize(d)?;
        Ok(opt.map(Duration::from_millis))
    }
}

/// A single normalized action to be executed by an adapter.
///
/// It is deliberately generic: `tool` selects the adapter (e.g. `filesystem`),
/// `action` selects the skill within it (e.g. `create_file`), and `parameters`
/// carries whatever JSON the skill needs. The runtime never interprets the
/// meaning of these fields.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionRequest {
    pub id: Uuid,
    /// Adapter selector, e.g. `filesystem`, `terminal`, `browser`.
    pub tool: String,
    /// Skill within the adapter, e.g. `create_file`, `run`.
    pub action: String,
    /// Opaque JSON parameters understood by the target adapter.
    pub parameters: serde_json::Value,
    /// Per-action timeout override; `None` falls back to the runtime default.
    #[serde(with = "serde_duration")]
    pub timeout: Option<Duration>,
    /// Capability strings the caller believes this action requires. The
    /// permission engine is the sole authority; these are hints only.
    pub permissions: Vec<String>,
}

impl ActionRequest {
    pub fn new(
        tool: impl Into<String>,
        action: impl Into<String>,
        parameters: serde_json::Value,
    ) -> Self {
        Self {
            id: Uuid::new_v4(),
            tool: tool.into(),
            action: action.into(),
            parameters,
            timeout: None,
            permissions: Vec::new(),
        }
    }

    /// Extract a required string parameter, returning an error name if absent.
    pub fn param_str(&self, key: &str) -> Result<String, &'static str> {
        self.parameters
            .get(key)
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .ok_or("missing_string_parameter")
    }
}

/// Normalized outcome of an action, returned by every adapter.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ActionResult {
    pub success: bool,
    pub output: serde_json::Value,
    pub error: Option<String>,
    #[serde(default)]
    pub metadata: HashMap<String, serde_json::Value>,
}

impl ActionResult {
    pub fn success(output: serde_json::Value) -> Self {
        Self {
            success: true,
            output,
            error: None,
            metadata: HashMap::new(),
        }
    }

    pub fn error(message: impl Into<String>) -> Self {
        Self {
            success: false,
            output: serde_json::Value::Null,
            error: Some(message.into()),
            metadata: HashMap::new(),
        }
    }

    /// Attach a latency measurement (ms) for telemetry/audit.
    pub fn with_latency_ms(mut self, ms: u128) -> Self {
        self.metadata
            .insert("latency_ms".to_string(), serde_json::json!(ms));
        self
    }
}
