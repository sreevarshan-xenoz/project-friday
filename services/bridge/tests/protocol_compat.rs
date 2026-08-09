//! Protocol Compatibility Suite — the frozen wire contract.
//!
//! `protocol-tests/v1/*.json` are golden byte samples of every historical
//! frame kind, produced by `examples/dump_protocol.rs` through the real
//! serializer. These tests enforce the two promises of the suite:
//!
//!   1. **Parse** — every committed frame parses losslessly today, so a frame
//!      written by an old bridge can always be read by a new one.
//!   2. **Byte-lock** — re-serializing a parsed fixture reproduces the golden
//!      bytes exactly, so the wire format cannot drift silently. A format
//!      change requires regenerating the fixtures *and* updating this suite.
//!
//! Control messages (client → server) are validated structurally below.

use std::fs;
use std::path::PathBuf;

use friday_bridge::{Frame, FrameSerializer, PROTOCOL_VERSION};

const FRAME_FIXTURES: [&str; 6] = [
    "hello",
    "capabilities",
    "snapshot",
    "delta",
    "heartbeat",
    "error",
];

fn v1_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("protocol-tests")
        .join("v1")
}

fn fixture(name: &str) -> String {
    let path = v1_dir().join(format!("{name}.json"));
    let raw = fs::read_to_string(&path).unwrap_or_else(|e| panic!("read fixture {name}: {e}"));
    raw.trim_end().to_string()
}

fn frame_meta(frame: &Frame) -> (&'static str, u32) {
    match frame {
        Frame::Hello(f) => ("hello", f.protocol),
        Frame::Snapshot(f) => ("snapshot", f.protocol),
        Frame::Delta(f) => ("delta", f.protocol),
        Frame::Heartbeat(f) => ("heartbeat", f.protocol),
        Frame::Capabilities(f) => ("capabilities", f.protocol),
        Frame::Error(f) => ("error", f.protocol),
    }
}

#[test]
fn every_v1_fixture_parses_and_round_trips_byte_identical() {
    for name in FRAME_FIXTURES {
        let golden = fixture(name);
        let frame: Frame = FrameSerializer::from_json(&golden)
            .unwrap_or_else(|e| panic!("{name} must parse: {e}"));

        let (kind, protocol) = frame_meta(&frame);
        assert_eq!(kind, name, "fixture filename must match its kind");
        assert_eq!(
            protocol, PROTOCOL_VERSION,
            "{name} must carry the current protocol version"
        );

        let reencoded = FrameSerializer::to_json(&frame)
            .unwrap_or_else(|e| panic!("{name} must serialize: {e}"));
        assert_eq!(
            reencoded, golden,
            "{name} drifted: the golden fixture no longer matches the serializer output. \
             Regenerate with `cargo run -p friday-bridge --example dump_protocol` and \
             commit the new bytes together with the change."
        );
    }
}

#[test]
fn control_messages_match_the_frozen_client_shapes() {
    let hello_response: serde_json::Value =
        serde_json::from_str(&fixture("control-hello-response")).unwrap();
    assert_eq!(hello_response["type"], "hello_response");
    let caps = &hello_response["capabilities"];
    assert!(
        caps.is_object(),
        "hello_response selects capabilities by name"
    );
    assert_eq!(
        caps["metrics"], true,
        "the console fixture enables the optional `metrics` capability"
    );

    let resync: serde_json::Value = serde_json::from_str(&fixture("control-resync")).unwrap();
    assert_eq!(resync["type"], "resync");
}

#[test]
fn error_fixture_carries_the_full_machine_readable_taxonomy() {
    let golden = fixture("error");
    let frame: Frame = FrameSerializer::from_json(&golden).unwrap();
    let Frame::Error(err) = frame else {
        panic!("error fixture must parse as an Error frame");
    };
    assert_eq!(
        err.code,
        Some(friday_bridge::ErrorCode::UnsupportedCapability)
    );
    assert_eq!(err.severity, Some(friday_bridge::Severity::Warning));
    assert_eq!(err.recoverable, Some(true));
    assert!(err.recommended_action.is_some());
}
