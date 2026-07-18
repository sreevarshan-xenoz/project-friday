//! Daemon integration hook for MCP (final wiring step).
//!
//! One call from the runtime entry point connects the planner stack and then
//! registers any configured MCP servers into the *same* capability registry the
//! planner/supervisor resolve against. MCP servers are opt-in (an empty config
//! list is a no-op), and a server that fails to launch is skipped rather than
//! crashing boot — so a host without `npx` still runs with native capabilities
//! only.

use std::sync::Arc;
use std::time::Duration;

use friday_core::Friday;
use friday_planner::execution::capability_registry::CapabilityRegistry;
use friday_planner::register_engines_with_llm;

use crate::discovery::{
    register_configured_servers, start_probe_for_server, RegisteredServer, McpServerConfig,
};

/// Wire the full planner stack (optionally with an LLM backend) and then
/// register any configured MCP servers into the shared registry. Returns the
/// registry together with the per-server [`RegisteredServer`] results so the
/// caller (the daemon) can start health probes or record telemetry. Registration
/// deliberately does not start probes — supervision is the integration layer's
/// job, not discovery's.
pub async fn register_with_mcp(
    friday: &mut Friday,
    llm_backend: Option<Box<dyn friday_planner::planning::planners::backend::ReasoningBackend>>,
    servers: &[McpServerConfig],
) -> anyhow::Result<(Arc<CapabilityRegistry>, Vec<RegisteredServer>)> {
    let registry = register_engines_with_llm(friday, llm_backend)
        .await
        .map_err(|e| anyhow::anyhow!("planner registration failed: {e}"))?;
    let registered = register_configured_servers(&registry, servers).await;
    Ok((registry, registered))
}

/// Like [`register_with_mcp`] but also starts a health probe per registered
/// server, returning the probes' join handles (so the daemon can abort them on
/// shutdown). This is where runtime supervision is wired — discovery stays
/// free of background-task ownership.
pub async fn register_with_mcp_and_probes(
    friday: &mut Friday,
    llm_backend: Option<Box<dyn friday_planner::planning::planners::backend::ReasoningBackend>>,
    servers: &[McpServerConfig],
    probe_interval: Duration,
) -> anyhow::Result<(
    Arc<CapabilityRegistry>,
    Vec<RegisteredServer>,
    Vec<tokio::task::JoinHandle<()>>,
)> {
    let (registry, registered) = register_with_mcp(friday, llm_backend, servers).await?;
    let mut probes = Vec::new();
    for reg in &registered {
        // Match the config to start its probe (config carries transport details).
        if let Some(cfg) = servers.iter().find(|c| c.name == reg.name) {
            let handle = start_probe_for_server(
                cfg,
                Arc::clone(&registry),
                reg.capability_ids.clone(),
                probe_interval,
            );
            probes.push(handle);
        }
    }
    Ok((registry, registered, probes))
}
