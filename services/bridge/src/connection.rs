//! Connection management for the FRIDAY ReadModelBridge.
//!
//! * `ConnectionManager` owns the shared monotonic sequence counter, the
//!   broadcast channel that fans frames out to every connected console, and
//!   the capability set the bridge advertises.
//! * `SnapshotPublisher` / `DeltaPublisher` / `Heartbeat` build protocol
//!   frames from source state (they never talk to the runtime directly).
//! * `handle_connection` drives one client through the protocol state machine:
//!   hello (advertising capabilities) → capability negotiation (optional) →
//!   snapshot → streaming, then relays broadcast frames, emits heartbeats at
//!   its own cadence, and measures round-trip latency with WebSocket ping/pong.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use futures::{SinkExt, StreamExt};
use serde_json::Value;
use tokio::net::TcpStream;
use tokio::sync::{broadcast, Mutex};
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::WebSocketStream;
use tracing::{info, warn};

use crate::frame_serializer::FrameSerializer;
use crate::now_ms;
use crate::protocol::{
    BridgeStatsDto, CapabilitiesFrame, Capability, CapabilitySet, DeltaFrame, ErrorCode,
    ErrorFrame, Frame, HelloFrame, HeartbeatFrame, PROTOCOL_VERSION, Severity, SnapshotFrame,
};
use crate::source::{RuntimeSource, SourceDelta, SourceSnapshot};

type WsStream = WebSocketStream<TcpStream>;

/// The broadcast capacity per connection. When a client falls this far
/// behind it receives a fresh snapshot instead of a backlog.
pub const DEFAULT_BROADCAST_CAPACITY: usize = 256;

/// A shared, monotonic frame sequence so every frame (broadcast or
/// per-connection) consumes from the same stream.
#[derive(Debug, Default)]
pub struct SequenceCounter {
    next: AtomicU64,
}

impl SequenceCounter {
    pub fn new() -> Self {
        Self {
            next: AtomicU64::new(0),
        }
    }
    pub fn next(&self) -> u64 {
        self.next.fetch_add(1, Ordering::Relaxed) + 1
    }
}

/// Bridge-wide counters, read by the console from hello/heartbeat frames.
///
/// `connections_active` is maintained with a Drop guard so it never leaks when
/// a connection task exits early.
#[derive(Debug, Default)]
pub struct BridgeStats {
    connections_active: AtomicU64,
    connections_total: AtomicU64,
    resyncs: AtomicU64,
    frames_sent: AtomicU64,
    bytes_sent: AtomicU64,
    snapshot_size_bytes: AtomicU64,
    delta_size_bytes: AtomicU64,
}

impl BridgeStats {
    /// A console connected: bump both the lifetime and active counters.
    pub fn connect(&self) {
        self.connections_total.fetch_add(1, Ordering::Relaxed);
        self.connections_active.fetch_add(1, Ordering::Relaxed);
    }

    /// A console disconnected.
    pub fn disconnect(&self) {
        self.connections_active.fetch_sub(1, Ordering::Relaxed);
    }

    /// One frame successfully written to a client.
    pub fn record_send(&self, bytes: usize) {
        self.frames_sent.fetch_add(1, Ordering::Relaxed);
        self.bytes_sent.fetch_add(bytes as u64, Ordering::Relaxed);
    }

    /// A resync snapshot was served (automatic or client-requested).
    pub fn record_resync(&self) {
        self.resyncs.fetch_add(1, Ordering::Relaxed);
    }

    /// Wire size of the last snapshot frame actually sent.
    pub fn set_snapshot_size(&self, bytes: usize) {
        self.snapshot_size_bytes.store(bytes as u64, Ordering::Relaxed);
    }

    /// Wire size of the last delta frame published.
    pub fn set_delta_size(&self, bytes: usize) {
        self.delta_size_bytes.store(bytes as u64, Ordering::Relaxed);
    }

    pub fn snapshot(&self) -> BridgeStatsDto {
        BridgeStatsDto {
            connections: self.connections_active.load(Ordering::Relaxed),
            reconnects: self.connections_total.load(Ordering::Relaxed),
            resyncs: self.resyncs.load(Ordering::Relaxed),
            frames_sent: self.frames_sent.load(Ordering::Relaxed),
            bytes_sent: self.bytes_sent.load(Ordering::Relaxed),
            snapshot_size_bytes: self.snapshot_size_bytes.load(Ordering::Relaxed),
            delta_size_bytes: self.delta_size_bytes.load(Ordering::Relaxed),
        }
    }
}

