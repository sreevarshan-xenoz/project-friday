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
use futures::{SinkExt, StreamExt};
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
        bridge_stats: None,
        capabilities: friday_bridge::CapabilitySet::standard(),
    });
    let json = FrameSerializer::to_json(&frame).unwrap();
    let value: serde_json::Value = serde_json::from_str(&json).unwrap();

    assert_eq!(value["kind"], "hello");
    assert_eq!(value["sequence"], 1);
    assert_eq!(value["protocol"], 1);
    assert_eq!(value["source"], "mock");
    assert_eq!(value["bridge"], "friday-bridge-demo");
    assert_eq!(value.get("message"), None);
    assert_eq!(value.get("bridge_stats"), None, "stats omitted when None");
    assert!(
        value["capabilities"].is_object(),
        "hello advertises capabilities: {value}"
    );
    assert_eq!(value["capabilities"]["protocol_version"], 1);
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
            bridge_stats: None,
            capabilities: friday_bridge::CapabilitySet::standard(),
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
            bridge_stats: Some(friday_bridge::protocol::BridgeStatsDto {
                connections: 1,
                reconnects: 1,
                resyncs: 0,
                frames_sent: 3,
                bytes_sent: 1024,
                snapshot_size_bytes: 900,
                delta_size_bytes: 60,
            }),
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

#[test]
fn capability_resolve_enables_only_advertised_features() {
    let set = friday_bridge::CapabilitySet::standard();
    let mut selections = std::collections::HashMap::new();
    selections.insert("metrics".to_string(), true);
    selections.insert("gzip".to_string(), false); // off = ignored

    let enabled = set.resolve(&selections).unwrap();
    assert_eq!(enabled.len(), 1);
    assert_eq!(enabled[0].name, friday_bridge::CapabilityName::Metrics);

    selections.insert("gzip".to_string(), true); // on but not advertised
    let err = set.resolve(&selections).unwrap_err();
    assert_eq!(err, "gzip", "unknown names are rejected by name");
}

