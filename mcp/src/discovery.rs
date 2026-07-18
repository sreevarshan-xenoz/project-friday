//! MCP discovery + registry integration (PR3).
//!
//! Connects to an MCP server over any [`transport::Transport`], performs the
//! handshake, lists tools, and registers them into the planner's
//! [`CapabilityRegistry`] through the same `register` path native adapters use.
//! External tools can go offline, so discovery also drives a lightweight health
//! probe that flips capabilities to [`CapabilityHealth::Offline`] (excluded from
//! planner search) when the server becomes unreachable.

use std::sync::Arc;
use std::time::Duration;

use friday_capabilities::spec::CapabilityHealth;
use friday_planner::execution::capability_registry::CapabilityRegistry;

use crate::adapter::{capability_id, register_tools};
use crate::protocol::{McpClient, McpTool};
use crate::transport::Transport;

/// Configuration for an MCP server (used by the daemon's startup wiring and by
/// the manual integration example). `command`/`args` launch a stdio server; a
/// future WebSocket variant carries a `url` instead.
#[derive(Debug, Clone)]
pub struct McpServerConfig {
    /// Logical server name; becomes the capability source (`mcp:<name>`).
    pub name: String,
    /// How to launch/connect. For PR1/PR4 this is stdio; PR5 adds SSE.
    pub transport: McpTransportConfig,
}

/// How an MCP server is reached.
#[derive(Debug, Clone)]
pub enum McpTransportConfig {
    /// Local subprocess speaking JSON-RPC over stdio (newline-delimited).
    Stdio {
        command: String,
        args: Vec<String>,
    },
    /// Remote server reached over WebSocket (PR4). One JSON-RPC message per text
    /// frame.
    Remote { url: String },
    /// Remote server reached over SSE (PR5): a long-lived `GET {url}/sse` stream
    /// for server→client frames and `POST {url}/message` for client→server frames.
    Sse { url: String },
}

/// Connect, handshake, list tools, and register them. Returns the discovered
/// tools (for provenance/telemetry). On any protocol error, returns the error
/// without registering partial state. Registration does not manage runtime
/// health — a newly registered capability starts [`CapabilityHealth::Healthy`]
/// and health transitions are owned by the probe subsystem.
pub async fn discover_and_register<T: Transport + 'static>(
    server: &str,
    transport: T,
    registry: &CapabilityRegistry,
) -> Result<Vec<McpTool>, crate::protocol::McpError> {
    let mut client = McpClient::new(transport);
    client.initialize().await?;
    let tools = client.list_tools().await?;
    register_tools(server, &tools, registry);
    Ok(tools)
}

/// Spawn a background health probe for a registered MCP server. Periodically
/// (re)connects via `transport_factory`, re-lists tools, and if the call fails
/// marks the server's capabilities [`CapabilityHealth::Offline`] (so planner
/// discovery skips them) until the next successful probe restores
/// [`CapabilityHealth::Healthy`]. `transport_factory` is async because some
/// transports (WebSocket, future SSE) must connect before they can be used.
///
/// The probe owns a clone of the registry and runs until the returned task is
/// aborted or the runtime shuts down. Starting the probe is the caller's
/// (integration / daemon) responsibility — registration itself never starts one.
pub fn start_health_probe<F, Fut>(
    server: String,
    registry: Arc<CapabilityRegistry>,
    transport_factory: F,
    tool_ids: Vec<String>,
    interval: Duration,
) -> tokio::task::JoinHandle<()>
where
    F: Fn() -> Fut + Send + 'static,
    Fut: std::future::Future<Output = Option<Box<dyn Transport>>> + Send + 'static,
{
    tokio::spawn(async move {
        tracing::debug!(server = %server, "starting MCP health probe");
        loop {
            tokio::time::sleep(interval).await;
            let healthy = match transport_factory().await {
                Some(transport) => {
                    let mut client = McpClient::new(transport);
                    match client.initialize().await {
                        Ok(_) => client.list_tools().await.is_ok(),
                        Err(_) => false,
                    }
                }
                None => false,
            };
            let health = if healthy {
                CapabilityHealth::Healthy
            } else {
                CapabilityHealth::Offline
            };
            for id in &tool_ids {
                registry.set_health(id, health);
            }
        }
    })
}