/// Fan-out hub: owns sequences, counters, advertised capabilities, and
/// broadcasts frames to consoles.
#[derive(Debug)]
pub struct ConnectionManager {
    next_sequence: SequenceCounter,
    tx: broadcast::Sender<Frame>,
    stats: Arc<BridgeStats>,
    capacity: usize,
    capabilities: CapabilitySet,
}

impl ConnectionManager {
    pub fn new() -> Self {
        Self::with_capacity(DEFAULT_BROADCAST_CAPACITY)
    }

    pub fn with_capacity(capacity: usize) -> Self {
        Self::with_capabilities(CapabilitySet::standard(), capacity)
    }

    /// A manager that advertises the given capability set. This is the server's
    /// feature gate: only what is advertised can ever be negotiated.
    pub fn with_capabilities(capabilities: CapabilitySet, capacity: usize) -> Self {
        let (tx, _rx) = broadcast::channel(capacity.max(1));
        Self {
            next_sequence: SequenceCounter::new(),
            tx,
            stats: Arc::new(BridgeStats::default()),
            capacity: capacity.max(1),
            capabilities,
        }
    }

    /// Reserve the next global sequence number.
    pub fn next_sequence(&self) -> u64 {
        self.next_sequence.next()
    }

    /// Shared bridge-wide counters.
    pub fn stats(&self) -> Arc<BridgeStats> {
        self.stats.clone()
    }

    /// The broadcast backlog limit for a slow console.
    pub fn capacity(&self) -> usize {
        self.capacity
    }

    /// The capability set this bridge advertises to new consoles.
    pub fn capabilities(&self) -> &CapabilitySet {
        &self.capabilities
    }

    /// Subscribe to the broadcast stream.
    pub fn subscribe(&self) -> broadcast::Receiver<Frame> {
        self.tx.subscribe()
    }

    /// Publish a frame to every connected console. Returns the number of
    /// receivers it reached (0 when nobody is connected).
    pub fn publish(&self, frame: Frame) -> usize {
        self.tx.send(frame).unwrap_or(0)
    }
}

impl Default for ConnectionManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Keeps `connections_active` in sync when the connection task exits early.
struct ActiveConnectionGuard {
    stats: Arc<BridgeStats>,
}

impl Drop for ActiveConnectionGuard {
    fn drop(&mut self) {
        self.stats.disconnect();
    }
}

/// Builds the initial full-state `snapshot` frame for a new connection.
pub struct SnapshotPublisher;

impl SnapshotPublisher {
    pub fn snapshot(
        manager: &ConnectionManager,
        snapshot: &SourceSnapshot,
        source: &str,
        bridge: &str,
        now_ms: u64,
    ) -> SnapshotFrame {
        SnapshotFrame {
            sequence: manager.next_sequence(),
            timestamp: now_ms,
            source: source.to_string(),
            protocol: PROTOCOL_VERSION,
            bridge: bridge.to_string(),
            dashboard: snapshot.dashboard.clone(),
            timelines: snapshot.timelines.clone(),
            metrics: snapshot.metrics.clone(),
            ledger: snapshot.ledger.clone(),
            latency_ms: None,
        }
    }
}

/// Builds additive `delta` frames from a source tick.
///
/// The driver broadcasts deltas WITHOUT a sequence: the connection loop stamps
/// each one at the moment it writes it, so every console's stream stays
/// contiguous in send order even when a heartbeat or ack interleaves. The
/// driver and the connection loops never race on the shared counter.
pub struct DeltaPublisher;

impl DeltaPublisher {
    pub fn frame(delta: &SourceDelta, source: &str, bridge: &str, now_ms: u64) -> DeltaFrame {
        DeltaFrame {
            sequence: 0,
            timestamp: now_ms,
            source: source.to_string(),
            protocol: PROTOCOL_VERSION,
            bridge: bridge.to_string(),
            dashboard: delta.dashboard.clone(),
            timelines: delta.timelines.clone(),
            metrics: delta.metrics.clone(),
            ledger: delta.ledger.clone(),
            latency_ms: None,
        }
    }
}

