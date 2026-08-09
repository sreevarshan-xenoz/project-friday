//! Native capability registration — the bridge between the existing grouped
//! [`friday_capabilities::model::Capability`] definitions (one per adapter) and
//! the M6 [`CapabilityRegistry`], which resolves a *canonical per-action id*
//! (`filesystem.write_file`) to an [`ExecutionBinding`].
//!
//! Native adapters advertise a single grouped capability whose `id` is the
//! adapter name (`filesystem`, `git`, …) and whose `actions` are the verbs. We
//! expand each into one [`CapabilitySpec`] per action so the supervisor can
//! gate and dispatch at action granularity — which is exactly what it needs for
//! risk gating (`git.push` is High, `git.status` is Safe).
//!
//! Risk is assigned per action here (see the architecture note in `docs/`); it
//! is metadata on the capability, never matched by string in the engine.

use crate::execution::capability_registry::{CapabilityRegistry, CapabilitySpec, Risk};

/// Risk classification for a native action, keyed by `{adapter}.{action}`.
///
/// Defaults to [`Risk::Safe`]; only actions that mutate state or reach outside
/// the sandbox are escalated. This table is the single source of truth for
/// native risk today — MCP/plugin capabilities carry their own risk at
/// registration time.
fn native_risk(adapter: &str, action: &str) -> Risk {
    match (adapter, action) {
        // Filesystem: reads are safe; writes/moves are medium; deletes are high.
        ("filesystem", "read_file") | ("filesystem", "list_directory") => Risk::Safe,
        ("filesystem", "create_file")
        | ("filesystem", "write_file")
        | ("filesystem", "rename")
        | ("filesystem", "copy")
        | ("filesystem", "move") => Risk::Medium,
        ("filesystem", "delete_file") => Risk::High,

        // Git: read/inspect are safe; local mutations medium; network push is high.
        ("git", "status") | ("git", "diff") | ("git", "log") => Risk::Safe,
        ("git", "clone")
        | ("git", "init")
        | ("git", "add")
        | ("git", "checkout")
        | ("git", "branch")
        | ("git", "merge")
        | ("git", "pull")
        | ("git", "commit") => Risk::Medium,
        ("git", "push") => Risk::High,

        // Terminal: inspection safe; running arbitrary commands is high by
        // default (it can do anything). Refinements (e.g. `rm -rf` detection)
        // belong in the policy layer, not here.
        ("terminal", "list_sessions") | ("terminal", "kill") => Risk::Medium,
        ("terminal", "run") | ("terminal", "exec") => Risk::High,

        // Web: read-only HTTP reads (search + fetch/extract). Safe.
        ("web", "search") | ("web", "fetch") => Risk::Safe,

        // Docs / code intelligence: read-only local inspection. Safe.
        ("docs", "search") | ("docs", "read") => Risk::Safe,
        ("codeintel", "understand") => Risk::Safe,

        _ => Risk::Safe,
    }
}

/// Convert one grouped native [`friday_capabilities::model::Capability`] into
/// per-action [`CapabilitySpec`]s for the M6 registry.
pub fn from_native_capability(cap: &friday_capabilities::model::Capability) -> Vec<CapabilitySpec> {
    cap.actions
        .iter()
        .map(|action| {
            let id = format!("{}.{}", cap.id, action);
            CapabilitySpec::native(
                id.clone(),
                format!("{}: {}", cap.name, action),
                format!("{} — action `{}`", cap.description, action),
                cap.id.clone(), // adapter routing key
                action.clone(), // verb within the adapter
                native_risk(&cap.id, action),
            )
        })
        .collect()
}

/// Register every native adapter's capabilities into the registry, expanding
/// each adapter into per-action bindings with risk classification.
///
/// This is the M6.1 built-in registration: run it once at startup before any
/// goal is dispatched. MCP/plugin capabilities register through the same
/// [`CapabilityRegistry::register`] later — the supervisor never changes.
pub fn register_native_builtins(registry: &CapabilityRegistry) {
    for cap in friday_capabilities::builtins::all() {
        registry.register_all(from_native_capability(&cap));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_builtins_register_per_action_ids() {
        let reg = CapabilityRegistry::new();
        register_native_builtins(&reg);
        let known = reg.known();
        // 8 fs + 4 terminal + 11 git + 2 web + 2 docs + 1 codeintel = 28
        // actions from the canonical builtins.
        assert_eq!(known.len(), 28, "unexpected native capability count");
        assert!(known.contains(&"filesystem.write_file".to_string()));
        assert!(known.contains(&"git.push".to_string()));
        assert!(known.contains(&"terminal.exec".to_string()));
        assert!(known.contains(&"web.search".to_string()));
        assert!(known.contains(&"docs.read".to_string()));
        assert!(known.contains(&"codeintel.understand".to_string()));
    }

    #[test]
    fn risk_classification_is_correct() {
        let reg = CapabilityRegistry::new();
        register_native_builtins(&reg);
        // High-risk: destructive / external.
        assert_eq!(reg.risk_of("filesystem.delete_file"), Risk::High);
        assert_eq!(reg.risk_of("git.push"), Risk::High);
        assert_eq!(reg.risk_of("terminal.exec"), Risk::High);
        // Medium: local mutations.
        assert_eq!(reg.risk_of("filesystem.write_file"), Risk::Medium);
        assert_eq!(reg.risk_of("git.commit"), Risk::Medium);
        // Safe: read-only.
        assert_eq!(reg.risk_of("filesystem.read_file"), Risk::Safe);
        assert_eq!(reg.risk_of("git.status"), Risk::Safe);
    }

    #[test]
    fn binding_routes_to_adapter_and_verb() {
        let reg = CapabilityRegistry::new();
        register_native_builtins(&reg);
        let b = reg.resolve("git.push").expect("git.push registered");
        assert_eq!(b.adapter, "git");
        assert_eq!(b.action, "push");
    }
}