/// Start a health probe for a server discovered via [`register_configured_servers`].
/// Convenience over [`start_health_probe`] that rebuilds the server's transport
/// on each cycle from its [`McpServerConfig`], so the integration / daemon layer
/// owns supervision without re-implementing transport selection.
///
/// Note: stdio probes relaunch the subprocess each cycle (acceptable for now; a
/// future `McpConnection` abstraction would keep one child alive). WebSocket
/// probes reconnect. This is the integration layer's responsibility, not the
/// registry's.
pub fn start_probe_for_server(
    config: &McpServerConfig,
    registry: Arc<CapabilityRegistry>,
    capability_ids: Vec<String>,
    interval: Duration,
) -> tokio::task::JoinHandle<()> {
    let config = config.clone();
    start_health_probe(
        config.name.clone(),
        registry,
        move || {
            let cfg = config.clone();
            async move { build_transport(&cfg).await }
        },
        capability_ids,
        interval,
    )
}

/// The first concrete server we integrate: the Filesystem MCP server. It
/// exercises discovery, schemas, permissions, artifacts, and context publishing
/// — every other server (Git, Notion, Slack, Figma) is a repetition of this
/// pattern.
pub fn filesystem_server_config(root: &str) -> McpServerConfig {
    McpServerConfig {
        name: "filesystem".to_string(),
        transport: McpTransportConfig::Stdio {
            command: "npx".to_string(),
            args: vec![
                "@modelcontextprotocol/server-filesystem".to_string(),
                root.to_string(),
            ],
        },
    }
}

/// Build a boxed transport for a server config. stdio spawns a subprocess;
/// `Remote` connects over WebSocket (PR4); `Sse` connects over SSE (PR5). Returns
/// `None` if launch/connect
/// fails (the caller logs and skips — a missing server must not crash boot).
async fn build_transport(config: &McpServerConfig) -> Option<Box<dyn Transport>> {
    match &config.transport {
        McpTransportConfig::Stdio { command, args } => {
            match crate::transport::stdio_transport_for(command, args) {
                Ok((transport, _child)) => Some(transport),
                Err(e) => {
                    tracing::warn!(server = %config.name, error = %e, "failed to launch MCP server; skipping");
                    None
                }
            }
        }
        McpTransportConfig::Remote { url } => {
            match crate::transport_ws::websocket_transport_for(url).await {
                Ok(transport) => Some(transport),
                Err(e) => {
                    tracing::warn!(server = %config.name, url = %url, error = %e, "failed to connect to MCP server; skipping");
                    None
                }
            }
        }
        McpTransportConfig::Sse { url } => {
            match crate::transport_sse::sse_transport_for(url).await {
                Ok(transport) => Some(transport),
                Err(e) => {
                    tracing::warn!(server = %config.name, url = %url, error = %e, "failed to connect to MCP (SSE) server; skipping");
                    None
                }
            }
        }
    }
}

/// Register a single MCP server (stdio or remote/WebSocket) into `registry`.
/// Launches/connects, handshakes, lists tools, and registers them. Failures are
/// logged and skipped (a missing `npx`/server must not crash the daemon) — the
/// server simply contributes no capabilities.
///
/// Returns the registered capability ids on success. These are intentionally
/// handed back to the caller (not used to auto-start a probe here): **registration
/// does not own runtime supervision.** The daemon/integration layer decides
/// whether and how to start a [`start_health_probe`] for the returned ids.
pub async fn register_stdio_server(
    registry: &CapabilityRegistry,
    config: &McpServerConfig,
) -> Option<Vec<String>> {
    let transport = build_transport(config).await?;
    match discover_and_register(&config.name, transport, registry).await {
        Ok(tools) => {
            let ids = tools.iter().map(|t| capability_id(&config.name, &t.name)).collect();
            tracing::info!(server = %config.name, count = tools.len(), "registered MCP capabilities");
            Some(ids)
        }
        Err(e) => {
            tracing::warn!(server = %config.name, error = %e, "MCP discovery failed; skipping");
            None
        }
    }
}

/// The result of registering one configured MCP server: its name and the
/// capability ids it contributed. Carrying the ids alongside the name keeps the
/// registration result self-describing so the caller can start a health probe or
/// record telemetry without re-deriving ids.
#[derive(Debug, Clone)]
pub struct RegisteredServer {
    pub name: String,
    pub capability_ids: Vec<String>,
}

/// Register every configured MCP server. Returns one [`RegisteredServer`] per
/// server that successfully registered (so the caller can, e.g., start health
/// probes or emit telemetry). Resilient: one bad server does not block the
/// others.
///
/// Registration intentionally does not modify runtime health — a freshly
/// registered capability starts [`CapabilityHealth::Healthy`] and health is owned
/// by the probe subsystem, not by discovery.
pub async fn register_configured_servers(
    registry: &CapabilityRegistry,
    servers: &[McpServerConfig],
) -> Vec<RegisteredServer> {
    let mut out = Vec::new();
    for cfg in servers {
        if let Some(ids) = register_stdio_server(registry, cfg).await {
            out.push(RegisteredServer {
                name: cfg.name.clone(),
                capability_ids: ids,
            });
        }
    }
    out
}
