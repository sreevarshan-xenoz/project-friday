//! The FRIDAY ReadModelBridge server.
//!
//! Binds a TCP listener, accepts WebSocket clients, and runs a driver task
//! that ticks the `RuntimeSource` and broadcasts the resulting `DeltaFrame`s
//! to every connected console. Read-only by construction: the source's read
//! models are serialized as-is; the bridge never mutates the runtime.

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::Mutex;
use tokio_tungstenite::accept_async;
use tracing::{info, warn};

use crate::connection::{
    handle_connection, ConnectionManager, DeltaPublisher, DEFAULT_BROADCAST_CAPACITY,
};
use crate::frame_serializer::FrameSerializer;
use crate::now_ms;
use crate::protocol::{CapabilitySet, Frame};
use crate::source::RuntimeSource;

/// The executive-cycle cadence of the demo source (matches the frontend mock).
pub const DEFAULT_TICK_MS: u64 = 1200;
/// How often each connection publishes a heartbeat frame.
pub const DEFAULT_HEARTBEAT_MS: u64 = 3600;
/// How long a new connection waits for an optional capability negotiation
/// before the snapshot is sent. Clients that answer immediately (the console
/// does) never notice it; clients that do not negotiate wait this long once.
pub const DEFAULT_NEGOTIATION_TIMEOUT_MS: u64 = 300;

pub struct BridgeServer {
    addr: SocketAddr,
    source: Arc<Mutex<Box<dyn RuntimeSource>>>,
    tick_ms: u64,
    heartbeat_ms: u64,
    broadcast_capacity: usize,
    capabilities: CapabilitySet,
    negotiation_timeout: Duration,
    metrics_addr: Option<SocketAddr>,
}

impl BridgeServer {
    pub fn new(addr: SocketAddr, source: Box<dyn RuntimeSource>) -> Self {
        Self {
            addr,
            source: Arc::new(Mutex::new(source)),
            tick_ms: DEFAULT_TICK_MS,
            heartbeat_ms: DEFAULT_HEARTBEAT_MS,
            broadcast_capacity: DEFAULT_BROADCAST_CAPACITY,
            capabilities: CapabilitySet::standard(),
            negotiation_timeout: Duration::from_millis(DEFAULT_NEGOTIATION_TIMEOUT_MS),
            metrics_addr: Some(SocketAddr::from(([127, 0, 0, 1], 18434))),
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

    /// Advertise a different capability set than the standard one. Only what
    /// is advertised can ever be negotiated.
    pub fn with_capabilities(mut self, capabilities: CapabilitySet) -> Self {
        self.capabilities = capabilities;
        self
    }

    /// Enable or disable the Prometheus-compatible metrics listener.
    pub fn with_metrics_addr(mut self, addr: Option<SocketAddr>) -> Self {
        self.metrics_addr = addr;
        self
    }

    /// Override the capability-negotiation window (tests use tiny values).
    pub fn with_negotiation_timeout_ms(mut self, timeout_ms: u64) -> Self {
        self.negotiation_timeout = Duration::from_millis(timeout_ms);
        self
    }

    pub async fn run(self) -> Result<(), Box<dyn std::error::Error>> {
        let listener = TcpListener::bind(self.addr).await?;
        let local = listener.local_addr()?;

        let manager = Arc::new(ConnectionManager::with_capabilities(
            self.capabilities.clone(),
            self.broadcast_capacity,
        ));

        let (bridge_id, source_name) = {
            let src = self.source.lock().await;
            (src.bridge_id().to_string(), src.source_name().to_string())
        };

        self.spawn_driver(manager.clone(), bridge_id.clone(), source_name.clone());
        if let Some(metrics_addr) = self.metrics_addr {
            self.spawn_metrics_server(metrics_addr, manager.stats());
        }

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
            let negotiation_timeout = self.negotiation_timeout;
            let manager = manager.clone();
            tokio::spawn(async move {
                match accept_async(stream).await {
                    Ok(ws) => {
                        handle_connection(
                            ws,
                            peer,
                            manager.clone(),
                            source,
                            heartbeat_ms,
                            negotiation_timeout,
                        )
                        .await;
                    }
                    Err(e) => warn!(%peer, "websocket handshake failed: {e}"),
                }
            });
        }
    }

    fn spawn_metrics_server(&self, addr: SocketAddr, stats: Arc<crate::connection::BridgeStats>) {
        tokio::spawn(async move {
            let listener = match TcpListener::bind(addr).await {
                Ok(listener) => listener,
                Err(error) => {
                    warn!(%addr, %error, "metrics listener failed to bind");
                    return;
                }
            };
            info!(%addr, "Prometheus metrics listening");
            loop {
                let Ok((stream, _)) = listener.accept().await else {
                    break;
                };
                let stats = stats.clone();
                tokio::spawn(async move {
                    let _ = serve_metrics(stream, stats).await;
                });
            }
        });
    }

    fn spawn_driver(
        &self,
        manager: Arc<ConnectionManager>,
        bridge_id: String,
        source_name: String,
    ) {
        let source = self.source.clone();
        let tick = Duration::from_millis(self.tick_ms);
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(tick);
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            loop {
                interval.tick().await;
                let deltas = {
                    let mut src = source.lock().await;
                    src.tick(now_ms()).await
                };
                for delta in deltas {
                    let frame = DeltaPublisher::frame(&delta, &source_name, &bridge_id, now_ms());
                    if let Ok(json) = FrameSerializer::to_json(&Frame::Delta(frame.clone())) {
                        manager.stats().set_delta_size(json.len());
                    }
                    manager.publish(Frame::Delta(frame));
                }
            }
        });
    }
}

async fn serve_metrics(
    mut stream: TcpStream,
    stats: Arc<crate::connection::BridgeStats>,
) -> Result<(), std::io::Error> {
    let mut request = [0u8; 1024];
    let _ = stream.read(&mut request).await?;
    let body = prometheus_metrics(&stats.snapshot());
    let response = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: text/plain; version=0.0.4\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        body.len(), body
    );
    stream.write_all(response.as_bytes()).await
}

pub fn prometheus_metrics(stats: &crate::protocol::BridgeStatsDto) -> String {
    format!(
        "# TYPE friday_bridge_connections_active gauge\nfriday_bridge_connections_active {}\n# TYPE friday_bridge_connections_total counter\nfriday_bridge_connections_total {}\n# TYPE friday_bridge_resyncs_total counter\nfriday_bridge_resyncs_total {}\n# TYPE friday_bridge_frames_sent_total counter\nfriday_bridge_frames_sent_total {}\n# TYPE friday_bridge_bytes_sent_total counter\nfriday_bridge_bytes_sent_total {}\n# TYPE friday_bridge_snapshot_bytes gauge\nfriday_bridge_snapshot_bytes {}\n# TYPE friday_bridge_delta_bytes gauge\nfriday_bridge_delta_bytes {}\n",
        stats.connections,
        stats.reconnects,
        stats.resyncs,
        stats.frames_sent,
        stats.bytes_sent,
        stats.snapshot_size_bytes,
        stats.delta_size_bytes,
    )
}
