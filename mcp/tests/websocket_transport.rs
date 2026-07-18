//! PR4: WebSocket transport round-trip test (hermetic, in-process server).
//!
//! Spins up a local tokio-tungstenite server that speaks a tiny MCP-shaped
//! protocol (initialize + list_tools + call_tool) over JSON-RPC text frames,
//! then drives it through `McpClient<WebSocketTransport>` exactly as stdio is
//! exercised in `transport_protocol.rs`. No external `npx`/Node involved.

use std::net::SocketAddr;

use friday_mcp::protocol::{McpClient, McpTool};
use friday_mcp::transport::Transport;
use friday_mcp::transport_ws::{websocket_transport_for, WebSocketTransport};
use futures::{SinkExt, StreamExt};
use tokio::net::TcpListener;
use tokio_tungstenite::{accept_async, tungstenite::Message};

async fn spawn_ws_mcp_server(tools: Vec<McpTool>) -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let (sock, _) = listener.accept().await.unwrap();
        let mut ws = accept_async(sock).await.unwrap();
        while let Some(Ok(msg)) = ws.next().await {
            let text = match msg {
                Message::Text(t) => t.to_string(),
                Message::Close(_) => break,
                _ => continue,
            };
            let v: serde_json::Value = match serde_json::from_str(&text) {
                Ok(v) => v,
                Err(_) => continue,
            };
            let id = v.get("id").and_then(|x| x.as_u64()).unwrap_or(0);
            let method = v.get("method").and_then(|x| x.as_str()).unwrap_or("");
            let response = match method {
                "initialize" => serde_json::json!({
                    "jsonrpc": "2.0", "id": id,
                    "result": { "protocolVersion": "2025-06-18", "capabilities": {}, "serverInfo": { "name": "fake-ws", "version": "0.0.0" } }
                }),
                "tools/list" => {
                    let list: Vec<serde_json::Value> = tools
                        .iter()
                        .map(|t| serde_json::json!({
                            "name": t.name, "description": t.description, "inputSchema": t.input_schema
                        }))
                        .collect();
                    serde_json::json!({
                        "jsonrpc": "2.0", "id": id,
                        "result": { "tools": list }
                    })
                }
                "tools/call" => {
                    serde_json::json!({
                        "jsonrpc": "2.0", "id": id,
                        "result": { "content": [{ "type": "text", "text": "ws-ok" }], "isError": false }
                    })
                }
                _ => serde_json::json!({
                    "jsonrpc": "2.0", "id": id,
                    "error": { "code": -32601, "message": "method not found" }
                }),
            };
            let out = Message::Text(serde_json::to_string(&response).unwrap().into());
            let _ = ws.send(out).await;
        }
    });
    addr
}

async fn fake_tools() -> Vec<McpTool> {
    vec![
        McpTool {
            name: "read".to_string(),
            description: "read a file".to_string(),
            input_schema: serde_json::json!({"type": "object", "properties": {"path": {"type": "string"}}}),
        },
        McpTool {
            name: "write".to_string(),
            description: "write a file".to_string(),
            input_schema: serde_json::json!({"type": "object", "properties": {"path": {"type": "string"}}}),
        },
    ]
}

#[tokio::test]
async fn websocket_initialize_and_list() {
    let addr = spawn_ws_mcp_server(fake_tools().await).await;
    let transport = websocket_transport_for(&format!("ws://{addr}/mcp"))
        .await
        .expect("connect");
    let mut client = McpClient::new(transport);
    client.initialize().await.expect("initialize");
    let tools = client.list_tools().await.expect("list_tools");
    assert_eq!(tools.len(), 2);
    assert_eq!(tools[0].name, "read");
    assert_eq!(tools[1].name, "write");
}

#[tokio::test]
async fn websocket_call_tool() {
    let addr = spawn_ws_mcp_server(fake_tools().await).await;
    let transport = websocket_transport_for(&format!("ws://{addr}/mcp"))
        .await
        .expect("connect");
    let mut client = McpClient::new(transport);
    client.initialize().await.expect("initialize");
    let res = client
        .call_tool("read", serde_json::json!({"path": "/tmp/x"}))
        .await
        .expect("call_tool");
    assert!(!res.is_error);
    let text = res
        .content
        .iter()
        .find_map(|c| c.get("text").and_then(|t| t.as_str()).map(|s| s.to_string()))
        .expect("content text");
    assert_eq!(text, "ws-ok");
}

#[tokio::test]
async fn websocket_transport_framing() {
    // Direct transport-level check: a send is observable as a text frame on the
    // server, and a server text frame is received via recv.
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let (sock, _) = listener.accept().await.unwrap();
        let mut ws = accept_async(sock).await.unwrap();
        // echo first received text frame back as a new text frame
        if let Some(Ok(Message::Text(t))) = ws.next().await {
            let _ = ws.send(Message::Text(t)).await;
        }
        let _ = ws.close(None).await;
    });
    let mut transport = websocket_transport_for(&format!("ws://{addr}/mcp"))
        .await
        .expect("connect");
    transport.send("{\"hello\":\"ws\"}").await.expect("send");
    let got = transport.recv().await.expect("recv echo");
    assert_eq!(got, "{\"hello\":\"ws\"}");
}

/// The transport must answer a server `Ping` with a `Pong` automatically
/// (transport responsibility), so idle connections survive. A server that
/// sends a ping and receives no pong would drop the client.
#[tokio::test]
async fn websocket_auto_pong_on_server_ping() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let pong_rx = tokio::spawn(async move {
        let (sock, _) = listener.accept().await.unwrap();
        let mut ws = accept_async(sock).await.unwrap();
        // Send a ping immediately after the handshake.
        ws.send(Message::Ping(vec![1, 2, 3].into())).await.unwrap();
        // Expect the client's automatic pong back.
        let got = ws.next().await;
        matches!(got, Some(Ok(Message::Pong(_))))
    });
    let transport = websocket_transport_for(&format!("ws://{addr}/mcp"))
        .await
        .expect("connect");
    // Keep the transport alive long enough for the reader task to answer.
    let got_pong = pong_rx.await.unwrap();
    assert!(got_pong, "client did not answer server ping with pong");
    let _ = transport;
}

/// `close()` must drive the close handshake and return only after the
/// background task has exited (graceful, non-detached shutdown).
#[tokio::test]
async fn websocket_close_awaits_task() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let (sock, _) = listener.accept().await.unwrap();
        let mut ws = accept_async(sock).await.unwrap();
        // Read until the client's close (we don't need to respond).
        while let Some(Ok(_)) = ws.next().await {}
    });
    let (ws, _resp) = tokio_tungstenite::connect_async(format!("ws://{addr}/mcp"))
        .await
        .unwrap();
    let mut transport = WebSocketTransport::new(ws);
    transport.close().await.expect("close");
    // After close, recv must report the channel as closed (task exited).
    let res = transport.recv().await;
    assert!(res.is_err(), "recv after close should fail");
}
