//! MCP → capability adapter (PR2).
//!
//! Translates an MCP [`protocol::McpTool`] into the workspace-wide
//! [`friday_capabilities::spec::CapabilitySpec`] resolution contract. The
//! planner, supervisor, and action runtime never learn that a capability came
//! from an MCP server — they see the same `CapabilitySpec` a native adapter
//! produces. Provenance is carried in [`CapabilitySource::Mcp`], so trust
//! policies ("only native", "prefer MCP") and debugging work without special
//! cases.

use friday_capabilities::spec::{CapabilitySource, CapabilitySpec, ExecutionBinding, Risk};
use friday_planner::execution::capability_registry::CapabilityRegistry;

use crate::protocol::McpTool;

/// Build the canonical capability id for an MCP tool: `mcp:<server>.<tool>`.
/// The `mcp:` prefix namespaces it away from native ids and matches the
/// [`CapabilitySource::Mcp`] tag.
pub fn capability_id(server: &str, tool: &str) -> String {
    format!("mcp:{server}.{tool}")
}

/// Default routing adapter key for an MCP server: `mcp:<server>`. The action
/// runtime dispatches any tool call for this server to the MCP adapter.
pub fn adapter_key(server: &str) -> String {
    format!("mcp:{server}")
}

/// Infer a sensible default [`Risk`] for an MCP tool from its name. External
/// tools that only read are `Safe`; anything that may mutate is `Medium`
/// (the supervisor can still gate `High` explicitly if a server tags it). This
/// is a conservative default and can be overridden by richer server metadata
/// later.
fn infer_risk(tool: &str) -> Risk {
    let t = tool.to_lowercase();
    if t.starts_with("read")
        || t.starts_with("list")
        || t.starts_with("get")
        || t.starts_with("search")
        || t.starts_with("stat")
    {
        Risk::Safe
    } else {
        Risk::Medium
    }
}

/// Convert one MCP tool into a [`CapabilitySpec`] bound to `server`.
pub fn tool_to_spec(server: &str, tool: &McpTool) -> CapabilitySpec {
    CapabilitySpec {
        id: capability_id(server, &tool.name),
        name: tool.name.clone(),
        description: tool.description.clone(),
        source: CapabilitySource::Mcp(server.to_string()),
        binding: ExecutionBinding {
            adapter: adapter_key(server),
            action: tool.name.clone(),
            schema: tool.input_schema.clone(),
            permissions: Vec::new(),
            risk: infer_risk(&tool.name),
        },
    }
}

/// Register every tool of an MCP server into the planner's capability registry.
/// Discovery (PR3) calls this after `tools/list`; the same `register` path is
/// used by native adapters, so the supervisor resolves MCP capabilities
/// identically.
pub fn register_tools(server: &str, tools: &[McpTool], registry: &CapabilityRegistry) {
    for tool in tools {
        registry.register(tool_to_spec(server, tool));
    }
}

/// Convenience: convert + register in one step, returning the registered specs
/// (useful for tests and for building provenance metadata).
pub fn register_server_capabilities(
    server: &str,
    tools: &[McpTool],
    registry: &CapabilityRegistry,
) -> Vec<CapabilitySpec> {
    let specs: Vec<CapabilitySpec> = tools.iter().map(|t| tool_to_spec(server, t)).collect();
    for s in &specs {
        registry.register(s.clone());
    }
    specs
}
