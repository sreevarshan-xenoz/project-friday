//! Versioned IPC envelope. Reuses [`friday_core::protocol::ProtocolMessage`] as
//! the correlated request/response body and adds exactly one field —
//! `protocol_version` — so the frozen core contract is untouched.

use friday_core::protocol::{ModuleId, ProtocolMessage};
use serde::{Deserialize, Serialize};

/// Current wire protocol major version. Minor (additive) changes are tolerated;
/// a mismatched *major* version is rejected as `ProtocolError`.
pub const PROTOCOL_VERSION: u8 = 1;

/// Logical name of the C# host service the adapter talks to.
pub const BRIDGE_NAME: &str = "windows-host";
/// Logical name of this adapter (the IPC client side).
pub const ADAPTER_NAME: &str = "windows-adapter";

/// A versioned, length-framed IPC message. The `message` body is the core
/// request/response envelope; `protocol_version` is the only bridge addition.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BridgeFrame {
    pub protocol_version: u8,
    pub message: ProtocolMessage,
}

/// The only response payload shape that crosses the boundary. `ok` carries the
/// success flag; `output` is normalized JSON; `error` is a [`error::BridgeError`]
/// code string (never an HRESULT / Win32 code).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BridgeResponse {
    pub ok: bool,
    #[serde(default = "serde_json::Value::default")]
    pub output: serde_json::Value,
    #[serde(default)]
    pub error: Option<String>,
}

impl BridgeResponse {
    pub fn ok(output: serde_json::Value) -> Self {
        Self {
            ok: true,
            output,
            error: None,
        }
    }

    pub fn err(code: &str) -> Self {
        Self {
            ok: false,
            output: serde_json::Value::Null,
            error: Some(code.to_string()),
        }
    }
}

impl BridgeFrame {
    /// Build a request frame for an intent (e.g. `windows.open`).
    pub fn request(intent: &str, payload: serde_json::Value) -> Self {
        let message = ProtocolMessage::request(
            ModuleId::new(ADAPTER_NAME),
            ModuleId::new(BRIDGE_NAME),
            intent,
            payload,
        );
        Self {
            protocol_version: PROTOCOL_VERSION,
            message,
        }
    }

    /// Build the correlated response frame for this request.
    pub fn response_to(&self, response: BridgeResponse) -> Self {
        let payload = serde_json::json!({
            "ok": response.ok,
            "output": response.output,
            "error": response.error,
        });
        Self {
            protocol_version: self.protocol_version,
            message: self.message.reply(payload),
        }
    }
}
