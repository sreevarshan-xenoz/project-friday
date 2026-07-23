# services/

Long-lived, addressable capability **services** hosted by the FRIDAY platform.
A service exposes a stable identity, lifecycle, and capabilities; the planner and
kernel reason over those capabilities rather than hardcoding plugin names.

The low-level `Service` trait remains in `core/src/services/traits.rs`. Concrete
crates live here.

## Workspace crates

| Crate | Path | Role | Status |
| ----- | ---- | ---- | ------ |
| `friday-actions` | `action-runtime/` | **AgentKernel** — permissions, audit, lifecycle, sessions, resources, metrics, `ActionAdapter` dispatch | ✅ Stable |
| `friday-capabilities` | `capabilities/` | Capability registry, ranking, health, graph, service orchestrator, cost estimator | ✅ |
| `friday-simulation` | `simulation/` | Plan simulation / dry-run / validation / optimization / approval | ✅ |
| `friday-context` | `context/` | Context Builder — assembles `AgentContext` for the intelligence layer | ✅ |
| `friday-intelligence` | `intelligence/` | Cognitive router, prompt compiler, providers, response interpreter | ✅ |
| `friday-recovery` | `recovery/` | Recovery graph, failure taxonomy, confidence model | ✅ (shadow mode in runtime) |
| `friday-benchmark` | `benchmark/` | Resilience dashboard, MTBF / first-pass / recovery metrics | ✅ |
| `friday-mobile` | `mobile/` | Mobile protocol daemon (WebSocket) for phone clients | ✅ |
| `provider-qual` | `provider-qual/` | LLM/provider qualification harness | ✅ |
| `windows-qual` | `windows-qual/` | Windows action qualification harness | ✅ |

## Adapters (under `action-runtime`)

| Adapter | Tool key | Notes |
| ------- | -------- | ----- |
| Filesystem | `filesystem` | create/read/write/delete/rename/copy/move/list |
| Terminal | `terminal` | Persistent shell session |
| Git | `git` | CLI-backed capabilities (clone, status, commit, …) |
| MCP | `mcp` / dynamic | First-class `McpAdapter` (see `mcp/` crate + ADR-0007) |
| Simulation | `sim` | Side-effect-free simulation path |

Windows automation is **not** an adapter in this folder alone: Rust client is
`plugins/windows` (`friday-windows`); C# host is `windows-host/`.

## Related

- Specs: `specs/kernel.md`, `specs/capabilities.md`, `specs/services.md`,
  `specs/simulation.md`, `specs/context.md`, `specs/intelligence.md`,
  `specs/recovery.md`, `specs/benchmark.md`
- Design notes: `docs/CapabilityIntelligence.md`, `docs/Execution.md`,
  `docs/WindowsBridge.md`
