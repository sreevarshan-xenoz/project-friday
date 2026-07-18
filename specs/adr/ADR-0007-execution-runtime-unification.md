# ADR-0007: Execution Runtime Unification — MCP as a first-class ActionAdapter

- **Status:** Accepted
- **Date:** 2026-07-18
- **Deciders:** FRIDAY architecture (M8)
- **Supersedes:** — (no prior dispatcher for MCP in production; only a test shim)
- **Superseded by:** —

## Context

FRIDAY's action runtime (`AgentKernel` in `services/action-runtime`) dispatches
`exec.action` events to `ActionAdapter` implementations (filesystem, terminal,
git). The MCP subsystem (`friday-mcp`) provides transports, a JSON-RPC protocol,
discovery, and health monitoring, and registers MCP capabilities into the planner's
`CapabilityRegistry`. But production had **no execution path** for an `mcp:`
capability: the supervisor emitted `EXEC_ACTION`, yet nothing routed it to an
`McpClient`. The vertical slice was closed only by a `McpExecutor` *test shim*.

Two paths were open:

1. Add an `McpExecutor` plugin (or a `CapabilityDispatcher` hierarchy) that
   special-cased `CapabilitySource::Mcp`, branching in the supervisor/kernel.
2. Implement `McpAdapter: ActionAdapter` and register it under the `mcp:<server>`
   tool key, reusing the existing `run_action` dispatch unchanged.

## Decision

**Implement `McpAdapter` as an `ActionAdapter` and register it under the tool key
`mcp:<server>` (the planner's capability id).** Do **not** introduce any
`CapabilitySource::Mcp` branch in the planner, supervisor, or kernel.

`AgentKernel::register_mcp_server(transport)` connects, initializes, lists tools,
and registers the adapter — identical in shape to registering a native adapter.
Routing stays `split_capability("mcp:fs.read_file") → ("mcp:fs", "read_file")` →
adapter lookup by `mcp:fs`. Cancellation, timeout, permissions, retries, audit,
and telemetry are inherited from `run_action` with zero backend-specific code.

## Consequences

**Positive**

- MCP is now indistinguishable from a native backend at every layer above the
  adapter. The supervisor and planner have zero MCP-specific code.
- Adding *any* new backend follows the same one-implementation path: implement
  `ActionAdapter`, register it, publish the capability. No changes to planner,
  supervisor, or scheduler.
- Cancellation is unified via the shared `CancellationToken` and the kernel's
  `tokio::select!` — a crashed or slow MCP server surfaces exactly like a native
  failure (clean error, no hang, retry/audit applied).
- The `ExecutionContext` / `ActionResult` contract is honored uniformly, so
  streaming, auditing, and telemetry work for MCP without new code.

**Negative / trade-offs**

- The MCP wire protocol is single-flight (`McpClient` is `&mut`), so the adapter
  serializes calls per server behind a `Mutex`. Concurrent tasks fan into one
  connection. A multiplexed client (future work) would relax this without changing
  the adapter boundary.
- `friday-mcp` depends on `friday-planner`, and `action-runtime` now depends on
  `friday-mcp`; the dependency direction stays acyclic (`action-runtime → mcp →
  planner`, and `planner`'s `mcp` dependency is dev-only in the integration tests).

## Alternatives considered

- **`McpExecutor` plugin branching on `CapabilitySource::Mcp`.** Rejected: it bakes
  backend-type discrimination into the dispatch surface and replicates the
  permission/timeout/cancel/retry/audit logic that `run_action` already owns. It
  also leaves the supervisor/kernel knowing about MCP.
- **New `CapabilityDispatcher` trait + registry of `CapabilityExecutor`.** Rejected
  as over-engineering: `ActionAdapter` already *is* that trait, and `AgentKernel`
  already *is* that dispatcher. Introducing a second parallel hierarchy would
  duplicate routing, lifecycle, and telemetry.
- **Fold MCP handling into `GoalSupervisor`.** Rejected: the supervisor is defined
  to contain no execution logic; it owns goal state, gating, reflection, and
  persistence. Execution belongs in the runtime.

## Follow-ups (explicitly deferred, not part of this decision)

Hot-reload, OAuth, remote federation, multiplexing, and transport pooling are
recognized as future work that layers *onto* this boundary without altering it.
