//! FRIDAY ReadModelBridge binary.
//!
//! Serves the demo runtime source over WebSocket for the Engineering Console.
//! The bridge is read-only; point it at a real runtime's ledger by swapping
//! the source in `BridgeServer::new`.
//!
//! Usage:
//!   friday-bridge [--listen 127.0.0.1:18433] [--seed 42] [--count 140]
//!   Prometheus metrics: http://127.0.0.1:18434/metrics

use std::net::SocketAddr;
use std::path::PathBuf;
use std::process::ExitCode;

use friday_bridge::{BridgeServer, DemoRuntimeSource, LiveRuntimeSource};
use tracing_subscriber::EnvFilter;

fn parse_args() -> (SocketAddr, u64, u64, Option<PathBuf>, PathBuf) {
    let mut listen = "127.0.0.1:18433".to_string();
    let mut seed = 42u64;
    let mut count = 140u64;
    let mut live_repo = None;
    let mut journal = std::env::temp_dir().join("friday-autonomy-journal.json");
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--listen" => listen = args.next().unwrap_or_else(|| listen.clone()),
            "--seed" => seed = args.next().and_then(|s| s.parse().ok()).unwrap_or(seed),
            "--count" => count = args.next().and_then(|s| s.parse().ok()).unwrap_or(count),
            "--live-repo" => live_repo = args.next().map(PathBuf::from),
            "--journal" => journal = args.next().map(PathBuf::from).unwrap_or(journal),
            "--help" | "-h" => {
                println!("friday-bridge [--listen 127.0.0.1:18433] [--seed 42] [--count 140]");
                println!("             [--live-repo PATH] [--journal PATH]");
                println!("Prometheus metrics: http://127.0.0.1:18434/metrics");
                std::process::exit(0);
            }
            other => {
                eprintln!("unknown argument: {other}");
                std::process::exit(2);
            }
        }
    }
    let addr = listen
        .parse()
        .unwrap_or_else(|e| panic!("invalid --listen address `{listen}`: {e}"));
    (addr, seed, count, live_repo, journal)
}

#[tokio::main]
async fn main() -> ExitCode {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    let (addr, seed, count, live_repo, journal) = parse_args();
    let source: Box<dyn friday_bridge::RuntimeSource> = match live_repo {
        Some(repo) => Box::new(LiveRuntimeSource::new(repo, journal).await),
        None => Box::new(DemoRuntimeSource::new(seed, count)),
    };
    let server = BridgeServer::new(addr, source);

    match server.run().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("bridge error: {e}");
            ExitCode::FAILURE
        }
    }
}
