use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Identifier for a module participating in request/response communication.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ModuleId(pub String);

impl ModuleId {
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }
}

/// A request/response envelope used for direct, correlated communication
/// between modules — distinct from the fire-and-forget [`crate::event::Event`].
/// `reply_to` carries the id of the original message so responses can be
/// correlated with requests across the protocol transport.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProtocolMessage {
    pub id: Uuid,
    pub source: ModuleId,
    pub target: ModuleId,
    pub action: String,
    pub payload: serde_json::Value,
    pub reply_to: Option<Uuid>,
    pub timestamp: DateTime<Utc>,
}

impl ProtocolMessage {
    pub fn request(
        source: ModuleId,
        target: ModuleId,
        action: impl Into<String>,
        payload: serde_json::Value,
    ) -> Self {
        Self {
            id: Uuid::new_v4(),
            source,
            target,
            action: action.into(),
            payload,
            reply_to: None,
            timestamp: Utc::now(),
        }
    }

    /// Build a response to this request, copying the correlation id.
    pub fn reply(&self, payload: serde_json::Value) -> Self {
        Self {
            id: Uuid::new_v4(),
            source: self.target.clone(),
            target: self.source.clone(),
            action: format!("{}.response", self.action),
            payload,
            reply_to: Some(self.id),
            timestamp: Utc::now(),
        }
    }

    pub fn is_response(&self) -> bool {
        self.reply_to.is_some()
    }
}

/// Transport abstraction so modules can communicate over in-process channels,
/// IPC, or the network without changing their logic. The core provides an
/// in-process implementation; language bridges (C#, Python) supply their own.
#[async_trait::async_trait]
pub trait Transport: Send + Sync {
    async fn send(&self, message: ProtocolMessage) -> crate::error::Result<()>;
    async fn recv(&self) -> crate::error::Result<ProtocolMessage>;
}
