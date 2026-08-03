//! FRIDAY ReadModelBridge.
//!
//! A read-only WebSocket bridge from the frozen FRIDAY read models to the
//! Engineering Console. It serializes what the runtime already knows —
//! snapshots, additive deltas, heartbeats — and never plans, learns, or
//! aggregates beyond the frozen engines.

pub mod connection;
pub mod frame_serializer;
pub mod protocol;
pub mod server;
pub mod source;

pub use connection::{ConnectionManager, DeltaPublisher, Heartbeat, SequenceCounter, SnapshotPublisher};
pub use frame_serializer::{FrameError, FrameSerializer};
pub use protocol::{
    CapabilitiesFrame, Capability, CapabilityName, CapabilitySet, ErrorCode, Frame, PROTOCOL_VERSION,
    Severity,
};
pub use server::{
    BridgeServer, DEFAULT_HEARTBEAT_MS, DEFAULT_NEGOTIATION_TIMEOUT_MS, DEFAULT_TICK_MS,
};
pub use source::{Clock, DemoRuntimeSource, RuntimeSource, Slice, SourceDelta, SourceSnapshot};

/// Current wall-clock time in milliseconds since the Unix epoch.
pub fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}
