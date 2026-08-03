//! Serialization for the console frame protocol.
//!
//! Owns protocol-version handling: `to_json` stamps the current version onto
//! every frame (the frame builders already do this), and `from_json` rejects
//! frames that carry an unsupported protocol version before handing them to
//! the caller.

use crate::protocol::{Frame, PROTOCOL_VERSION};
use serde_json::{json, Value};

/// Errors produced while parsing or validating an incoming frame.
#[derive(Debug, thiserror::Error)]
pub enum FrameError {
    #[error("invalid JSON: {0}")]
    Json(#[from] serde_json::Error),
    #[error("missing `kind` on frame")]
    MissingKind,
    #[error("unsupported protocol version {0} (this bridge speaks v{PROTOCOL_VERSION})")]
    UnsupportedProtocol(u32),
}

/// Serializes and validates console frames.
pub struct FrameSerializer;

impl FrameSerializer {
    /// The protocol version this serializer emits and accepts.
    pub const fn protocol_version() -> u32 {
        PROTOCOL_VERSION
    }

    /// Encode a frame as a single-line JSON text message.
    pub fn to_json(frame: &Frame) -> Result<String, serde_json::Error> {
        serde_json::to_string(frame)
    }

    /// Encode a frame as pretty-printed JSON (diagnostics/tests only).
    pub fn to_json_pretty(frame: &Frame) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(frame)
    }

    /// Parse and validate a frame received from a client.
    ///
    /// Verifies the `kind` tag is present and that the frame's `protocol`
    /// matches `PROTOCOL_VERSION` so a mismatched client fails fast.
    pub fn from_json(input: &str) -> Result<Frame, FrameError> {
        let value: Value = serde_json::from_str(input)?;
        if value.get("kind").is_none() {
            return Err(FrameError::MissingKind);
        }
        let frame: Frame = serde_json::from_value(value)?;
        let version = frame_protocol(&frame);
        if version != PROTOCOL_VERSION {
            return Err(FrameError::UnsupportedProtocol(version));
        }
        Ok(frame)
    }

    /// Build the JSON of an error frame without going through the builder
    /// machinery — used when a connection cannot complete the handshake.
    pub fn error_json(
        sequence: u64,
        timestamp: u64,
        source: &str,
        bridge: &str,
        message: &str,
    ) -> String {
        json!({
            "kind": "error",
            "sequence": sequence,
            "timestamp": timestamp,
            "source": source,
            "protocol": PROTOCOL_VERSION,
            "bridge": bridge,
            "message": message,
        })
        .to_string()
    }
}

fn frame_protocol(frame: &Frame) -> u32 {
    match frame {
        Frame::Hello(f) => f.protocol,
        Frame::Snapshot(f) => f.protocol,
        Frame::Delta(f) => f.protocol,
        Frame::Heartbeat(f) => f.protocol,
        Frame::Capabilities(f) => f.protocol,
        Frame::Error(f) => f.protocol,
    }
}
