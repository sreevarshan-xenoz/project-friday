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
//!
//! The assistant loop runs **model-backed**: `GoalRunner::with_fabric_and_memory`
//! injects an [`InferenceFabric`] built from the environment (Ollama is always
//! registered; OpenRouter/Groq/Gemini/OmniRoute/… register when their env
//! vars are set), so the model decides what evidence a goal needs before
//! planning — the heuristic catches any model failure (never a failed goal).
//! Persistent memory (S2): every admitted episode is journaled to
//! `FRIDAY_MEMORY_DIR` (default `./friday-data`) and replayed on restart, so
//! FRIDAY knows tomorrow what mattered today.

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use friday_core::runtime::Friday;
use friday_goal_execution::MemoryJournal;
use friday_intelligence::InferenceFabric;
use tokio::sync::broadcast;

use friday_mobile::{BusBridge, CommandGateway, GoalRunner, SystemProbe, WsServer};

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

    // QR-pairing is ON by default (the phone-facing protocol contract). Local
    // development on the desktop portal can opt out:
    //   FRIDAY_MOBILE_AUTH=off cargo run -p friday-mobile --bin friday-mobile-daemon
    let require_auth = std::env::var("FRIDAY_MOBILE_AUTH")
        .map(|v| {
            !matches!(
                v.to_ascii_lowercase().as_str(),
                "off" | "0" | "false" | "no"
            )
        })
        .unwrap_or(true);

    let server = WsServer::new(addr).with_require_auth(require_auth);
    tracing::info!("mobile daemon auth required: {require_auth}");
    let outbound: broadcast::Sender<_> = server.broadcast_tx();
    let sessions = server.sessions();
    let mut ingress = server.subscribe_commands();

    // Subscribe to the bus and forward real events to phones (no polling).
    BusBridge::new(bus.clone(), outbound.clone()).start();

    // Real host telemetry onto the bus.
    let probe = SystemProbe::new(bus.clone());
    tokio::spawn(async move { probe.run().await });

    // The assistant loop: consume `plan.goal` (published by the command
    // gateway when the portal sends `planner.goal`) and run each goal through
    // the real goal-execution Executive with real evidence adapters, then
    // publish goal.started / goal.completed | goal.failed back to the bus so
    // the portal chat responds. Model-backed investigation: the fabric derives
    // the evidence requirements, the heuristic catches any model failure.
    let mut fabric = InferenceFabric::default_from_env();
    // Chaos-lab finding: the fabric had no per-request deadline — a hung
    // model (e.g. a local Ollama that accepts TCP but never answers) would
    // stall the chat forever. Bound one provider attempt; the strategy's
    // heuristic then answers deterministically.
    fabric.request_timeout = Some(Duration::from_secs(15));
    let fabric = Arc::new(fabric);
    tracing::info!(
        "[Investigation] fabric: {} providers / {} models registered",
        fabric.providers.len(),
        fabric.models.len()
    );

    // Persistent memory (S2): one durable journal backs the Executive's
    // admission gate and the memory recall/remember tools. Episodes survive
    // restarts, so the daemon keeps its memory across runs.
    let memory_dir =
        std::env::var("FRIDAY_MEMORY_DIR").unwrap_or_else(|_| "friday-data".to_string());
    let journal = MemoryJournal::open(std::path::Path::new(&memory_dir).join("episodes.jsonl"))
        .expect("open memory journal");
    tracing::info!(
        "persistent memory journal at {}/episodes.jsonl: {} episode(s) replayed",
        memory_dir,
        journal.len()
    );
    GoalRunner::with_fabric_and_memory(bus.clone(), fabric, journal).start();

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
