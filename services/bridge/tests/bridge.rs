//! End-to-end protocol tests for the FRIDAY ReadModelBridge.
//!
//! Covers:
//!   1. Frame serialization round-trips into the exact JSON the console reads.
//!   2. The demo source produces a plausible snapshot + deterministic ticks.
//!   3. A live client over TCP/WebSocket sees hello -> snapshot -> delta ->
//!      heartbeat with contiguous sequences and a measured round-trip.

use std::net::SocketAddr;
use std::time::Duration;

use friday_bridge::{
    BridgeServer, DemoRuntimeSource, Frame, FrameSerializer, RuntimeSource, SourceDelta,
};
use futures::StreamExt;
use tokio_tungstenite::connect_async;

// ---------------------------------------------------------------------------
// Unit-ish: frame serialization
// ---------------------------------------------------------------------------

#[test]
fn hello_frame_matches_console_json_shape() {
    let frame = Frame::Hello(friday_bridge::protocol::HelloFrame {
        sequence: 1,
        timestamp: 1_784_000_000_000,
        source: "mock".into(),
        protocol: 1,
        bridge: "friday-bridge-demo".into(),
    });
    let json = FrameSerializer::to_json(&frame).unwrap();
    let value: serde_json::Value = serde_json::from_str(&json).unwrap();

    assert_eq!(value["kind"], "hello");
    assert_eq!(value["sequence"], 1);
    assert_eq!(value["protocol"], 1);
    assert_eq!(value["source"], "mock");
    assert_eq!(value["bridge"], "friday-bridge-demo");
    assert_eq!(value.get("message"), None);
}

#[test]
fn delta_frame_omits_unchanged_slices() {
    let frame = Frame::Delta(friday_bridge::protocol::DeltaFrame {
        sequence: 4,
        timestamp: 1_784_000_000_000,
        source: "mock".into(),
        protocol: 1,
        bridge: "friday-bridge-demo".into(),
        dashboard: None,
        timelines: None,
        metrics: None,
        ledger: None,
        latency_ms: None,
    });
    let json = FrameSerializer::to_json(&frame).unwrap();
    let value: serde_json::Value = serde_json::from_str(&json).unwrap();

    assert_eq!(value["kind"], "delta");
    assert_eq!(value.get("dashboard"), None, "unchanged slices are omitted");
    assert_eq!(value.get("ledger"), None);
}

#[test]
fn frame_round_trips_through_json() {
    let mut source = DemoRuntimeSource::new(42, 8);
    let snapshot = source.snapshot();
    let mut deltas = source.tick(1);
    let delta = deltas.pop().unwrap();

    let frames = vec![
        Frame::Hello(friday_bridge::protocol::HelloFrame {
            sequence: 1,
            timestamp: 1,
            source: "mock".into(),
            protocol: 1,
            bridge: "b".into(),
        }),
        Frame::Snapshot(friday_bridge::protocol::SnapshotFrame {
            sequence: 2,
            timestamp: 2,
            source: "mock".into(),
            protocol: 1,
            bridge: "b".into(),
            dashboard: snapshot.dashboard,
            timelines: snapshot.timelines,
            metrics: snapshot.metrics,
            ledger: snapshot.ledger,
            latency_ms: None,
        }),
        Frame::Delta(friday_bridge::protocol::DeltaFrame {
            sequence: 3,
            timestamp: 3,
            source: "mock".into(),
            protocol: 1,
            bridge: "b".into(),
            dashboard: delta.dashboard,
            timelines: delta.timelines,
            metrics: delta.metrics,
            ledger: delta.ledger,
            latency_ms: None,
        }),
        Frame::Heartbeat(friday_bridge::protocol::HeartbeatFrame {
            sequence: 4,
            timestamp: 4,
            source: "mock".into(),
            protocol: 1,
            bridge: "b".into(),
            latency_ms: Some(9),
        }),
    ];

    for frame in frames {
        let json = FrameSerializer::to_json(&frame).unwrap();
        let parsed = FrameSerializer::from_json(&json).unwrap();
        assert_eq!(parsed, frame, "round-trip must be lossless");
    }
}

#[test]
fn from_json_rejects_unknown_protocol_version() {
    let bad = r#"{"kind":"hello","sequence":1,"timestamp":1,"source":"mock","protocol":99,"bridge":"b"}"#;
    let err = FrameSerializer::from_json(bad).unwrap_err();
    assert!(format!("{err}").contains("protocol"), "got: {err}");
}

#[test]
fn from_json_rejects_missing_kind() {
    let bad = r#"{"sequence":1,"timestamp":1,"source":"mock","protocol":1,"bridge":"b"}"#;
    let err = FrameSerializer::from_json(bad).unwrap_err();
    assert!(format!("{err}").contains("kind"), "got: {err}");
}

