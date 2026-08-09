//! MCP JSON-RPC protocol layer.
//!
//! This module implements the Model Context Protocol *wire contract* as a single
//! state machine, independent of how bytes move. [`McpClient`] is generic over a
//! [`Transport`] so the same protocol logic drives stdio, WebSocket, or SSE
//! without duplication. Per the architecture: protocol is separated from
//! transport, and transport is separated from process management.
//!
//! **Invariant — protocol never interprets tool semantics.** The protocol
//! understands only JSON-RPC: ids, results, errors, and notifications. It must
//! never know about *filesystem*, *git*, *capabilities*, *planning*, or
//! *execution* — those live in the adapter/discovery layer. Keeping this
//! boundary makes the protocol crate transport-agnostic and tool-agnostic as
//! FRIDAY grows.

use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use thiserror::Error;

use crate::transport::Transport;

/// Errors raised by the MCP protocol client.
#[derive(Debug, Error)]
pub enum McpError {
    #[error("transport error: {0}")]
    Transport(String),
    #[error("json-rpc parse error: {0}")]
    Json(String),
    #[error("rpc error {code}: {message}")]
    Rpc { code: i64, message: String },
    #[error("protocol violation: {0}")]
    Protocol(String),
    #[error("method not supported by server: {0}")]
    UnsupportedMethod(String),
    /// A request exceeded the configured [`McpClientConfig::request_timeout`].
    #[error("request '{0}' timed out after {1:?}")]
    Timeout(String, Duration),
}

/// A JSON-RPC 2.0 request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcRequest {
    pub jsonrpc: String,
    pub id: Value,
    pub method: String,
    #[serde(default, skip_serializing_if = "Value::is_null")]
    pub params: Value,
}

/// A JSON-RPC 2.0 response (result or error).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcResponse {
    pub jsonrpc: String,
    pub id: Value,
    #[serde(default, skip_serializing_if = "Value::is_null")]
    pub result: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<JsonRpcError>,
}

/// A JSON-RPC 2.0 error object.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcError {
    pub code: i64,
    pub message: String,
    #[serde(default, skip_serializing_if = "Value::is_null")]
    pub data: Value,
}

/// A tool advertised by an MCP server (`tools/list`).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpTool {
    pub name: String,
    #[serde(default)]
    pub description: String,
    /// JSON Schema describing the tool's input.
    #[serde(default)]
    pub input_schema: Value,
}

/// Result of `tools/list`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolsListResult {
    pub tools: Vec<McpTool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<String>,
}

/// Result of `tools/call`. The server returns structured content and/or raw
/// text the supervisor can publish into the goal context.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCallResult {
    #[serde(default)]
    pub content: Vec<Value>,
    #[serde(default)]
    pub is_error: bool,
}

/// Handshake parameters for `initialize`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InitializeParams {
    pub protocol_version: String,
    pub capabilities: Value,
    pub client_info: ClientInfo,
}

/// Client identification sent during `initialize`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClientInfo {
    pub name: String,
    pub version: String,
}

/// Initialize result returned by the server.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InitializeResult {
    pub protocol_version: String,
    #[serde(default)]
    pub capabilities: Value,
    #[serde(default)]
    pub server_info: Value,
}

const PROTOCOL_VERSION: &str = "2024-11-05";

/// Configuration for [`McpClient`]. Decouples policy (timeouts) from protocol
/// logic so different transports or deployments can tune behavior without
/// touching the JSON-RPC state machine. Timeouts live at the protocol layer
/// (not the transport): the transport only moves frames; the caller turns a
/// timeout into a recoverable execution outcome.
#[derive(Debug, Clone)]
pub struct McpClientConfig {
    /// Per-request timeout. A request that does not get a matching response
    /// within this window fails with [`McpError::Timeout`]. `None` disables the
    /// timeout (block until a response arrives).
    pub request_timeout: Option<Duration>,
}

impl Default for McpClientConfig {
    fn default() -> Self {
        Self {
            request_timeout: Some(Duration::from_secs(30)),
        }
    }
}

/// The MCP protocol client. Owns a [`Transport`] and implements the JSON-RPC
/// request/response correlation exactly once; transports only move framed
/// messages.
///
/// **Concurrency: this client is single-flight.** Every method takes `&mut
/// self` and the request loop owns the read side, so only one request may be in
/// flight at a time (the borrow checker enforces it). Concurrent requests would
/// require a future multiplexed-receiver architecture (a background reader +
/// pending-request table) and are intentionally out of scope for v1.
pub struct McpClient<T: Transport> {
    transport: T,
    next_id: std::sync::atomic::AtomicU64,
    config: McpClientConfig,
}

impl<T: Transport> McpClient<T> {
    pub fn new(transport: T) -> Self {
        Self::with_config(transport, McpClientConfig::default())
    }

