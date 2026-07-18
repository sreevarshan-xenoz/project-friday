//! PR1 hermetic tests: JSON-RPC protocol over stdio-shaped transport.
//!
//! No subprocess, no Node. A [`server::FakeMcpServer`] speaks real MCP over an
//! in-memory channel; the client is the production [`protocol::McpClient`].

use std::time::Duration;

use friday_mcp::protocol::{McpClient, McpClientConfig, McpError};
use friday_mcp::server::{FakeMcpServer, FakeTransport};
use friday_mcp::transport::Transport;
use serde_json::json;

#[tokio::test]
async fn initialize_handshake_succeeds() {
    // `pair()` cross-wires the channels: client.outbound -> server.inbound and
    // server.outbound -> client.inbound, so the client and server each get a
    // `FakeTransport` that talks to the other.
    let (client_side, server_side) = FakeTransport::pair();
    let server = FakeMcpServer::new();
    tokio::spawn(async move { server.serve(server_side).await });

    let mut client = McpClient::new(client_side);
    let result = client.initialize().await.expect("initialize ok");
    assert_eq!(result.protocol_version, "2024-11-05");
}

#[tokio::test]
async fn list_tools_returns_advertised_tools() {
    let (client_side, server_side) = FakeTransport::pair();
    let server = FakeMcpServer::new();
    tokio::spawn(async move { server.serve(server_side).await });

    let mut client = McpClient::new(client_side);
    client.initialize().await.unwrap();
    let tools = client.list_tools().await.expect("list_tools ok");
    let names: Vec<&str> = tools.iter().map(|t| t.name.as_str()).collect();
    assert!(names.contains(&"read_file"));
    assert!(names.contains(&"write_file"));
    // Tool schemas are surfaced (needed by the adapter later).
    let read = tools.iter().find(|t| t.name == "read_file").unwrap();
    assert!(read.input_schema.get("properties").is_some());
}

#[tokio::test]
async fn call_tool_round_trips_arguments_and_result() {
    let (client_side, server_side) = FakeTransport::pair();
    let server = FakeMcpServer::new().with_call_handler(|name, args| {
        json!({
            "content": [ { "type": "text", "text": format!("ran {name} with {args}") } ],
            "is_error": false
        })
    });
    tokio::spawn(async move { server.serve(server_side).await });

    let mut client = McpClient::new(client_side);
    client.initialize().await.unwrap();
    let res = client
        .call_tool("read_file", json!({ "path": "Cargo.toml" }))
        .await
        .expect("call_tool ok");
    assert!(!res.is_error);
    let text = res
        .content
        .first()
        .and_then(|c| c.get("text"))
        .and_then(|t| t.as_str())
        .unwrap();
    assert!(text.contains("read_file"), "result echoes tool name");
    assert!(text.contains("Cargo.toml"), "result echoes arguments");
}

#[tokio::test]
async fn call_unknown_tool_marks_error() {
    let (client_side, server_side) = FakeTransport::pair();
    let server = FakeMcpServer::new();
    tokio::spawn(async move { server.serve(server_side).await });

    let mut client = McpClient::new(client_side);
    client.initialize().await.unwrap();
    let res = client
        .call_tool("nonexistent_tool", json!({}))
        .await
        .expect("call_tool returns a result, not a transport error");
    assert!(res.is_error, "unknown tool must be flagged as an error result");
}

#[tokio::test]
async fn protocol_reports_server_rpc_errors() {
    // A method the server genuinely does not implement returns a JSON-RPC error.
    let (client_side, server_side) = FakeTransport::pair();
    let server = FakeMcpServer::new();
    tokio::spawn(async move { server.serve(server_side).await });

    let mut client = McpClient::new(client_side);
    client.initialize().await.unwrap();
    // `ping` is not implemented by the fake -> RPC error surfaced as McpError.
    let err = client.request_unchecked("ping", json!({})).await.unwrap_err();
    assert!(format!("{err}").contains("method not found"));
}

#[tokio::test]
async fn request_timeout_fires_when_server_never_replies() {
    // A server that accepts the connection but never responds to any request.
    // With a short configured timeout the client must fail with `McpError::Timeout`
    // rather than hang forever.
    let (client_side, mut server_side) = FakeTransport::pair();
    tokio::spawn(async move {
        // Read the request and then block forever (no response).
        let _ = server_side.recv().await;
        tokio::time::sleep(Duration::from_secs(60)).await;
    });

    let mut client = McpClient::with_config(
        client_side,
        McpClientConfig {
            request_timeout: Some(Duration::from_millis(150)),
        },
    );
    let err = client.initialize().await.unwrap_err();
    assert!(
        matches!(err, McpError::Timeout(..)),
        "expected Timeout, got {err:?}"
    );
}

#[tokio::test]
async fn initialize_drains_post_handshake_notifications() {
    // The fake server emits `notifications/initialized` (an id-less frame)
    // after the initialize response. The client must tolerate it and still
    // return a clean list_tools, proving notification draining is semantic
    // (drops only id-less frames) rather than a blind bounded poll.
    let (client_side, server_side) = FakeTransport::pair();
    let server = FakeMcpServer::new().with_notification_after_initialize(
        json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }),
    );
    tokio::spawn(async move { server.serve(server_side).await });

    let mut client = McpClient::new(client_side);
    client.initialize().await.expect("initialize ok despite notification");
    let tools = client.list_tools().await.expect("list_tools ok after notification");
    assert!(!tools.is_empty(), "tools listed after notification drained");
}
