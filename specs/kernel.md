# Kernel Specification — AgentKernel

The **Kernel** owns execution. It is the bottom of the stack: every action funnels
through it. Renamed from `ActionRuntime` to `AgentKernel` to reflect its role as the
kernel of the agent operating system.

## Crate

`services/action-runtime` — crate `friday-actions`. Type: `AgentKernel`
(`runtime.rs`).

## Responsibilities

The kernel owns, and only owns:

- **Execution** — single dispatch path `AgentKernel::run_action` (permission →
  timeout + deadline + cancellation + bounded retries → metrics → audit →
  lifecycle). Attaches `execution_id` to `exec.result`.
- **Lifecycle** — `LifecycleManager`: `Created → Running → Completed | Failed |
  Cancelled` transitions, cancellation propagation.
- **Context** — `ExecutionContext`: inherits and merges parent context; carries the
  hooks to no-op side effects (used by Simulation M4.7).
- **Resources** — `ResourceManager`: acquire/release (file locks, ports, etc.).
- **Sessions** — `SessionManager`: per-goal execution sessions.
- **Metrics** — `Metrics`: per-action timing, counts, failures.
- **Permissions & Audit** — authorization before dispatch; immutable audit log of
  every action/result.
- **Capability routing** — resolves an action's `tool` to a registered
  `ActionAdapter` and consults `CapabilityRegistry` for health/availability.

## API stability

Declared **STABLE** at M4.4.5. M4.5/M4.6/M4.6.5/M4.7+ build on these contracts
without changing them. The `ActionAdapter` trait is unchanged.

## Consumes / produces

- Consumes: `exec.action`
- Produces: `exec.result`, `task.started`, `task.completed`, `task.failed`,
  `task.retry`
- Consumed by Simulation (M4.7) in `dry_run` mode (no side effects).

## See also

- `kernel.md` in `docs/Architecture.md` (frozen addendum).
- `Execution.md` (locked).
