//! Deterministic generator for the Protocol Compatibility Suite
//! (`protocol-tests/v1/*.json`).
//!
//! Every fixture is produced through the real construction + serialization
//! paths (`SnapshotPublisher`, `DeltaPublisher`, `Heartbeat`, `HelloFrame`,
//! `FrameSerializer`) with a fixed timestamp so the bytes are reproducible.
//! The control-message fixtures are the canonical client shapes.
//!
//! Regenerate after any intentional wire-format change:
//!
//! ```bash
//! cargo run -p friday-bridge --example dump_protocol
//! ```
//!
//! The `protocol_compat` integration test then fails unless every committed
//! fixture round-trips through `FrameSerializer` byte-identically, so a
//! regeneration must land together with its tests.

use std::fs;
use std::path::PathBuf;

use friday_bridge::connection::ErrorSpec;
use friday_bridge::protocol::HelloFrame;
use friday_bridge::{
    ConnectionManager, DeltaPublisher, DemoRuntimeSource, ErrorCode, Frame, FrameSerializer,
    Heartbeat, RuntimeSource, Severity, SnapshotPublisher, PROTOCOL_VERSION,
};

/// Fixed wire timestamp so fixtures are byte-reproducible across runs.
const FIXED_TS: u64 = 1_700_000_000_000;
const SOURCE: &str = "mock";
const BRIDGE: &str = "friday-bridge-demo";

fn out_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("protocol-tests")
        .join("v1")
}

fn write_fixture(name: &str, frame: &Frame) {
    let json = FrameSerializer::to_json(frame).unwrap_or_else(|e| panic!("serialize {name}: {e}"));
    fs::write(out_dir().join(format!("{name}.json")), format!("{json}\n"))
        .unwrap_or_else(|e| panic!("write {name}: {e}"));
}

#[tokio::main]
async fn main() {
    fs::create_dir_all(out_dir()).expect("create protocol-tests/v1");
    let manager = ConnectionManager::new();
    let source_name = SOURCE.to_string();
    let bridge_id = BRIDGE.to_string();

    // hello — the server's opening frame: identity + counters + capability set.
    let hello = HelloFrame {
        sequence: manager.next_sequence(),
        timestamp: FIXED_TS,
        source: source_name.clone(),
        protocol: PROTOCOL_VERSION,
        bridge: bridge_id.clone(),
        bridge_stats: Some(manager.stats().snapshot()),
        capabilities: friday_bridge::CapabilitySet::standard(),
    };
    write_fixture("hello", &Frame::Hello(hello));

    // capabilities — the negotiation ack (a console enabled `metrics`).
    let caps = friday_bridge::Heartbeat::capabilities(
        &manager,
        friday_bridge::CapabilitySet::standard(),
        &source_name,
        &bridge_id,
        FIXED_TS,
    );
    write_fixture("capabilities", &Frame::Capabilities(caps));

    // snapshot — full read models from a deterministic 5-decision demo runtime.
    let mut source = DemoRuntimeSource::new(42, 5);
    let snap = SnapshotPublisher::snapshot(
        &manager,
        &source.snapshot().await,
        &source_name,
        &bridge_id,
        FIXED_TS,
    );
    write_fixture("snapshot", &Frame::Snapshot(snap));

    // delta — the first tick (the executive phase advances the dashboard).
    let delta = source.tick(FIXED_TS).await;
    let delta = DeltaPublisher::frame(&delta[0], &source_name, &bridge_id, FIXED_TS);
    write_fixture("delta", &Frame::Delta(delta));

    // heartbeat — liveness with a measured 7 ms round-trip + counters.
    let hb = Heartbeat::frame(&manager, Some(7), &source_name, &bridge_id, FIXED_TS);
    write_fixture("heartbeat", &Frame::Heartbeat(hb));

    // error — the machine-readable taxonomy in full.
    let err = Heartbeat::structured_error(
        &manager,
        ErrorSpec {
            code: ErrorCode::UnsupportedCapability,
            severity: Severity::Warning,
            recoverable: true,
            recommended_action: "drop the unsupported capability from hello_response and retry",
            message: "capability `teleport` is not advertised by this bridge".to_string(),
        },
        &source_name,
        &bridge_id,
        FIXED_TS,
    );
    write_fixture("error", &Frame::Error(err));

    // control-hello-response / control-resync — the canonical client messages.
    fs::write(
        out_dir().join("control-hello-response.json"),
        "{\"type\":\"hello_response\",\"capabilities\":{\"metrics\":true}}\n",
    )
    .expect("write control-hello-response");
    fs::write(
        out_dir().join("control-resync.json"),
        "{\"type\":\"resync\"}\n",
    )
    .expect("write control-resync");

    println!("wrote 8 protocol fixtures to {}", out_dir().display());
}
