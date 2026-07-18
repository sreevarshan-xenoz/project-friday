# Execution Runtime Unification (M8)

**Status:** ✅ implemented (commit `8ea17d9`). **Spec-first:** this document records
the architecture that M8 locked in.

M8 makes MCP *disappear* from the runtime architecture. Before M8, an MCP
capability had no first-class execution path in production — only a test shim
bridged `EXEC_ACTION` to an `McpClient`. After M8, an MCP server is registered as
just another `ActionAdapter`, and the supervisor/planner/kernel never branch on
backend type.

## Why M8 exists

The original runtime (`AgentKernel` in `services/action-runtime`) already had the
correct abstraction. An `ActionAdapter` is a swappable capability backend; the
kernel routes by tool key and centralizes permission, timeout, cancellation,
retry, audit, and telemetry. The *missing piece was not a new dispatcher* — it was
a single adapter implementation: `McpAdapter`.

Discovering this kept M8 small and surgical. We did not introduce a
`CapabilityDispatcher` hierarchy or move dispatch logic: the existing
`ActionAdapter` / `run_action` pair already satisfied every M8 requirement. The
lesson is recorded as ADR-0007: when the runtime already expresses the right
abstraction, extend it with one more implementation rather than reorganizing it.

## Current execution flow

```
USER
  │
GOAL_RAW
  │
Planner (IntentEngine → PlanningEngine → TaskGraph)
  │
CapabilityRegistry.resolve(capability) → ExecutionBinding{ adapter, action }
  │
GoalSupervisor  (risk gate, then emits EXEC_ACTION)
  │
EXEC_ACTION ──────────────────────────────┐
  │                                         │
  ▼                                         ▼
AgentKernel::on_exec_action                (other subscribers)
  │
split_capability("mcp:fs.read_file")
  → ("mcp:fs", "read_file")
  │
ActionAdapter  (lookup by tool key "mcp:fs")
  ├── NativeAdapter    (filesystem, terminal, git, …)
  ├── PluginAdapter
  ├── McpAdapter       ← M8: MCP is here, indistinguishable from the rest
  └── Future RemoteAdapter
  │
run_action(): permission → timeout/deadline → cancellation → retry → audit → metrics
  │
EXEC_RESULT { capability, task_id, plan_id, success, output, error }
  │
GoalSupervisor → VerificationEngine → GOAL_COMPLETED
```

The planner and supervisor observe only events (`EXEC_ACTION` in, `EXEC_RESULT`
out) and a capability id. They never see the adapter type.

## Design invariants

These are the properties M8 commits to. They are enforced by construction (the
kernel routes only by tool key) and by the absence of `CapabilitySource` branching
in production code.

1. **The planner never branches on backend type.** Capability resolution returns an
   opaque `ExecutionBinding`; backend identity is not inspected.
2. **The supervisor never branches on backend type.** It emits `EXEC_ACTION` and
   waits for `EXEC_RESULT`; it does not care how the action runs.
3. **The dispatcher routes only by capability / tool key.** `split_capability`
   yields `(tool, action)`; the adapter map is keyed by `tool` (e.g. `mcp:fs`).
4. **Every backend receives the same `ExecutionContext`.** Carries
   `execution_id`, `session_id`, `cwd`, `environment`, `trace_id`, `deadline`, and
   a shared `CancellationToken`. Backends consume services; they do not construct
   them.
5. **Every backend returns the same `ActionResult`.** Normalized
   `{ success, output, error, metadata }` — independent of how the work was done.
6. **Cross-cutting concerns are runtime concerns, not backend concerns.** Permission
   gating, timeout/deadline, cancellation, retries, audit, and telemetry all live
   in `AgentKernel::run_action`. Adding a backend does not re-implement any of them.

## Extending the runtime (adding a new backend)

To add a new capability source (a new protocol, a remote federation layer, a
plugin host), implement `ActionAdapter` and register it. **No planner changes, no
supervisor changes, no scheduler changes.**

1. Implement `ActionAdapter`:
   - `tool()` returns the tool key the planner resolves to (for MCP this is
     `mcp:<server>`).
   - `actions()` returns the action verbs (for discovery/logging).
   - `execute(&self, req: ActionRequest) -> ActionResult` performs the work and
     returns a normalized result. It must **not** make permission decisions, emit
     events, or talk to other adapters.
2. Register it with the kernel:
   - `AgentKernel::register_adapter(Box::new(MyAdapter::new(…)))`, or for live
     connections `AgentKernel::register_mcp_server(name, transport)` (which
     connects + initializes + lists tools + registers an `McpAdapter`).
3. Publish the capability into the planner's `CapabilityRegistry` (same `id` as the
   adapter's `tool()` + action), so resolution binds to the right adapter.

That is the complete surface. The backend inherits permission, cancellation,
timeout, retry, audit, and telemetry for free.

## Intentionally out of scope

M8 froze the runtime shape. The following are deliberately deferred and become
trivial once the abstraction is settled — they are NOT reasons to revisit the
dispatcher:

- MCP server hot-reloading / dynamic (re)registration.
- Authentication / OAuth for remote MCP.
- Remote capability federation and distributed execution.
- Multiplexed MCP protocol (multiple in-flight requests on one connection).
- Capability result caching and transport pooling.

## Crates

| Concern            | Crate                     | Notes                                  |
|--------------------|---------------------------|----------------------------------------|
| Dispatch + runtime | `services/action-runtime` | `AgentKernel`, `run_action`, adapters  |
| Adapter trait      | `services/action-runtime` | `adapters::ActionAdapter`              |
| MCP adapter        | `services/action-runtime` | `adapters::mcp::McpAdapter` (M8)       |
| MCP protocol/transport | `friday-mcp`          | frozen; only bug-fixes                 |
| Planner / supervisor | `friday-planner`       | emits/consumes events; no MCP knowledge|
