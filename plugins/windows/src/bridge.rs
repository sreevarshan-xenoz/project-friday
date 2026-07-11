//! The Windows adapter: a platform-agnostic [`ActionAdapter`] that forwards
//! `windows.*` intent over IPC. It owns no Windows-specific logic.

use std::sync::Arc;

use async_trait::async_trait;
use friday_actions::adapters::ActionAdapter;
use friday_actions::request::{ActionRequest, ActionResult};

use crate::actions::{intent_for, ACTIONS};
use crate::error::BridgeError;
use crate::ipc::IpcTransport;
use crate::protocol::{BridgeFrame, BridgeResponse, PROTOCOL_VERSION};

/// Adapter that routes `windows.*` actions to the C# Windows host via IPC.
pub struct WindowsAdapter {
    transport: Arc<dyn IpcTransport>,
}

impl WindowsAdapter {
    /// Build with an explicit transport (mock in tests, pipe in production).
    pub fn new(transport: Arc<dyn IpcTransport>) -> Self {
        Self { transport }
    }

    /// Build the production transport for a session (Windows only).
    #[cfg(windows)]
    pub fn for_session(session: &str) -> Self {
        Self::new(Arc::new(crate::ipc::NamedPipeTransport::session(session)))
    }

    /// Decode a response frame into a normalized `ActionResult`.
    fn decode(&self, frame: BridgeFrame) -> ActionResult {
        if frame.protocol_version != PROTOCOL_VERSION {
            return ActionResult::error(format!(
                "protocol_error: expected {}, got {}",
                PROTOCOL_VERSION, frame.protocol_version
            ));
        }
        let response: BridgeResponse = match serde_json::from_value(frame.message.payload.clone()) {
            Ok(r) => r,
            Err(e) => return ActionResult::error(format!("protocol_error: {e}")),
        };
        if response.ok {
            ActionResult::success(response.output)
        } else {
            let code = response.error.unwrap_or_else(|| "Unknown".into());
            let err = BridgeError::from_code(&code);
            ActionResult::error(err.code())
        }
    }
}

#[async_trait]
impl ActionAdapter for WindowsAdapter {
    fn tool(&self) -> &str {
        "windows"
    }

    fn actions(&self) -> Vec<&str> {
        ACTIONS.to_vec()
    }

    async fn execute(&self, req: ActionRequest) -> ActionResult {
        let intent = intent_for(&req.action);
        let frame = BridgeFrame::request(&intent, req.parameters.clone());
        match self.transport.call(frame).await {
            Ok(resp) => self.decode(resp),
            Err(e) => {
                // Transport failure (e.g. pipe down) -> normalized HostUnavailable.
                let _ = e;
                ActionResult::error(BridgeError::HostUnavailable.code())
            }
        }
    }
}