/// Assign a connection-local sequence to a frame at write time. Broadcast
/// frames arrive placeholder-stamped; per-connection frames already reserve
/// theirs in the connection task, so re-stamping is a no-op for those.
fn stamp_frame(frame: Frame, sequence: u64) -> Frame {
    match frame {
        Frame::Hello(mut f) => {
            f.sequence = sequence;
            Frame::Hello(f)
        }
        Frame::Snapshot(mut f) => {
            f.sequence = sequence;
            Frame::Snapshot(f)
        }
        Frame::Delta(mut f) => {
            f.sequence = sequence;
            Frame::Delta(f)
        }
        Frame::Heartbeat(mut f) => {
            f.sequence = sequence;
            Frame::Heartbeat(f)
        }
        Frame::Error(mut f) => {
            f.sequence = sequence;
            Frame::Error(f)
        }
        Frame::Capabilities(mut f) => {
            f.sequence = sequence;
            Frame::Capabilities(f)
        }
    }
}

/// Builds liveness frames (`heartbeat`, `error`).
pub struct Heartbeat;

impl Heartbeat {
    pub fn frame(
        manager: &ConnectionManager,
        latency_ms: Option<u64>,
        source: &str,
        bridge: &str,
        now_ms: u64,
    ) -> HeartbeatFrame {
        HeartbeatFrame {
            sequence: manager.next_sequence(),
            timestamp: now_ms,
            source: source.to_string(),
            protocol: PROTOCOL_VERSION,
            bridge: bridge.to_string(),
            latency_ms,
            bridge_stats: Some(manager.stats.snapshot()),
        }
    }

    pub fn error(
        manager: &ConnectionManager,
        message: impl Into<String>,
        source: &str,
        bridge: &str,
        now_ms: u64,
    ) -> ErrorFrame {
        Self::structured_error(
            manager,
            ErrorSpec {
                code: ErrorCode::InternalBridgeError,
                severity: Severity::Error,
                recoverable: false,
                recommended_action: "retry the connection",
                message: message.into(),
            },
            source,
            bridge,
            now_ms,
        )
    }

    /// Build an error frame carrying the full machine-readable taxonomy.
    pub fn structured_error(
        manager: &ConnectionManager,
        spec: ErrorSpec,
        source: &str,
        bridge: &str,
        now_ms: u64,
    ) -> ErrorFrame {
        ErrorFrame {
            sequence: manager.next_sequence(),
            timestamp: now_ms,
            source: source.to_string(),
            protocol: PROTOCOL_VERSION,
            bridge: bridge.to_string(),
            message: spec.message,
            code: Some(spec.code),
            severity: Some(spec.severity),
            recoverable: Some(spec.recoverable),
            recommended_action: Some(spec.recommended_action.to_string()),
        }
    }

    /// Build the `capabilities` acknowledgement frame after a negotiation.
    pub fn capabilities(
        manager: &ConnectionManager,
        capabilities: CapabilitySet,
        source: &str,
        bridge: &str,
        now_ms: u64,
    ) -> CapabilitiesFrame {
        CapabilitiesFrame {
            sequence: manager.next_sequence(),
            timestamp: now_ms,
            source: source.to_string(),
            protocol: PROTOCOL_VERSION,
            bridge: bridge.to_string(),
            capabilities,
        }
    }
}

fn encode(frame: &Frame, source: &str, bridge: &str) -> String {
    FrameSerializer::to_json(frame).unwrap_or_else(|_| {
        FrameSerializer::error_json(0, now_ms(), source, bridge, "frame serialization failed")
    })
}

/// The full machine-readable description of a problem, carried in an
/// `error` frame (`code`/`severity`/`recoverable`/`recommended_action` +
/// `message`). Bundled so callers describe the error once.
#[derive(Debug, Clone)]
pub struct ErrorSpec {
    pub code: ErrorCode,
    pub severity: Severity,
    pub recoverable: bool,
    pub recommended_action: &'static str,
    pub message: String,
}

