//! End-to-end protocol tests for the FRIDAY ReadModelBridge.
//!
//! Covers:
//!   1. Frame serialization round-trips into the exact JSON the console reads.
//!   2. The demo source produces a plausible snapshot + deterministic ticks.
//!   3. A live client over TCP/WebSocket sees hello -> snapshot -> delta ->
//!      heartbeat with contiguous sequences and a measured round-trip.

use std::fs;
use std::net::SocketAddr;
use std::process::Command;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use friday_bridge::{
    prometheus_metrics, BridgeServer, DemoRuntimeSource, Frame, FrameSerializer, LiveRuntimeSource,
    RuntimeSource, SourceDelta,
};
use futures::{SinkExt, StreamExt};
use tokio_tungstenite::connect_async;

// ---------------------------------------------------------------------------
// Unit-ish: frame serialization
// ---------------------------------------------------------------------------

#[test]
fn prometheus_metrics_exposes_bridge_counters() {
    let stats = friday_bridge::connection::BridgeStats::default();
    stats.connect();
    stats.record_send(128);
    let body = prometheus_metrics(&stats.snapshot());
    assert!(body.contains("friday_bridge_connections_active 1"));
    assert!(body.contains("friday_bridge_frames_sent_total 1"));
    assert!(body.contains("friday_bridge_bytes_sent_total 128"));
}

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

#[tokio::test]
async fn frame_round_trips_through_json() {
    let mut source = DemoRuntimeSource::new(42, 8);
    let snapshot = source.snapshot().await;
    let mut deltas = source.tick(1).await;
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
    let bad =
        r#"{"kind":"hello","sequence":1,"timestamp":1,"source":"mock","protocol":99,"bridge":"b"}"#;
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
    assert_eq!(
        value["recommended_action"],
        "drop the unsupported capability and retry"
    );

    let parsed = FrameSerializer::from_json(&json).unwrap();
    assert_eq!(parsed, frame, "round-trip must be lossless");
}

// ---------------------------------------------------------------------------
// Unit-ish: demo source
// ---------------------------------------------------------------------------

