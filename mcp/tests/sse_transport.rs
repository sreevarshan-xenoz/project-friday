//! Hermetic SSE transport tests (PR5).
//!
//! A tiny in-process HTTP server speaks the classic MCP SSE contract over a raw
//! `TcpListener` (no Node / no extra deps): `GET /sse` returns a chunked
//! `text/event-stream` and `POST /message` triggers a correlated response frame
//! written onto that stream. This exercises the real [`transport_sse`] framing
//! without a live MCP server.

use std::sync::Arc;

use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpListener;
use tokio::sync::broadcast;

use friday_mcp::protocol::McpClient;
use friday_mcp::transport_sse::sse_transport_for;

/// A minimal HTTP server implementing classic MCP SSE: one SSE stream plus a
/// message POST endpoint. Inbound POST bodies are echoed as correlated JSON-RPC
/// responses onto the open SSE stream.
async fn spawn_sse_server() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    // Broadcast carries SSE frames to whichever connection opened the stream.
    let (sse_tx, _rx) = broadcast::channel::<String>(16);
    let sse_tx = Arc::new(sse_tx);

    tokio::spawn(async move {
        loop {
            let (sock, _) = listener.accept().await.unwrap();
            let sse_tx = sse_tx.clone();
            tokio::spawn(handle_conn(sock, sse_tx));
        }
    });

    format!("http://{addr}")
}

async fn handle_conn(sock: tokio::net::TcpStream, sse_tx: Arc<broadcast::Sender<String>>) {
    let (read_half, mut write_half) = sock.into_split();
    let mut reader = BufReader::new(read_half);
    let mut request_line = String::new();
    if reader.read_line(&mut request_line).await.unwrap_or(0) == 0 {
        return;
    }
    let method = request_line.split_whitespace().next().unwrap_or("");
    // Drain headers.
    let mut content_len = 0usize;
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line).await.unwrap_or(0) == 0 {
            break;
        }
        if line.trim().is_empty() {
            break;
        }
        if let Some(rest) = line.to_ascii_lowercase().strip_prefix("content-length:") {
            content_len = rest.trim().parse().unwrap_or(0);
        }
    }

    if method == "GET" {
        // Open the SSE stream: chunked text/event-stream.
        let headers = "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nCache-Control: no-cache\r\nTransfer-Encoding: chunked\r\nConnection: keep-alive\r\n\r\n";
        if write_half.write_all(headers.as_bytes()).await.is_err() {
            return;
        }
        let mut sse_rx = sse_tx.subscribe();
        while let Ok(frame) = sse_rx.recv().await {
            // Chunked encoding: <hex len>\r\n<data>\r\n
            let chunk = format!("data: {frame}\n\n");
            if write_chunk(&mut write_half, chunk.as_bytes())
                .await
                .is_err()
            {
                break;
            }
        }
    } else if method == "POST" {
        // Read body, parse JSON-RPC id, emit a correlated response over SSE.
        let mut body = vec![0u8; content_len];
        if reader.read_exact(&mut body).await.is_err() {
            return;
        }
        let v: serde_json::Value = match serde_json::from_slice(&body) {
            Ok(v) => v,
            Err(_) => return,
        };
        let rid = v.get("id").cloned().unwrap_or(serde_json::Value::Null);
        let method = v.get("method").and_then(|x| x.as_str()).unwrap_or("");
        let response = match method {
            "initialize" => serde_json::json!({
                "jsonrpc": "2.0", "id": rid,
                "result": { "protocolVersion": "2024-11-05", "capabilities": {}, "serverInfo": {} }
            }),
            "tools/list" => {
                serde_json::json!({ "jsonrpc": "2.0", "id": rid, "result": { "tools": [] } })
            }
            "tools/call" => {
                serde_json::json!({ "jsonrpc": "2.0", "id": rid, "result": { "content": [ { "type": "text", "text": "pong" } ] } })
            }
            _ => {
                serde_json::json!({ "jsonrpc": "2.0", "id": rid, "error": { "code": -32601, "message": "x" } })
            }
        };
        let _ = sse_tx.send(serde_json::to_string(&response).unwrap());
        // 202 Accepted: response delivered via SSE stream, not the POST body.
        let _ = write_half
            .write_all(b"HTTP/1.1 202 Accepted\r\nContent-Length: 0\r\n\r\n")
            .await;
    }
}

async fn write_chunk(w: &mut (impl AsyncWriteExt + Unpin), data: &[u8]) -> std::io::Result<()> {
    let header = format!("{:x}\r\n", data.len());
    w.write_all(header.as_bytes()).await?;
    w.write_all(data).await?;
    w.write_all(b"\r\n").await?;
    w.flush().await
}

#[tokio::test]
async fn sse_handshake_and_roundtrip() {
    // GET /sse opens a stream; a POST /message triggers a correlated response
    // delivered back over SSE. Full initialize + tools/list roundtrip.
    let base = spawn_sse_server().await;
    let transport = sse_transport_for(&base).await.expect("connect sse");
    let mut client = McpClient::new(transport);
    let info = client.initialize().await.expect("initialize over SSE");
    assert_eq!(info.protocol_version, "2024-11-05");

    let tools = client.list_tools().await.expect("list over SSE");
    assert!(tools.is_empty());
}

#[tokio::test]
async fn sse_tools_call_roundtrip() {
    // tools/call over SSE returns content delivered via the stream.
    let base = spawn_sse_server().await;
    let transport = sse_transport_for(&base).await.expect("connect sse");
    let mut client = McpClient::new(transport);
    client.initialize().await.unwrap();

    let result = client
        .call_tool("ping", serde_json::json!({}))
        .await
        .expect("call_tool over SSE");
    let text = result
        .content
        .first()
        .and_then(|c| c.get("text"))
        .and_then(|t| t.as_str())
        .unwrap_or("");
    assert_eq!(text, "pong");
}

#[tokio::test]
async fn sse_connection_refused_is_error() {
    // A base URL with nothing listening must surface a connect error, not hang.
    let result = sse_transport_for("http://127.0.0.1:1").await;
    assert!(result.is_err(), "refused SSE connection should error");
}
