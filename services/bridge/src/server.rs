//! The FRIDAY ReadModelBridge server.
//!
//! Binds a TCP listener, accepts WebSocket clients, and runs a driver task
//! that ticks the `RuntimeSource` and broadcasts the resulting `DeltaFrame`s
//! to every connected console. Read-only by construction: the source's read
//! models are serialized as-is; the bridge never mutates the runtime.

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use tokio::net::TcpListener;
use tokio::sync::Mutex;
use tokio_tungstenite::accept_async;
use tracing::{info, warn};

use crate::connection::{handle_connection, ConnectionManager, DeltaPublisher, DEFAULT_BROADCAST_CAPACITY};
use crate::frame_serializer::FrameSerializer;
use crate::now_ms;
use crate::protocol::Frame;
use crate::source::RuntimeSource;

/// The executive-cycle cadence of the demo source (matches the frontend mock).
pub const DEFAULT_TICK_MS: u64 = 1200;
/// How often each connection publishes a heartbeat frame.
pub const DEFAULT_HEARTBEAT_MS: u64 = 3600;

pub struct BridgeServer {
    addr: SocketAddr,
    source: Arc<Mutex<Box<dyn RuntimeSource>>>,
    tick_ms: u64,
    heartbeat_ms: u64,
    broadcast_capacity: usize,
}

impl BridgeServer {
    pub fn new(addr: SocketAddr, source: Box<dyn RuntimeSource>) -> Self {
        Self {
            addr,
            source: Arc::new(Mutex::new(source)),
            tick_ms: DEFAULT_TICK_MS,
            heartbeat_ms: DEFAULT_HEARTBEAT_MS,
            broadcast_capacity: DEFAULT_BROADCAST_CAPACITY,
        }
    }

    /// Override the source-tick cadence (tests use tiny values).
    pub fn with_tick_ms(mut self, tick_ms: u64) -> Self {
        self.tick_ms = tick_ms;
        self
    }

    /// Override the heartbeat cadence.
    pub fn with_heartbeat_ms(mut self, heartbeat_ms: u64) -> Self {
        self.heartbeat_ms = heartbeat_ms;
        self
    }

    /// Set the broadcast backlog a slow console is allowed to accumulate
    /// before it is resynced with a fresh snapshot instead. This is the
    /// bridge's backpressure: memory stays bounded per connection, and a
    /// slow client only ever catches up via resync.
    pub fn with_broadcast_capacity(mut self, capacity: usize) -> Self {
        self.broadcast_capacity = capacity.max(1);
        self
    }

    pub async fn run(self) -> Result<(), Box<dyn std::error::Error>> {
        let listener = TcpListener::bind(self.addr).await?;
        let local = listener.local_addr()?;

        let manager = Arc::new(ConnectionManager::with_capacity(self.broadcast_capacity));

        let (bridge_id, source_name) = {
            let src = self.source.lock().await;
            (src.bridge_id().to_string(), src.source_name().to_string())
        };

        self.spawn_driver(manager.clone(), bridge_id.clone(), source_name.clone());

        info!(
            bridge = %bridge_id,
            source = %source_name,
            capacity = manager.capacity(),
            "FRIDAY ReadModelBridge listening on ws://{local}"
        );

        loop {
            let (stream, peer) = listener.accept().await?;
            let source = self.source.clone();
            let heartbeat_ms = self.heartbeat_ms;
            let manager = manager.clone();
            tokio::spawn(async move {
                match accept_async(stream).await {
                    Ok(ws) => {
                        handle_connection(ws, peer, manager.clone(), source, heartbeat_ms).await;
                    }
                    Err(e) => warn!(%peer, "websocket handshake failed: {e}"),
                }
            });
        }
    }

    fn spawn_driver(&self, manager: Arc<ConnectionManager>, bridge_id: String, source_name: String) {
        let source = self.source.clone();
        let tick = Duration::from_millis(self.tick_ms);
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(tick);
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            loop {
                interval.tick().await;
                let deltas = {
                    let mut src = source.lock().await;
                    src.tick(now_ms())
                };
                for delta in deltas {
                    let frame =
                        DeltaPublisher::frame(&manager, &delta, &source_name, &bridge_id, now_ms());
                    if let Ok(json) = FrameSerializer::to_json(&Frame::Delta(frame.clone())) {
                        manager.stats().set_delta_size(json.len());
                    }
                    manager.publish(Frame::Delta(frame));
                }
            }
        });
    }
}