// ---------------------------------------------------------------------------
// Unit-ish: demo source
// ---------------------------------------------------------------------------

#[test]
fn demo_source_seeds_read_models_through_frozen_engines() {
    let source = DemoRuntimeSource::new(42, 140);
    assert_eq!(source.bridge_id(), "friday-bridge-demo");
    assert_eq!(source.source_name(), "mock");

    let snap = source.snapshot();
    assert_eq!(snap.timelines.len(), 140, "one timeline per seeded record");
    assert_eq!(snap.ledger.len(), 140);
    assert_eq!(snap.metrics.decisions, 140);
    assert_eq!(snap.dashboard.executive.len(), 10, "ten-phase executive");

    // Determinism: same seed => same metrics.
    let again = DemoRuntimeSource::new(42, 140).snapshot();
    assert_eq!(snap.metrics, again.metrics);
}

#[test]
fn demo_source_tick_advances_executive_and_appends_on_cycle_completion() {
    let mut source = DemoRuntimeSource::new(42, 5);

    // First nine ticks advance the executive without touching the ledger.
    for _ in 0..9 {
        let deltas = source.tick(1);
        assert_eq!(deltas.len(), 1, "one delta per tick");
        let d: &SourceDelta = &deltas[0];
        assert_eq!(d.slices, vec![friday_bridge::Slice::Dashboard]);
        assert!(d.dashboard.is_some());
        assert!(d.ledger.is_none(), "ledger untouched until a cycle completes");
    }

    // Tenth tick completes a cycle: a record is appended and every slice
    // follows along.
    let deltas = source.tick(1);
    let d = &deltas[0];
    assert_eq!(d.slices.len(), 4);
    assert!(d.timelines.is_some());
    assert!(d.metrics.is_some());
    assert!(d.ledger.is_some());
    assert_eq!(d.ledger.as_ref().unwrap().len(), 6);
}

// ---------------------------------------------------------------------------
// Integration: live WebSocket client
// ---------------------------------------------------------------------------

async fn spawn_bridge() -> SocketAddr {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    drop(listener);

    let server = BridgeServer::new(
        addr,
        Box::new(DemoRuntimeSource::new(42, 5)),
    )
    .with_tick_ms(25)
    .with_heartbeat_ms(75);
    tokio::spawn(async move {
        let _ = server.run().await;
    });

    tokio::time::sleep(Duration::from_millis(50)).await;
    addr
}

#[tokio::test]
async fn live_client_sees_handshake_then_streams_with_contiguous_sequences() {
    let addr = spawn_bridge().await;
    let url = format!("ws://{addr}");
    let (mut ws, _resp) = connect_async(&url).await.unwrap();

    // hello -> snapshot immediately.
    let hello = next_frame(&mut ws).await;
    assert_eq!(hello["kind"], "hello");
    assert_eq!(hello["bridge"], "friday-bridge-demo");
    assert_eq!(hello["protocol"], 1);

    let snapshot = next_frame(&mut ws).await;
    assert_eq!(snapshot["kind"], "snapshot");
    assert_eq!(snapshot["timelines"].as_array().unwrap().len(), 5);

    // Collect the stream for ~300ms and verify the frame grammar.
    let mut kinds = Vec::new();
    let mut sequences = Vec::new();
    let mut saw_heartbeat_with_rtt = false;
    let deadline = tokio::time::Instant::now() + Duration::from_millis(300);
    while tokio::time::Instant::now() < deadline {
        if let Some(Ok(msg)) = ws.next().await {
            if let tokio_tungstenite::tungstenite::Message::Text(text) = msg {
                let value: serde_json::Value = serde_json::from_str(&text).unwrap();
                let kind = value["kind"].as_str().unwrap();
                kinds.push(kind.to_string());
                sequences.push(value["sequence"].as_u64().unwrap());
                if kind == "heartbeat" {
                    assert!(
                        value["latency_ms"].is_number(),
                        "round-trip latency must be measured, got {value}"
                    );
                    saw_heartbeat_with_rtt = true;
                }
            }
        }
    }

    assert!(saw_heartbeat_with_rtt, "expected a measured heartbeat");
    assert!(kinds.contains(&"delta".to_string()), "expected deltas: {kinds:?}");
    for pair in sequences.windows(2) {
        assert_eq!(pair[1], pair[0] + 1, "sequences must be contiguous: {sequences:?}");
    }
    let _ = ws.close(None).await;
}

async fn next_frame(
    ws: &mut tokio_tungstenite::WebSocketStream<
        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
    >,
) -> serde_json::Value {
    loop {
        let msg = ws.next().await.expect("connection alive").unwrap();
        if let tokio_tungstenite::tungstenite::Message::Text(text) = msg {
            return serde_json::from_str(&text).unwrap();
        }
    }
}
