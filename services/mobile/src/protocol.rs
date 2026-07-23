//! Wire protocol for the FRIDAY Mobile Console (M6.1).
//!
//! All traffic is JSON over WebSocket. Messages are tagged by a `type` field so the
//! transport can route them without knowing their shape. The dashboard payloads
//! reuse the real FRIDAY capability types ([`ServiceStatus`], [`Telemetry`],
//! [`Health`]) so the phone renders exactly what the orchestrator measures — no
//! parallel telemetry invented here.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

pub use friday_capabilities::model::Health;
pub use friday_capabilities::orchestrator::{ServiceStatus, Telemetry};

/// Current Mobile Protocol version. Bump (1.0 → 1.1 → 2.0) only with a deliberate,
/// documented change. Clients announce this in `Hello`; the server answers in
/// `Welcome`. Never break a shipped version casually — treat the protocol as an API.
pub const PROTOCOL_VERSION: &str = "1.0";

/// Identifying information a phone sends on connect.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClientInfo {
    pub device: String,
    pub model: String,
    pub app_version: String,
}

/// A live snapshot of FRIDAY subsystem health for the mobile dashboard.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemHealth {
    pub subsystems: HashMap<String, ServiceStatus>,
    pub latency_ms: u64,
}

/// Messages sent *from* the phone *to* the mobile service.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ClientMessage {
    /// First message on a connection. `token` is `None` for an unpaired device.
    Hello {
        client: ClientInfo,
        token: Option<String>,
        /// Mobile Protocol version, e.g. `"1.0"`. Lets the server reject/negotiate
        /// incompatible clients instead of guessing.
        protocol: String,
    },
    /// Response to an [`ServerMessage::AuthChallenge`]; proves the QR was seen.
    AuthResponse { code: String, signature: String },
    /// A command for FRIDAY (run task, approve, open terminal, ...).
    Command {
        id: String,
        method: String,
        params: serde_json::Value,
    },
    /// Client acknowledges a server-initiated command/request.
    CommandAck { id: String, ok: bool },
    /// Subscribe to outbound event topics (e.g. `tasks`, `notifications`).
    Subscribe { topics: Vec<String> },
    /// Keep-alive; the server answers with [`ServerMessage::Pong`].
    Ping,
}

/// Messages sent *from* the mobile service *to* the phone.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ServerMessage {
    /// Sent immediately on connect. `requires_auth` is true for unpaired devices.
    Welcome {
        session_id: String,
        requires_auth: bool,
        /// Mobile Protocol version the server speaks, e.g. `"1.0"`.
        protocol: String,
    },
    /// Issued when a pairing is initiated on the PC; the QR encodes `pairing_code`.
    AuthChallenge {
        pairing_code: String,
        challenge: String,
    },
    /// Pairing succeeded; the phone persists `token` for future reconnects.
    AuthComplete { token: String },
    /// An outbound event (notification, task update, log line, ...).
    Event {
        topic: String,
        payload: serde_json::Value,
    },
    /// A full health snapshot for the dashboard.
    Health { snapshot: SystemHealth },
    /// A single telemetry sample.
    Telemetry { sample: Telemetry },
    /// Result of a [`ClientMessage::Command`].
    CommandResult {
        id: String,
        ok: bool,
        result: serde_json::Value,
    },
    /// Answer to [`ClientMessage::Ping`].
    Pong,
    /// Protocol-level error.
    Error { code: String, message: String },
}

/// Encode a server message as a WebSocket text frame.
pub fn encode(msg: &ServerMessage) -> Result<String, serde_json::Error> {
    serde_json::to_string(msg)
}

/// Decode a client message from a WebSocket text frame.
pub fn decode(text: &str) -> Result<ClientMessage, serde_json::Error> {
    serde_json::from_str(text)
}

/// Decode a server message from a WebSocket text frame.
pub fn decode_server(text: &str) -> Result<ServerMessage, serde_json::Error> {
    serde_json::from_str(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hello_round_trips() {
        let msg = ClientMessage::Hello {
            client: ClientInfo {
                device: "Samsung J7".into(),
                model: "SM-J730G".into(),
                app_version: "0.1.0".into(),
            },
            token: None,
            protocol: "1.0".into(),
        };
        let s = serde_json::to_string(&msg).unwrap();
        assert!(s.contains("\"type\":\"hello\""));
        let back: ClientMessage = serde_json::from_str(&s).unwrap();
        matches!(back, ClientMessage::Hello { token: None, .. });
    }

    #[test]
    fn welcome_is_tagged() {
        let w = ServerMessage::Welcome {
            session_id: "abc".into(),
            requires_auth: true,
            protocol: "1.0".into(),
        };
        let s = encode(&w).unwrap();
        let back: ServerMessage = serde_json::from_str(&s).unwrap();
        matches!(
            back,
            ServerMessage::Welcome {
                requires_auth: true,
                ..
            }
        );
    }

    #[test]
    fn health_snapshot_carries_real_types() {
        let mut subsystems = HashMap::new();
        subsystems.insert(
            "planner".to_string(),
            ServiceStatus {
                state: friday_capabilities::orchestrator::ServiceState::Healthy,
                since: chrono::Utc::now(),
                health: Health::Healthy,
                message: "ok".into(),
            },
        );
        let h = ServerMessage::Health {
            snapshot: SystemHealth {
                subsystems,
                latency_ms: 183,
            },
        };
        let s = encode(&h).unwrap();
        assert!(s.contains("\"latency_ms\":183"));
        let back: ServerMessage = serde_json::from_str(&s).unwrap();
        matches!(back, ServerMessage::Health { .. });
    }
}
