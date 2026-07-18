//! MCP capability adapter (M8: Execution Runtime Unification).
//!
//! This is the change that makes MCP *disappear* from the runtime architecture.
//! An MCP server is registered with the [`crate::runtime::AgentKernel`] under the
//! tool key `mcp:<server>` (exactly the capability id the planner resolves),
//! exactly like a native adapter. When the supervisor emits `exec.action` for a
//! `mcp:<server>.<tool>` capability, the kernel's existing `split_capability`
//! yields `("mcp:<server>", "<tool>")`, looks up this adapter, and calls
//! [`ActionAdapter::execute`]. **No code anywhere branches on
//! `CapabilitySource::Mcp`** — MCP is just another executor behind the unified
//! [`crate::adapters::ActionAdapter`] interface.
//!
//! Because dispatch, permission, timeout, cancellation, retry, audit and
//! telemetry all live in `AgentKernel::run_action`, the MCP backend inherits
//! identical execution semantics to native/plugin backends (M8.2/M8.3/M8.5) and
//! emits the same normalized [`ActionResult`] (M8.4). The adapter only translates
//! between the normalized request/result and the MCP `tools/call` wire call.

use async_trait::async_trait;
use serde_json::json;
use tokio::sync::Mutex;

use friday_mcp::protocol::McpClient;
use friday_mcp::transport::Transport;

use crate::adapters::ActionAdapter;
use crate::request::{ActionRequest, ActionResult};

/// An [`ActionAdapter`] backed by a live MCP server.
///
/// Holds a single shared [`McpClient`] behind a `Mutex` because the MCP wire
/// protocol is single-flight (one in-flight request at a time; see
/// `friday_mcp::protocol`). The kernel serializes calls per adapter, so multiple
/// tasks touching the same server are naturally fanned into one connection.
pub struct McpAdapter {
    /// Logical server name; the tool key is `mcp:<server>`.
    server: String,
    /// Fully-qualified tool key (`mcp:<server>`), kept as a stable `&str` source.
    tool_key: String,
    /// Shared, locked MCP client (single-flight on the wire).
    client: tokio::sync::Mutex<McpClient<Box<dyn Transport>>>,
    /// Capabilities this server is known to expose (for `actions()`/discovery).
    actions: Vec<String>,
}

impl McpAdapter {
    /// Wrap a connected, initialized MCP client as an adapter. `server` becomes
    /// the tool key `mcp:<server>`; `actions` advertises the tool names (used for
    /// logging/discovery, not routing — routing uses `req.action` directly).
    pub fn new(
        server: impl Into<String>,
        client: McpClient<Box<dyn Transport>>,
        actions: Vec<String>,
    ) -> Self {
        let server = server.into();
        let tool_key = format!("mcp:{server}");
        Self {
            server,
            tool_key,
            client: Mutex::new(client),
            actions,
        }
    }

    /// The tool key used to register this adapter with the kernel.
    pub fn tool_key_for(server: &str) -> String {
        format!("mcp:{server}")
    }
}

#[async_trait]
impl ActionAdapter for McpAdapter {
    fn tool(&self) -> &str {
        // Capability id == tool key: `mcp:<server>`.
        &self.tool_key
    }

    fn actions(&self) -> Vec<&str> {
        self.actions.iter().map(|s| s.as_str()).collect()
    }

    async fn execute(&self, req: ActionRequest) -> ActionResult {
        // `req.action` is the MCP tool name (e.g. `read_file`); `req.parameters`
        // are the tool arguments. The normalized request/result contract hides
        // all MCP specifics from the runtime.
        let mut client = self.client.lock().await;
        // M9.3: capture the artifact path up-front (before `req` is moved into
        // `call_tool`) so a successful write/create can be annotated.
        let artifact_path = if req.action == "write_file" || req.action == "create_directory" {
            req.parameters
                .get("path")
                .and_then(|p| p.as_str())
                .map(|s| s.to_string())
        } else {
            None
        };
        match client.call_tool(&req.action, req.parameters).await {
            Ok(result) => {
                if result.is_error {
                    return ActionResult::error(
                        result
                            .content
                            .first()
                            .and_then(|c| c.get("text"))
                            .and_then(|t| t.as_str())
                            .unwrap_or("mcp tool returned error")
                            .to_string(),
                    );
                }
                let mut out = ActionResult::success(json!({
                    "content": result.content,
                    "source": "mcp",
                    "server": self.server,
                }));
                // Adapters never publish events; they only surface the path. The
                // runtime emits `artifact.created` from this annotation.
                if let Some(path) = artifact_path {
                    out.metadata
                        .insert("artifact_paths".to_string(), json!([path]));
                }
                out
            }
            Err(e) => ActionResult::error(format!("mcp call failed: {e}")),
        }
    }
}
