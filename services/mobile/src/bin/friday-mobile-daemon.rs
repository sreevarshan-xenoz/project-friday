//! FRIDAY Mobile Daemon (M6.2a) — live integration entry point.
//!
//! Wires the mobile transport into the real FRIDAY runtime:
//!
//! ```text
//! Friday runtime (EventBus)
//!        │  SystemProbe publishes real host telemetry
//!        │  Planner / Recovery / Capabilities publish their events
//!   BusBridge  ── subscribes, never polls ──▶ WsServer.outbound ──▶ phones
//!        ▲                                                      │
//!   CommandGateway ◀── commands (ingress) ◀── phone commands ────┘
//!        │ publishes structured commands as real bus events
//! ```
//!
//! Run: `cargo run -p friday-mobile --bin friday-mobile-daemon`
//! (or set `FRIDAY_MOBILE_ADDR`, default `0.0.0.0:18432`).

use std::net::SocketAddr;

use friday_core::runtime::Friday;
use tokio::sync::broadcast;

use friday_mobile::{BusBridge, CommandGateway, SystemProbe, WsServer};

#[tokio::main]
async fn main() {
    friday_core::init_telemetry();
    tracing::info!("FRIDAY Mobile Daemon starting (M6.2a)");

    // Real FRIDAY runtime — the event bus every subsystem publishes to.
    let mut friday = Friday::new();
    let bus = friday.bus().clone();

    let addr: SocketAddr = std::env::var("FRIDAY_MOBILE_ADDR")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or_else(|| "0.0.0.0:18432".parse().unwrap());

    let server = WsServer::new(addr);
    let outbound: broadcast::Sender<_> = server.broadcast_tx();
    let sessions = server.sessions();
    let mut ingress = server.subscribe_commands();

    // Subscribe to the bus and forward real events to phones (no polling).
    BusBridge::new(bus.clone(), outbound.clone()).start();

    // Real host telemetry onto the bus.
    let probe = SystemProbe::new(bus.clone());
    tokio::spawn(async move { probe.run().await });

    // Translate phone commands → real bus events, and return results.
    let gateway = CommandGateway::new(bus.clone(), sessions.clone());
    let result_out = outbound.clone();
    tokio::spawn(async move {
        while let Ok(cmd) = ingress.recv().await {
            let result = gateway.dispatch(&cmd).await;
            if result_out.send(result).is_err() {
                break;
            }
        }
    });

    tracing::info!("mobile daemon listening on {addr}");
    if let Err(e) = server.run().await {
        tracing::error!("mobile daemon stopped: {e}");
    }
    let _ = friday.run().await;
}