/// A client → server control message (the only messages clients may send).
enum Control {
    /// `{"type":"hello_response","capabilities":{name: bool}}`
    HelloResponse(HashMap<String, bool>),
    /// `{"type":"resync"}`
    Resync,
    /// A typed protocol frame — never legal from a client.
    TypedFrame,
    /// Unknown control shape; ignored for forward compatibility.
    Ignore,
}

fn parse_control(text: &str) -> Control {
    let Ok(value) = serde_json::from_str::<Value>(text) else {
        return Control::Ignore;
    };
    if value.get("kind").is_some() {
        return Control::TypedFrame;
    }
    match value.get("type").and_then(|t| t.as_str()) {
        Some("hello_response") => {
            let caps = value
                .get("capabilities")
                .and_then(|c| serde_json::from_value::<HashMap<String, bool>>(c.clone()).ok())
                .unwrap_or_default();
            Control::HelloResponse(caps)
        }
        Some("resync") => Control::Resync,
        _ => Control::Ignore,
    }
}

/// Write a frame to the console and count it in the bridge stats. Returns
/// false when the connection died.
async fn send_frame(
    sink: &mut (impl SinkExt<Message> + Unpin),
    manager: &ConnectionManager,
    frame: Frame,
    source: &str,
    bridge: &str,
) -> bool {
    let text = encode(&frame, source, bridge);
    if sink.send(Message::Text(text.clone())).await.is_err() {
        return false;
    }
    manager.stats().record_send(text.len());
    true
}

/// Emit a structured error frame (full taxonomy: code/severity/recoverable/
/// recommended_action/message).
async fn send_structured_error(
    sink: &mut (impl SinkExt<Message> + Unpin),
    manager: &ConnectionManager,
    spec: ErrorSpec,
    source: &str,
    bridge: &str,
) {
    let frame = Heartbeat::structured_error(manager, spec, source, bridge, now_ms());
    send_frame(sink, manager, Frame::Error(frame), source, bridge).await;
}

/// Acknowledge a negotiation with the negotiated capability set.
async fn send_negotiation_ack(
    sink: &mut (impl SinkExt<Message> + Unpin),
    manager: &ConnectionManager,
    advertised: &CapabilitySet,
    enabled: Vec<Capability>,
    source: &str,
    bridge: &str,
) {
    let ack = CapabilitySet {
        protocol_version: advertised.protocol_version,
        required: advertised.required.clone(),
        optional: enabled,
    };
    let frame = Heartbeat::capabilities(manager, ack, source, bridge, now_ms());
    send_frame(sink, manager, Frame::Capabilities(frame), source, bridge).await;
}

/// The capability negotiation window. The console *may* answer the advertised
/// set with its selections; a console that never answers is served with just
/// the required capabilities (old clients keep working). Whatever is agreed
/// here is locked in before the snapshot is sent.
async fn negotiate(
    sink: &mut (impl SinkExt<Message> + Unpin),
    incoming: &mut (impl StreamExt<Item = Result<Message, tokio_tungstenite::tungstenite::Error>> + Unpin),
    manager: &ConnectionManager,
    advertised: &CapabilitySet,
    source: &str,
    bridge: &str,
    timeout: Duration,
) {
    let deadline = Instant::now() + timeout;
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return;
        }
        tokio::select! {
            incoming_msg = incoming.next() => match incoming_msg {
                Some(Ok(Message::Text(text))) => match parse_control(&text) {
                    Control::HelloResponse(selections) => {
                        match advertised.resolve(&selections) {
                            Ok(enabled) => {
                                send_negotiation_ack(sink, manager, advertised, enabled, source, bridge).await;
                                return;
                            }
                            Err(unknown) => {
                                warn!(capability = %unknown, "console requested an unsupported capability");
                                send_structured_error(
                                    sink,
                                    manager,
                                    ErrorSpec {
                                        code: ErrorCode::UnsupportedCapability,
                                        severity: Severity::Warning,
                                        recoverable: true,
                                        recommended_action: "drop the unsupported capability from hello_response and retry",
                                        message: format!("capability `{unknown}` is not advertised by this bridge"),
                                    },
                                    source,
                                    bridge,
                                )
                                .await;
                            }
                        }
                    }
                    Control::Resync => {
                        send_structured_error(
                            sink,
                            manager,
                            ErrorSpec {
                                code: ErrorCode::InvalidFrame,
                                severity: Severity::Warning,
                                recoverable: false,
                                recommended_action: "resync is only valid after the initial snapshot",
                                message: "resync received before the initial snapshot".to_string(),
                            },
                            source,
                            bridge,
                        )
                        .await;
                    }
                    Control::TypedFrame => {
                        send_structured_error(
                            sink,
                            manager,
                            ErrorSpec {
                                code: ErrorCode::InvalidFrame,
                                severity: Severity::Warning,
                                recoverable: true,
                                recommended_action: "clients only send control messages (hello_response, resync)",
                                message: "received a typed frame from a client".to_string(),
                            },
                            source,
                            bridge,
                        )
                        .await;
                    }
                    Control::Ignore => {}
                },
                Some(Ok(_)) => {}
                Some(Err(e)) => {
                    warn!("connection error during negotiation: {e}");
                    return;
                }
                None => return,
            },
            _ = tokio::time::sleep(remaining) => return,
        }
    }
}

