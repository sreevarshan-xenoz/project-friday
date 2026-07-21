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
        // M10.2: record which Git action this is so we can annotate
        // Git-specific metadata after a successful call.
        let git_action = if self.server == "git" {
            Some(req.action.clone())
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
                // Adapters never publish events; they only surface metadata. The
                // runtime emits domain-specific events from these annotations.
                if let Some(path) = artifact_path {
                    out.metadata
                        .insert("artifact_paths".to_string(), json!([path]));
                }
                // M10.2: Git-specific metadata extraction. The response text
                // from the MCP server is parsed to populate structured fields
                // that the runtime emits as domain events.
                if let Some(action) = git_action {
                    let text = Self::response_text(&result);
                    match action.as_str() {
                        "git_commit" => {
                            if let Some(hash) = Self::extract_hash(&text) {
                                out.metadata.insert(
                                    "commit_hash".to_string(),
                                    json!(hash),
                                );
                            }
                        }
                        "git_branch" | "git_checkout" | "git_create_branch" => {
                            let branch = Self::extract_branch(&text, &action);
                            if !branch.is_empty() {
                                out.metadata.insert(
                                    "branch".to_string(),
                                    json!(branch),
                                );
                            }
                        }
                        "git_diff_unstaged" | "git_diff_staged" | "git_diff" => {
                            let files = Self::count_diff_files(&text);
                            out.metadata.insert(
                                "files_affected".to_string(),
                                json!(files),
                            );
                        }
                        _ => {}
                    }
                }
                out
            }
            Err(e) => ActionResult::error(format!("mcp call failed: {e}")),
        }
    }
}

/// Internal helpers for Git response parsing. These are intentionally simple
/// string operations — the MCP server returns human-readable text, and we only
/// need rough extraction for metadata.
impl McpAdapter {
    /// Concatenate all text content blocks from a tool result.
    fn response_text(result: &friday_mcp::protocol::ToolCallResult) -> String {
        result
            .content
            .iter()
            .filter_map(|c| c.get("text").and_then(|t| t.as_str()))
            .collect::<Vec<_>>()
            .join("")
    }

    /// Extract a 7+ character hex SHA from commit output. The Git MCP server
    /// returns text like "[abc1234] Initial commit" or just the hash.
    fn extract_hash(text: &str) -> Option<String> {
        // Look for bracketed hash first: "[abc1234] message"
        if let Some(start) = text.find('[') {
            if let Some(end) = text[start..].find(']') {
                let hash = &text[start + 1..start + end];
                if hash.len() >= 7 && hash.chars().all(|c| c.is_ascii_hexdigit()) {
                    return Some(hash.to_string());
                }
            }
        }
        // Fallback: find any 7+ char hex word.
        for word in text.split_whitespace() {
            let clean: String = word.chars().filter(|c| c.is_ascii_hexdigit()).collect();
            if clean.len() >= 7 {
                return Some(clean);
            }
        }
        None
    }

    /// Extract the branch name from branch-related output. The server returns
    /// text like "* master" or "Switched to branch 'main'".
    fn extract_branch(text: &str, action: &str) -> String {
        match action {
            "git_checkout" => {
                // "Switched to branch 'main'" or "Switched to a new branch 'feat'"
                if let Some(start) = text.find('\'') {
                    if let Some(end) = text[start + 1..].find('\'') {
                        return text[start + 1..start + 1 + end].to_string();
                    }
                }
                String::new()
            }
            "git_branch" => {
                // "* master" or "  feature-branch" — find the line with "*".
                for line in text.lines() {
                    let trimmed = line.trim();
                    if trimmed.starts_with('*') {
                        return trimmed.trim_start_matches('*').trim().to_string();
                    }
                }
                // No asterisk found; return first non-empty line.
                text.lines()
                    .map(|l| l.trim())
                    .find(|l| !l.is_empty())
                    .unwrap_or("")
                    .to_string()
            }
            "git_create_branch" => {
                // "Created branch 'feat'" or just the branch name.
                if let Some(start) = text.find('\'') {
                    if let Some(end) = text[start + 1..].find('\'') {
                        return text[start + 1..start + 1 + end].to_string();
                    }
                }
                text.trim().to_string()
            }
            _ => String::new(),
        }
    }

    /// Count the number of files affected in a diff by counting `diff --git`
    /// headers.
    fn count_diff_files(text: &str) -> usize {
        text.lines()
            .filter(|l| l.starts_with("diff --git"))
            .count()
    }
}