#[test]
fn error_frame_carries_full_machine_readable_taxonomy() {
    let frame = Frame::Error(friday_bridge::protocol::ErrorFrame {
        sequence: 5,
        timestamp: 1,
        source: "mock".into(),
        protocol: 1,
        bridge: "b".into(),
        message: "capability `gzip` is not advertised by this bridge".into(),
        code: Some(friday_bridge::ErrorCode::UnsupportedCapability),
        severity: Some(friday_bridge::Severity::Warning),
        recoverable: Some(true),
        recommended_action: Some("drop the unsupported capability and retry".into()),
    });
    let json = FrameSerializer::to_json(&frame).unwrap();
    let value: serde_json::Value = serde_json::from_str(&json).unwrap();

    assert_eq!(value["kind"], "error");
    assert_eq!(value["code"], "unsupported_capability");
    assert_eq!(value["severity"], "warning");
    assert_eq!(value["recoverable"], true);
    assert_eq!(value["recommended_action"], "drop the unsupported capability and retry");

    let parsed = FrameSerializer::from_json(&json).unwrap();
    assert_eq!(parsed, frame, "round-trip must be lossless");
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
    .with_heartbeat_ms(75)
    .with_negotiation_timeout_ms(150);
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
        if let Some(Ok(tokio_tungstenite::tungstenite::Message::Text(text))) = ws.next().await {
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

async fn next_snapshot(
    ws: &mut tokio_tungstenite::WebSocketStream<
        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
    >,
) -> serde_json::Value {
    loop {
        let frame = next_frame(ws).await;
        if frame["kind"] == "snapshot" {
            return frame;
        }
    }
}

#[tokio::test]
async fn client_can_request_a_resync_snapshot() {
    let addr = spawn_bridge().await;
    let url = format!("ws://{addr}");
    let (mut ws, _resp) = connect_async(&url).await.unwrap();

    let hello = next_frame(&mut ws).await;
    assert_eq!(hello["kind"], "hello");
    assert!(hello["bridge_stats"].is_object(), "hello carries bridge stats");
    let snapshot1 = next_frame(&mut ws).await;
    assert_eq!(snapshot1["kind"], "snapshot");

    // A console that lost frames asks for a fresh snapshot mid-stream.
    ws.send(tokio_tungstenite::tungstenite::Message::Text(
        r#"{"type":"resync"}"#.into(),
    ))
    .await
    .unwrap();

    let resync = next_snapshot(&mut ws).await;
    assert!(
        resync["sequence"].as_u64().unwrap() > snapshot1["sequence"].as_u64().unwrap(),
        "resync continues the stream: {} -> {}",
        snapshot1["sequence"],
        resync["sequence"]
    );
    assert!(resync["ledger"].is_array(), "resync is a full snapshot");
    let _ = ws.close(None).await;
}

#[tokio::test]
async fn hello_advertises_capabilities_and_negotiation_is_acked() {
    let addr = spawn_bridge().await;
    let url = format!("ws://{addr}");
    let (mut ws, _resp) = connect_async(&url).await.unwrap();

    let hello = next_frame(&mut ws).await;
    assert_eq!(hello["kind"], "hello");
    let caps = hello["capabilities"].clone();
    let required: Vec<String> = caps["required"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["name"].as_str().unwrap().to_string())
        .collect();
    assert!(required.contains(&"delta".to_string()));
    assert!(required.contains(&"heartbeat".to_string()));
    assert!(required.contains(&"resync".to_string()));
    assert!(required.contains(&"errors".to_string()));

    // The console enables the optional `metrics` capability.
    ws.send(tokio_tungstenite::tungstenite::Message::Text(
        r#"{"type":"hello_response","capabilities":{"metrics":true}}"#.into(),
    ))
    .await
    .unwrap();

    // The server acknowledges with a `capabilities` frame *before* the snapshot.
    let ack = next_frame(&mut ws).await;
    assert_eq!(ack["kind"], "capabilities");
    let negotiated: Vec<String> = ack["capabilities"]["optional"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["name"].as_str().unwrap().to_string())
        .collect();
    assert!(negotiated.contains(&"metrics".to_string()));

    let snapshot = next_frame(&mut ws).await;
    assert_eq!(snapshot["kind"], "snapshot");
    let _ = ws.close(None).await;
}

#[tokio::test]
async fn unknown_capability_gets_a_structured_error_but_stays_connected() {
    let addr = spawn_bridge().await;
    let url = format!("ws://{addr}");
    let (mut ws, _resp) = connect_async(&url).await.unwrap();

    next_frame(&mut ws).await; // hello
    ws.send(tokio_tungstenite::tungstenite::Message::Text(
        r#"{"type":"hello_response","capabilities":{"gzip":true}}"#.into(),
    ))
    .await
    .unwrap();

    let err = next_frame(&mut ws).await;
    assert_eq!(err["kind"], "error");
    assert_eq!(err["code"], "unsupported_capability");
    assert_eq!(err["severity"], "warning");
    assert_eq!(err["recoverable"], true);
    assert!(
        err["recommended_action"].as_str().unwrap().contains("retry"),
        "guidance must be actionable: {err}"
    );

    // The connection survives the failed negotiation.
    let snapshot = next_frame(&mut ws).await;
    assert_eq!(snapshot["kind"], "snapshot");
    let _ = ws.close(None).await;
}

#[tokio::test]
async fn old_client_without_negotiation_still_streams() {
    let addr = spawn_bridge().await;
    let url = format!("ws://{addr}");
    let (mut ws, _resp) = connect_async(&url).await.unwrap();

    // No hello_response at all: the handshake must still complete.
    let hello = next_frame(&mut ws).await;
    assert_eq!(hello["kind"], "hello");
    let snapshot = next_frame(&mut ws).await;
    assert_eq!(snapshot["kind"], "snapshot");
    assert_eq!(snapshot["timelines"].as_array().unwrap().len(), 5);

    // And it must keep streaming (a delta or heartbeat arrives).
    let deadline = tokio::time::Instant::now() + Duration::from_millis(300);
    let mut saw_live = false;
    while tokio::time::Instant::now() < deadline {
        if let Some(Ok(tokio_tungstenite::tungstenite::Message::Text(text))) = ws.next().await {
            let kind = serde_json::from_str::<serde_json::Value>(&text).unwrap();
            if kind["kind"] == "delta" || kind["kind"] == "heartbeat" {
                saw_live = true;
                break;
            }
        }
    }
    assert!(saw_live, "a non-negotiating client must still receive live frames");
    let _ = ws.close(None).await;
}

#[tokio::test]
async fn client_sending_a_typed_frame_gets_a_structured_invalid_frame_error() {
    let addr = spawn_bridge().await;
    let url = format!("ws://{addr}");
    let (mut ws, _resp) = connect_async(&url).await.unwrap();

    next_frame(&mut ws).await; // hello
    next_frame(&mut ws).await; // snapshot

    // Clients may only send control messages; a typed frame is illegal.
    ws.send(tokio_tungstenite::tungstenite::Message::Text(
        r#"{"kind":"delta","sequence":999,"timestamp":1,"source":"x","protocol":1,"bridge":"x"}"#
            .into(),
    ))
    .await
    .unwrap();

    let err = next_frame(&mut ws).await;
    assert_eq!(err["kind"], "error");
    assert_eq!(err["code"], "invalid_frame");
    assert_eq!(err["severity"], "warning");

    // Still alive afterwards.
    let deadline = tokio::time::Instant::now() + Duration::from_millis(300);
    let mut saw_live = false;
    while tokio::time::Instant::now() < deadline {
        if let Some(Ok(tokio_tungstenite::tungstenite::Message::Text(text))) = ws.next().await {
            let kind = serde_json::from_str::<serde_json::Value>(&text).unwrap();
            if kind["kind"] == "delta" || kind["kind"] == "heartbeat" {
                saw_live = true;
                break;
            }
        }
    }
    assert!(saw_live, "the connection must survive a rejected typed frame");
    let _ = ws.close(None).await;
}

#[tokio::test]
async fn heartbeat_carries_bridge_wide_stats() {
    let addr = spawn_bridge().await;
    let url = format!("ws://{addr}");
    let (mut ws, _resp) = connect_async(&url).await.unwrap();

    next_frame(&mut ws).await; // hello
    next_frame(&mut ws).await; // snapshot

    let deadline = tokio::time::Instant::now() + Duration::from_millis(500);
    let mut stats: Option<serde_json::Value> = None;
    while tokio::time::Instant::now() < deadline {
        if let Some(Ok(tokio_tungstenite::tungstenite::Message::Text(text))) = ws.next().await {
            let value: serde_json::Value = serde_json::from_str(&text).unwrap();
            if value["kind"] == "heartbeat" {
                let bs = value["bridge_stats"].clone();
                assert!(bs.is_object(), "heartbeat carries bridge_stats: {value}");
                assert!(bs["connections"].as_u64().unwrap() >= 1);
                assert!(bs["reconnects"].as_u64().unwrap() >= 1);
                assert!(bs["frames_sent"].as_u64().unwrap() > 0);
                assert!(bs["bytes_sent"].as_u64().unwrap() > 0);
                stats = Some(bs);
                break;
            }
        }
    }

    let stats = stats.expect("expected a heartbeat with bridge_stats");
    assert!(
        stats["snapshot_size_bytes"].as_u64().unwrap() > 0,
        "snapshot size measured: {stats}"
    );
    assert!(
        stats["delta_size_bytes"].as_u64().unwrap() > 0,
        "delta size measured: {stats}"
    );
    let _ = ws.close(None).await;
}

#[tokio::test]
async fn reconnects_are_counted_and_active_connections_tracked() {
    let addr = spawn_bridge().await;
    let url = format!("ws://{addr}");

    let (mut conn1, _resp) = connect_async(&url).await.unwrap();
    let hello1 = next_frame(&mut conn1).await;
    assert_eq!(hello1["bridge_stats"]["reconnects"], 1);
    assert_eq!(hello1["bridge_stats"]["connections"], 1);
    conn1.close(None).await.unwrap();

    // Give the server a moment to observe the close.
    tokio::time::sleep(Duration::from_millis(50)).await;

    let (mut conn2, _resp) = connect_async(&url).await.unwrap();
    let hello2 = next_frame(&mut conn2).await;
    assert_eq!(
        hello2["bridge_stats"]["reconnects"], 2,
        "second connect is a reconnect"
    );
    assert_eq!(
        hello2["bridge_stats"]["connections"], 1,
        "only conn2 is active after conn1 closed"
    );
    let _ = conn2.close(None).await;
}