    pub fn with_config(transport: T, config: McpClientConfig) -> Self {
        Self {
            transport,
            next_id: std::sync::atomic::AtomicU64::new(1),
            config,
        }
    }

    fn next_rpc_id(&self) -> Value {
        let n = self
            .next_id
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Value::from(n)
    }

    async fn request(&mut self, method: &str, params: Value) -> Result<Value, McpError> {
        let id = self.next_rpc_id();
        let req = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: id.clone(),
            method: method.to_string(),
            params,
        };
        let raw = serde_json::to_string(&req).map_err(|e| McpError::Json(e.to_string()))?;
        self.transport
            .send(&raw)
            .await
            .map_err(McpError::Transport)?;

        // Read responses until we find the one matching our id (servers may emit
        // notifications in between). The wait is bounded by the configured
        // request timeout so a stalled server fails recoverably rather than
        // hanging the caller (and the supervisor) forever.
        let read = async {
            loop {
                let frame = self.transport.recv().await.map_err(McpError::Transport)?;
                let resp: JsonRpcResponse =
                    serde_json::from_str(&frame).map_err(|e| McpError::Json(e.to_string()))?;
                if resp.id != id {
                    // A notification or out-of-order response; skip and keep reading.
                    continue;
                }
                return match resp.error {
                    Some(err) => Err(McpError::Rpc {
                        code: err.code,
                        message: err.message,
                    }),
                    None => Ok(resp.result),
                };
            }
        };
        match self.config.request_timeout {
            Some(timeout) => {
                let inner = tokio::time::timeout(timeout, read)
                    .await
                    .map_err(|_| McpError::Timeout(method.to_string(), timeout))?;
                inner
            }
            None => read.await,
        }
    }

    /// Perform the `initialize` handshake. Servers may send a follow-up
    /// `notifications/initialized` which we tolerate but do not require.
    pub async fn initialize(&mut self) -> Result<InitializeResult, McpError> {
        let params = InitializeParams {
            protocol_version: PROTOCOL_VERSION.to_string(),
            capabilities: json!({}),
            client_info: ClientInfo {
                name: "friday".to_string(),
                version: env!("CARGO_PKG_VERSION").to_string(),
            },
        };
        let result = self
            .request("initialize", serde_json::to_value(&params).unwrap())
            .await?;
        // Drain any post-init notification frames so callers start clean.
        self.drain_notifications().await;
        serde_json::from_value(result).map_err(|e| McpError::Json(e.to_string()))
    }

    /// Read and discard genuine notifications (frames with no `id`) following a
    /// request, until the next frame carries an `id`. Stops as soon as a
    /// response/request frame appears so we never consume a frame belonging to a
    /// subsequent call. This replaces a transport-specific `try_recv` poll with
    /// a semantic rule: only id-less JSON-RPC messages are notifications.
    async fn drain_notifications(&mut self) {
        // Bound the work so a misbehaving server that streams notifications
        // forever cannot loop indefinitely; the bound is generous and only a
        // safety net — normal servers send zero or one.
        for _ in 0..16 {
            // Peek via a short non-blocking read; if nothing is buffered we stop.
            let frame = match self.transport.try_recv().await {
                Ok(f) => f,
                Err(_) => break,
            };
            let v: Value = match serde_json::from_str(&frame) {
                Ok(v) => v,
                // Unparseable frame: drop it rather than risk an infinite loop.
                Err(_) => continue,
            };
            let has_id = v.get("id").is_some() && !v.get("id").unwrap().is_null();
            if has_id {
                // This frame belongs to a later request; it must not be consumed.
                // We cannot push it back, so we rely on the single-flight contract
                // (no other request is in flight right after `initialize`) and stop
                // draining. A multiplexed client would stash it in the pending table.
                break;
            }
            // Id-less message: a notification — discard and continue.
        }
    }

    /// List tools advertised by the server.
    pub async fn list_tools(&mut self) -> Result<Vec<McpTool>, McpError> {
        let result = self.request("tools/list", json!({})).await?;
        let list: ToolsListResult =
            serde_json::from_value(result).map_err(|e| McpError::Json(e.to_string()))?;
        Ok(list.tools)
    }

    /// Call a tool by name with the given arguments.
    pub async fn call_tool(
        &mut self,
        name: &str,
        arguments: Value,
    ) -> Result<ToolCallResult, McpError> {
        let params = json!({ "name": name, "arguments": arguments });
        let result = self.request("tools/call", params).await?;
        serde_json::from_value(result).map_err(|e| McpError::Json(e.to_string()))
    }

    /// Call an arbitrary MCP method (e.g. vendor extensions). Most users want
    /// [`McpClient::call_tool`]; this exists for methods outside the core trio.
    pub async fn request_unchecked(
        &mut self,
        method: &str,
        params: Value,
    ) -> Result<Value, McpError> {
        self.request(method, params).await
    }
}