#[tokio::test]
async fn live_source_exposes_real_autonomy_provenance() {
    let dir = std::env::temp_dir().join(format!(
        "friday-live-source-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&dir).unwrap();
    let repo = dir.to_string_lossy().to_string();
    Command::new("git")
        .args(["init", "--quiet", &repo])
        .status()
        .unwrap();
    fs::write(dir.join("live-autonomy-marker.txt"), "safe\n").unwrap();

    let journal_path = dir.join("journal.json");
    let source = LiveRuntimeSource::new(&dir, &journal_path).await;
    let snapshot = source.snapshot().await;
    let trace = snapshot.dashboard.autonomy.expect("live source trace");
    let action = trace.action.expect("decision event");
    let execution = trace.execution.expect("execution event");
    let outcome = trace.outcome.expect("outcome event");
    assert_eq!(action.payload["action"], "git.status");
    assert_eq!(execution.payload["success"], true);
    assert_eq!(outcome.payload["verified"], true);
    assert!(!action.event_id.is_empty());
    assert!(!execution.event_id.is_empty());
    assert!(!outcome.event_id.is_empty());
    assert!(action.timestamp_ms <= execution.timestamp_ms);
    assert!(execution.timestamp_ms <= outcome.timestamp_ms);

    // A fresh runtime instance rehydrates the completed trace and does not
    // create a second journal record or redispatch git.status.
    let recovered_source = LiveRuntimeSource::new(&dir, &journal_path).await;
    let recovered = recovered_source.snapshot().await;
    let recovered_trace = recovered.dashboard.autonomy.expect("recovered trace");
    assert_eq!(recovered_trace.outcome.unwrap().payload["recovered"], true);
    let records: serde_json::Value =
        serde_json::from_slice(&fs::read(&journal_path).unwrap()).unwrap();
    assert_eq!(records.as_array().unwrap().len(), 1);
    let _ = fs::remove_dir_all(dir);
}

#[tokio::test]
async fn demo_source_seeds_read_models_through_frozen_engines() {
    let source = DemoRuntimeSource::new(42, 140);
    assert_eq!(source.bridge_id(), "friday-bridge-demo");
    assert_eq!(source.source_name(), "mock");

    let snap = source.snapshot().await;
    assert_eq!(snap.timelines.len(), 140, "one timeline per seeded record");
    assert_eq!(snap.ledger.len(), 140);
    assert_eq!(snap.metrics.decisions, 140);
    assert_eq!(snap.dashboard.executive.len(), 11, "eleven-phase executive");

    // Determinism: same seed => same metrics.
    let again = DemoRuntimeSource::new(42, 140).snapshot().await;
    assert_eq!(snap.metrics, again.metrics);
}

#[tokio::test]
async fn demo_source_retention_window_emits_recent_lists_and_keeps_metrics_cumulative() {
    // MECH-0002 C2: with a retention window, the bridge read model (and thus
    // the emitted timelines/ledger lists) is bounded to the recent window,
    // while metrics/episodes stay cumulative over full history.
    let mut source = DemoRuntimeSource::with_retention(42, 140, 8);
    let snap = source.snapshot().await;

    assert_eq!(snap.timelines.len(), 8, "read model bounded to the window");
    assert_eq!(snap.ledger.len(), 8, "ledger list is the recent window");
    assert_eq!(snap.metrics.decisions, 140, "metrics fold in full history");
    assert_eq!(snap.dashboard.world.episodes, 140, "episodes stay cumulative");

    let first_before = snap.ledger[0].id.clone();

    // A full executive cycle appends one record: the window slides, oldest is
    // evicted, the list stays at 8.
    let mut deltas = Vec::new();
    for _ in 0..11 {
        deltas.extend(source.tick(1).await);
    }
    let ledger = deltas
        .into_iter()
        .rev()
        .find_map(|d| d.ledger)
        .expect("cycle completion carries the ledger slice");
    assert_eq!(ledger.len(), 8, "window stays bounded as history grows");
    assert_ne!(
        ledger[0].id, first_before,
        "oldest record evicted when the window slides"
    );
}

#[tokio::test]
async fn demo_source_tick_advances_executive_and_appends_on_cycle_completion() {
    let mut source = DemoRuntimeSource::new(42, 5);

    // First ten ticks advance the executive without touching the ledger.
    for _ in 0..10 {
        let deltas = source.tick(1).await;
        assert_eq!(deltas.len(), 1, "one delta per tick");
        let d: &SourceDelta = &deltas[0];
        assert_eq!(d.slices, vec![friday_bridge::Slice::Dashboard]);
        assert!(d.dashboard.is_some());
        assert!(
            d.ledger.is_none(),
            "ledger untouched until a cycle completes"
        );
    }

    // Eleventh tick completes a cycle: a record is appended and every slice
    // follows along.
    let deltas = source.tick(1).await;
    let d = &deltas[0];
    assert_eq!(d.slices.len(), 4);
    assert!(d.timelines.is_some());
    assert!(d.metrics.is_some());
    assert!(d.ledger.is_some());
    assert_eq!(d.ledger.as_ref().unwrap().len(), 6);
}

#[tokio::test]
async fn demo_source_graph_comes_from_the_real_executive_runtime() {
    // The dashboard's InvestigationGraph is the goal-execution Executive's own
    // evidence-gate outcome — the first demo goal genuinely runs through the
    // runtime (investigate → plan → …) and its branches appear on the graph.
    let source = DemoRuntimeSource::new(42, 5);
    let snap = source.snapshot().await;
    let graph = snap.dashboard.graph.expect("snapshot carries the graph");

    assert_eq!(
        graph.goal,
        friday_bridge::demo_goals()[0],
        "graph reflects the goal the runtime processed"
    );
    let channels: Vec<&str> = graph
        .requirements
        .iter()
        .map(|r| r.channel.as_str())
        .collect();
    assert!(
        channels.contains(&"web"),
        "web evidence branch from the executive's investigation: {channels:?}"
    );
    assert!(
        graph.phases.iter().any(|p| p == "investigate"),
        "evidence goal carries the investigate phase: {:?}",
        graph.phases
    );

    // A full cycle later the runtime advances to the next goal and its graph
    // follows — the trace reflects the Executive, not a static demo cycle.
    let mut source = source;
    for _ in 0..11 {
        source.tick(1).await;
    }
    let snap = source.snapshot().await;
    let graph = snap.dashboard.graph.expect("graph follows the runtime");
    assert_eq!(graph.goal, friday_bridge::demo_goals()[1]);
}

#[tokio::test]
async fn demo_source_evidence_trail_shows_the_executed_lifecycle() {
    // The dashboard's evidence trail is the Execute-phase segment of the
    // cognitive trace, read back from the Executive's own workspace: goal 0
    // (web + docs) runs two evidence actions through the evidence lifecycle
    // and both end Verified.
    let source = DemoRuntimeSource::new(42, 5);
    let snap = source.snapshot().await;

    let evidence = snap
        .dashboard
        .evidence
        .expect("evidence-gated goal carries an evidence trail");
    assert_eq!(evidence.goal, friday_bridge::demo_goals()[0]);

    let channels: Vec<&str> = evidence
        .records
        .iter()
        .map(|r| r.channel.as_str())
        .collect();
    assert!(channels.contains(&"web"), "channels: {channels:?}");
    assert!(channels.contains(&"docs"), "channels: {channels:?}");

    // Mock tools succeed, so every record ends the lifecycle Verified — and
    // the wire carries the real skill capability ids (per-action timing is 0
    // for the instant mock tools, which is the honest recorded duration).
    for record in &evidence.records {
        assert_eq!(record.execution_state, "verified", "{record:?}");
        assert!(record.verified, "{record:?}");
        assert!(record.tool_success, "{record:?}");
        assert!(
            record.capability.contains('.'),
            "capability is a skill id: {record:?}"
        );
    }
}

#[tokio::test]
async fn demo_source_self_contained_goal_has_no_investigate_phase() {
    // Three full cycles advance to goal index 3 ("Open notepad…"). The
    // Executive's investigation of a self-contained goal yields no branches
    // and a `plan → execute` chain — the runtime's real judgment.
    let mut source = DemoRuntimeSource::new(42, 5);
    source.snapshot().await; // runs goal 0
    for _ in 0..33 {
        source.tick(1).await; // wraps three times → goals 1, 2, 3
    }
    let snap = source.snapshot().await;
    let graph = snap.dashboard.graph.expect("graph follows the runtime");

    assert_eq!(graph.goal, friday_bridge::demo_goals()[3]);
    assert!(
        graph.requirements.is_empty(),
        "self-contained goal has no evidence branches"
    );
    assert_eq!(
        graph.phases,
        vec!["plan".to_string(), "execute".to_string()]
    );
    assert!(
        snap.dashboard.evidence.is_none(),
        "self-contained goal gathers no evidence — no lifecycle to render"
    );
}

// ---------------------------------------------------------------------------
// Integration: live WebSocket client
// ---------------------------------------------------------------------------

async fn spawn_bridge() -> SocketAddr {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    drop(listener);

    let server = BridgeServer::new(addr, Box::new(DemoRuntimeSource::new(42, 5)))
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

    // Collect the stream for up to 1s and verify the frame grammar. The
    // larger window accommodates Windows scheduling and a real source doing
    // Executive work before its first measured heartbeat.
    let mut kinds = Vec::new();
    let mut sequences = Vec::new();
    let mut saw_heartbeat_with_rtt = false;
    let deadline = tokio::time::Instant::now() + Duration::from_millis(1000);
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
    assert!(
        kinds.contains(&"delta".to_string()),
        "expected deltas: {kinds:?}"
    );
    for pair in sequences.windows(2) {
        assert_eq!(
            pair[1],
            pair[0] + 1,
            "sequences must be contiguous: {sequences:?}"
        );
    }
    let _ = ws.close(None).await;
}

async fn spawn_live_bridge() -> (SocketAddr, std::path::PathBuf) {
    let dir = std::env::temp_dir().join(format!(
        "friday-live-bridge-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&dir).unwrap();
    let repo = dir.to_string_lossy().to_string();
    Command::new("git")
        .args(["init", "--quiet", &repo])
        .status()
        .unwrap();
    fs::write(dir.join("live-autonomy-marker.txt"), "safe\n").unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    drop(listener);
    let source = LiveRuntimeSource::new(&dir, dir.join("journal.json")).await;
    let server = BridgeServer::new(addr, Box::new(source))
        .with_metrics_addr(None)
        .with_negotiation_timeout_ms(20);
    tokio::spawn(async move {
        let _ = server.run().await;
    });
    tokio::time::sleep(Duration::from_millis(50)).await;
    (addr, dir)
}

#[tokio::test]
async fn live_bridge_streams_real_autonomy_provenance() {
    let (addr, dir) = spawn_live_bridge().await;
    let (mut ws, _) = connect_async(format!("ws://{addr}")).await.unwrap();
    let hello = next_frame(&mut ws).await;
    assert_eq!(hello["bridge"], "friday-bridge-live");
    let snapshot = next_frame(&mut ws).await;
    assert_eq!(snapshot["kind"], "snapshot");
    let autonomy = &snapshot["dashboard"]["autonomy"];
    assert_eq!(autonomy["action"]["payload"]["action"], "git.status");
    assert_eq!(autonomy["execution"]["payload"]["success"], true);
    assert_eq!(autonomy["outcome"]["payload"]["verified"], true);
    assert!(autonomy["action"]["event_id"].is_string());
    assert!(autonomy["outcome"]["timestamp_ms"].is_number());
    let _ = ws.close(None).await;
    let _ = fs::remove_dir_all(dir);
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
    assert!(
        hello["bridge_stats"].is_object(),
        "hello carries bridge stats"
    );
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
        err["recommended_action"]
            .as_str()
            .unwrap()
            .contains("retry"),
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
    assert!(
        saw_live,
        "a non-negotiating client must still receive live frames"
    );
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

    // The error must arrive promptly, but a live broadcast delta can legally
    // interleave first — drain until the structured error shows up.
    let err = {
        let deadline = tokio::time::Instant::now() + Duration::from_millis(300);
        let mut err = None;
        while tokio::time::Instant::now() < deadline {
            let frame = next_frame(&mut ws).await;
            if frame["kind"] == "error" {
                err = Some(frame);
                break;
            }
        }
        err.expect("structured error for a typed frame")
    };
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
    assert!(
        saw_live,
        "the connection must survive a rejected typed frame"
    );
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
