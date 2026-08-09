//! Hermetic in-process MCP server + transport for tests.
//!
//! These let `cargo test` exercise the full JSON-RPC protocol and adapter logic
//! with zero external processes and no Node — CI never needs `npx`. The fake
//! server speaks real MCP JSON-RPC over an in-memory channel pair, so it is a
//! faithful stand-in for a subprocess server.

use async_trait::async_trait;
use serde_json::{json, Value};
use tokio::sync::mpsc;

use crate::protocol::{JsonRpcError, JsonRpcRequest, JsonRpcResponse, McpTool};
use crate::transport::Transport;

/// Canned `tools/call` response generator used by the hermetic fake server.
type CallHandler = Box<dyn Fn(&str, &Value) -> Value + Send + Sync>;

/// A transport backed by two in-memory channels: one for outbound frames
/// (client→server), one for inbound (server→client). Implements [`Transport`]
/// so [`crate::protocol::McpClient`] is none the wiser that no bytes hit a pipe.
pub struct FakeTransport {
    outbound: mpsc::Sender<String>,
    inbound: mpsc::Receiver<String>,
}

impl FakeTransport {
    /// Create a connected pair: the client gets `client_side`, the server gets
    /// `server_side`.
    pub fn pair() -> (FakeTransport, FakeTransport) {
        let (c_tx, s_rx) = mpsc::channel::<String>(64);
        let (s_tx, c_rx) = mpsc::channel::<String>(64);
        let client_side = FakeTransport {
            outbound: c_tx,
            inbound: c_rx,
        };
        let server_side = FakeTransport {
            outbound: s_tx,
            inbound: s_rx,
        };
        (client_side, server_side)
    }
}

#[async_trait]
impl Transport for FakeTransport {
    async fn send(&mut self, frame: &str) -> Result<(), String> {
        self.outbound
            .send(frame.to_string())
            .await
            .map_err(|e| e.to_string())
    }

    async fn recv(&mut self) -> Result<String, String> {
        self.inbound
            .recv()
            .await
            .ok_or_else(|| "channel closed".to_string())
    }

    async fn try_recv(&mut self) -> Result<String, String> {
        match self.inbound.try_recv() {
            Ok(f) => Ok(f),
            Err(_) => Err("no frame available".to_string()),
        }
    }
}

/// A minimal but protocol-correct MCP server for tests. It responds to
/// `initialize`, `tools/list`, and `tools/call` with canned data, and can be
/// configured with custom tools and call behavior.
pub struct FakeMcpServer {
    tools: Vec<McpTool>,
    /// Optional canned response generator for `tools/call`.
    on_call: Option<CallHandler>,
    /// Optional notification frame to emit right after the `initialize` response
    /// (e.g. `notifications/initialized`), used to exercise notification draining.
    notification_after_initialize: Option<Value>,
}

impl Default for FakeMcpServer {
    fn default() -> Self {
        Self::new()
    }
}

impl FakeMcpServer {
    pub fn new() -> Self {
        Self {
            tools: vec![
                McpTool {
                    name: "read_file".to_string(),
                    description: "Read a file from disk".to_string(),
                    input_schema: json!({
                        "type": "object",
                        "properties": { "path": { "type": "string" } },
                        "required": ["path"]
                    }),
                },
                McpTool {
                    name: "write_file".to_string(),
                    description: "Write a file to disk".to_string(),
                    input_schema: json!({
                        "type": "object",
                        "properties": { "path": { "type": "string" }, "content": { "type": "string" } },
                        "required": ["path", "content"]
                    }),
                },
            ],
            on_call: None,
            notification_after_initialize: None,
        }
    }

    /// Override the tools the server advertises.
    pub fn with_tools(mut self, tools: Vec<McpTool>) -> Self {
        self.tools = tools;
        self
    }

    /// Snapshot of the tools the server will advertise (for tests).
    pub fn tools(&self) -> Vec<McpTool> {
        self.tools.clone()
    }

    /// Provide custom `tools/call` behavior (e.g. to simulate failures).
    pub fn with_call_handler<F>(mut self, f: F) -> Self
    where
        F: Fn(&str, &Value) -> Value + Send + Sync + 'static,
    {
        self.on_call = Some(Box::new(f));
        self
    }

    /// Emit `notification` (a JSON-RPC notification frame, i.e. no `id`) immed-
    /// iately after responding to `initialize`. Used to test client-side
    /// notification draining.
    pub fn with_notification_after_initialize(mut self, notification: Value) -> Self {
        self.notification_after_initialize = Some(notification);
        self
    }

    fn respond(&self, req: &JsonRpcRequest) -> JsonRpcResponse {
        let id = req.id.clone();
        let result = match req.method.as_str() {
            "initialize" => json!({
                "protocolVersion": "2024-11-05",
                "capabilities": { "tools": {} },
                "server_info": { "name": "fake-mcp", "version": "0.0.0" }
            }),
            "tools/list" => json!({ "tools": self.tools }),
            "tools/call" => {
                let name = req
                    .params
                    .get("name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let args = req.params.get("arguments").cloned().unwrap_or(Value::Null);
                // Unknown tool -> MCP-style error result (is_error: true).
                if !self.tools.iter().any(|t| t.name == name) {
                    return JsonRpcResponse {
                        jsonrpc: "2.0".to_string(),
                        id,
                        result: json!({
                            "content": [ { "type": "text", "text": format!("unknown tool: {name}") } ],
                            "is_error": true
                        }),
                        error: None,
                    };
                }
                match &self.on_call {
                    Some(f) => (f)(name, &args),
                    None => json!({
                        "content": [ { "type": "text", "text": format!("called {name}") } ],
                        "is_error": false
                    }),
                }
            }
            other => {
                return JsonRpcResponse {
                    jsonrpc: "2.0".to_string(),
                    id,
                    result: Value::Null,
                    error: Some(JsonRpcError {
                        code: -32601,
                        message: format!("method not found: {other}"),
                        data: Value::Null,
                    }),
                };
            }
        };
        JsonRpcResponse {
            jsonrpc: "2.0".to_string(),
            id,
            result,
            error: None,
        }
    }

    /// Run the server loop on `transport` until the client disconnects. Returns
    /// when the inbound channel closes.
    pub async fn serve(self, mut transport: FakeTransport) {
        while let Ok(frame) = transport.recv().await {
            let req: JsonRpcRequest = match serde_json::from_str(&frame) {
                Ok(r) => r,
                Err(_) => continue,
            };
            let resp = self.respond(&req);
            let raw = serde_json::to_string(&resp).unwrap();
            if transport.send(&raw).await.is_err() {
                break;
            }
            // After `initialize`, push any configured notification so the client
            // must drain id-less frames before the next request.
            if req.method == "initialize" {
                if let Some(note) = &self.notification_after_initialize {
                    let note_raw = serde_json::to_string(note).unwrap();
                    if transport.send(&note_raw).await.is_err() {
                        break;
                    }
                }
            }
        }
    }
}
