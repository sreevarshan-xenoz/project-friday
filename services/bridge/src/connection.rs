//! Connection management for the FRIDAY ReadModelBridge.
//!
//! * `ConnectionManager` owns the shared monotonic sequence counter and the
//!   broadcast channel that fans frames out to every connected console.
//! * `SnapshotPublisher` / `DeltaPublisher` / `Heartbeat` build protocol
//!   frames from source state (they never talk to the runtime directly).
//! * `handle_connection` drives one client: handshake (hello + snapshot),
//!   then relays broadcast frames, emits heartbeats at its own cadence, and
//!   measures round-trip latency with WebSocket ping/pong.

use std::net::SocketAddr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use futures::{SinkExt, StreamExt};
use tokio::net::TcpStream;
use tokio::sync::{broadcast, Mutex};
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::WebSocketStream;
use tracing::{info, warn};

use crate::frame_serializer::FrameSerializer;
use crate::now_ms;
use crate::protocol::{
    DeltaFrame, ErrorFrame, Frame, HelloFrame, HeartbeatFrame, PROTOCOL_VERSION, SnapshotFrame,
};
use crate::source::{RuntimeSource, SourceDelta, SourceSnapshot};

type WsStream = WebSocketStream<TcpStream>;

/// The broadcast capacity per connection. When a client falls this far
/// behind it receives a fresh snapshot instead of a backlog.
const BROADCAST_CAPACITY: usize = 256;

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

/// Fan-out hub: owns sequences and broadcasts frames to all consoles.
#[derive(Debug)]
pub struct ConnectionManager {
    next_sequence: SequenceCounter,
    tx: broadcast::Sender<Frame>,
}

impl ConnectionManager {
    pub fn new() -> Self {
        let (tx, _rx) = broadcast::channel(BROADCAST_CAPACITY);
        Self {
            next_sequence: SequenceCounter::new(),
            tx,
        }
    }

    /// Reserve the next global sequence number.
    pub fn next_sequence(&self) -> u64 {
        self.next_sequence.next()
    }

    /// Subscribe to the broadcast stream.
    pub fn subscribe(&self) -> broadcast::Receiver<Frame> {
        self.tx.subscribe()
    }

    /// Publish a frame to every connected console. Returns the number of
    /// receivers it reached (0 when nobody is connected).
    pub fn publish(&self, frame: Frame) -> usize {
        self.tx.send(frame).map_err(|e| e.0).map(|n| n).unwrap_or(0)
    }
}

impl Default for ConnectionManager {
    fn default() -> Self {
        Self::new()
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
pub struct DeltaPublisher;

impl DeltaPublisher {
    pub fn frame(
        manager: &ConnectionManager,
        delta: &SourceDelta,
        source: &str,
        bridge: &str,
        now_ms: u64,
    ) -> DeltaFrame {
        DeltaFrame {
            sequence: manager.next_sequence(),
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
        }
    }

    pub fn error(
        manager: &ConnectionManager,
        message: impl Into<String>,
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
            message: message.into(),
        }
    }
}

fn encode(frame: &Frame, source: &str, bridge: &str) -> String {
    FrameSerializer::to_json(frame).unwrap_or_else(|_| {
        FrameSerializer::error_json(0, now_ms(), source, bridge, "frame serialization failed")
    })
}

/// Drive a single console connection until it closes.
pub async fn handle_connection(
    ws: WsStream,
    peer: SocketAddr,
    manager: Arc<ConnectionManager>,
    source: Arc<Mutex<Box<dyn RuntimeSource>>>,
    heartbeat_ms: u64,
) {
    let (mut sink, mut incoming) = ws.split();
    let mut broadcast_rx = manager.subscribe();

    let (bridge_id, source_name) = {
        let src = source.lock().await;
        (src.bridge_id().to_string(), src.source_name().to_string())
    };

    // Handshake: hello, then a full snapshot so the console can render before
    // any delta arrives. Sequences are reserved at connect time so the client
    // sees a contiguous stream from its own hello onward.
    let hello = HelloFrame {
        sequence: manager.next_sequence(),
        timestamp: now_ms(),
        source: source_name.clone(),
        protocol: PROTOCOL_VERSION,
        bridge: bridge_id.clone(),
    };
    if sink
        .send(Message::Text(encode(&Frame::Hello(hello), &source_name, &bridge_id)))
        .await
        .is_err()
    {
        return;
    }
    let snapshot = {
        let src = source.lock().await;
        SnapshotPublisher::snapshot(&manager, &src.snapshot(), &source_name, &bridge_id, now_ms())
    };
    if sink
        .send(Message::Text(encode(
            &Frame::Snapshot(snapshot),
            &source_name,
            &bridge_id,
        )))
        .await
        .is_err()
    {
        return;
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
                    if sink.send(Message::Text(encode(&frame, &source_name, &bridge_id))).await.is_err() {
                        break;
                    }
                }
                Err(broadcast::error::RecvError::Lagged(skipped)) => {
                    warn!(%peer, skipped, "fell behind on broadcasts; resyncing with a fresh snapshot");
                    let snap = {
                        let src = source.lock().await;
                        SnapshotPublisher::snapshot(&manager, &src.snapshot(), &source_name, &bridge_id, now_ms())
                    };
                    if sink.send(Message::Text(encode(&Frame::Snapshot(snap), &source_name, &bridge_id))).await.is_err() {
                        break;
                    }
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
                    if sink.send(Message::Text(encode(&Frame::Heartbeat(hb), &source_name, &bridge_id))).await.is_err() {
                        break;
                    }
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