/// Drive a single console connection until it closes.
pub async fn handle_connection(
    ws: WsStream,
    peer: SocketAddr,
    manager: Arc<ConnectionManager>,
    source: Arc<Mutex<Box<dyn RuntimeSource>>>,
    heartbeat_ms: u64,
    negotiation_timeout: Duration,
) {
    let stats = manager.stats();
    let _active = ActiveConnectionGuard { stats: stats.clone() };
    stats.connect();

    let (mut sink, mut incoming) = ws.split();

    let (bridge_id, source_name) = {
        let src = source.lock().await;
        (src.bridge_id().to_string(), src.source_name().to_string())
    };

    // Handshake: hello advertises the capability set, then the console may
    // negotiate optional capabilities before the snapshot is sent. Sequences
    // are reserved at connect time so the client sees a contiguous stream
    // from its own hello onward.
    let advertised = manager.capabilities().clone();
    let hello = HelloFrame {
        sequence: manager.next_sequence(),
        timestamp: now_ms(),
        source: source_name.clone(),
        protocol: PROTOCOL_VERSION,
        bridge: bridge_id.clone(),
        bridge_stats: Some(stats.snapshot()),
        capabilities: advertised.clone(),
    };
    if !send_frame(&mut sink, &manager, Frame::Hello(hello), &source_name, &bridge_id).await {
        return;
    }

    negotiate(
        &mut sink,
        &mut incoming,
        &manager,
        &advertised,
        &source_name,
        &bridge_id,
        negotiation_timeout,
    )
    .await;

    // Subscribe after negotiation: the snapshot is the full state, and a fresh
    // receiver never misses (or lags on) a broadcast.
    let mut broadcast_rx = manager.subscribe();

    let snapshot = {
        let src = source.lock().await;
        SnapshotPublisher::snapshot(&manager, &src.snapshot(), &source_name, &bridge_id, now_ms())
    };
    {
        let text = encode(&Frame::Snapshot(snapshot), &source_name, &bridge_id);
        stats.set_snapshot_size(text.len());
        if sink.send(Message::Text(text.clone())).await.is_err() {
            return;
        }
        stats.record_send(text.len());
    }

    let mut heartbeat_interval = tokio::time::interval(Duration::from_millis(heartbeat_ms));
    heartbeat_interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    let mut pending_ping_at: Option<Instant> = None;
    let mut last_rtt_ms: Option<u64> = None;

    info!(%peer, bridge = %bridge_id, "console connected");

    loop {
        tokio::select! {
            broadcasted = broadcast_rx.recv() => match broadcasted {
                Ok(frame) => {
                    let frame = stamp_frame(frame, manager.next_sequence());
                    let text = encode(&frame, &source_name, &bridge_id);
                    if sink.send(Message::Text(text.clone())).await.is_err() {
                        break;
                    }
                    stats.record_send(text.len());
                }
                Err(broadcast::error::RecvError::Lagged(skipped)) => {
                    warn!(%peer, skipped, "fell behind on broadcasts; resyncing with a fresh snapshot");
                    let snap = {
                        let src = source.lock().await;
                        SnapshotPublisher::snapshot(&manager, &src.snapshot(), &source_name, &bridge_id, now_ms())
                    };
                    let text = encode(&Frame::Snapshot(snap), &source_name, &bridge_id);
                    stats.set_snapshot_size(text.len());
                    stats.record_resync();
                    if sink.send(Message::Text(text.clone())).await.is_err() {
                        break;
                    }
                    stats.record_send(text.len());
                }
                Err(broadcast::error::RecvError::Closed) => break,
            },
            _ = heartbeat_interval.tick() => {
                if let Some(started) = pending_ping_at {
                    if started.elapsed() > Duration::from_millis(heartbeat_ms) {
                        warn!(%peer, "peer never ponged; dropping");
                        break;
                    }
                } else if sink.send(Message::Ping(now_ms().to_be_bytes().to_vec())).await.is_err() {
                    break;
                } else {
                    pending_ping_at = Some(Instant::now());
                }
                if let Some(rtt) = last_rtt_ms {
                    let hb = Heartbeat::frame(&manager, Some(rtt), &source_name, &bridge_id, now_ms());
                    let text = encode(&Frame::Heartbeat(hb), &source_name, &bridge_id);
                    if sink.send(Message::Text(text.clone())).await.is_err() {
                        break;
                    }
                    stats.record_send(text.len());
                }
            }
            incoming_msg = incoming.next() => match incoming_msg {
                Some(Ok(Message::Pong(_))) => {
                    if let Some(started) = pending_ping_at.take() {
                        last_rtt_ms = Some(started.elapsed().as_millis() as u64);
                    }
                }
                Some(Ok(Message::Close(_))) | None => break,
                Some(Ok(Message::Ping(payload))) => {
                    let _ = sink.send(Message::Pong(payload)).await;
                }
                Some(Ok(Message::Text(text))) => match parse_control(&text) {
                    Control::HelloResponse(selections) => {
                        // Late negotiation: still validated and acked. It has no
                        // effect on an already-open transport (compression/binary
                        // are agreed before the snapshot) but the plumbing stays
                        // honest and the error taxonomy applies.
                        match advertised.resolve(&selections) {
                            Ok(enabled) => {
                                send_negotiation_ack(
                                    &mut sink,
                                    &manager,
                                    &advertised,
                                    enabled,
                                    &source_name,
                                    &bridge_id,
                                )
                                .await;
                            }
                            Err(unknown) => {
                                send_structured_error(
                                    &mut sink,
                                    &manager,
                                    ErrorSpec {
                                        code: ErrorCode::UnsupportedCapability,
                                        severity: Severity::Warning,
                                        recoverable: true,
                                        recommended_action: "drop the unsupported capability from hello_response and retry",
                                        message: format!("capability `{unknown}` is not advertised by this bridge"),
                                    },
                                    &source_name,
                                    &bridge_id,
                                )
                                .await;
                            }
                        }
                    }
                    Control::Resync => {
                        info!(%peer, "console requested a resync snapshot");
                        let snap = {
                            let src = source.lock().await;
                            SnapshotPublisher::snapshot(&manager, &src.snapshot(), &source_name, &bridge_id, now_ms())
                        };
                        let text = encode(&Frame::Snapshot(snap), &source_name, &bridge_id);
                        stats.set_snapshot_size(text.len());
                        stats.record_resync();
                        if sink.send(Message::Text(text.clone())).await.is_err() {
                            break;
                        }
                        stats.record_send(text.len());
                    }
                    Control::TypedFrame => {
                        send_structured_error(
                            &mut sink,
                            &manager,
                            ErrorSpec {
                                code: ErrorCode::InvalidFrame,
                                severity: Severity::Warning,
                                recoverable: true,
                                recommended_action: "clients only send control messages (hello_response, resync)",
                                message: "received a typed frame from a client".to_string(),
                            },
                            &source_name,
                            &bridge_id,
                        )
                        .await;
                    }
                    Control::Ignore => {}
                },
                Some(Ok(_)) => {}
                Some(Err(e)) => {
                    warn!(%peer, "connection error: {e}");
                    break;
                }
            },
        }
    }

    let _ = sink.send(Message::Close(None)).await;
    info!(%peer, "console disconnected");
}
