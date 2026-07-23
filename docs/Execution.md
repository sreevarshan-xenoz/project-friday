# Execution Design (LOCKED)

This document is part of the frozen architecture. Changes require an explicit
review and a version bump.

> **Implementation note (non-breaking):** the planner's Execution Engine still
> publishes `exec.action` onto the bus. The **AgentKernel**
> (`friday-actions` / `services/action-runtime`) is the sole runtime that
> turns those into `ActionRequest`s, runs the permission + audit gate, dispatches
> `ActionAdapter`s (filesystem, terminal, git, windows, MCP, sim), and emits
> normalized `exec.result`. See `specs/kernel.md` and ADR-0007 for the unified
> runtime. Capability names in production prefer `windows.*` / `filesystem.*`
> style; older `win.*` / `shell.*` examples below remain design vocabulary.

## Principle

The Execution Engine chooses **which plugin / capability** performs an action and
lets the adapter decide **how**. The engine itself contains no
Windows/terminal/browser logic.

## Inputs / outputs

- **Consumes:** `plan.created` (and individual `plan.step` events).
- **Produces:** `exec.action` (the dispatched invocation) and `exec.result`
  (outcome + artifacts).
- **Always followed by:** the Verification Engine (`verify.passed` / `verify.failed`).

## Dispatch

1. Read the step's target capability (e.g. `win.open`, `shell.run`,
   `fs.write`).
2. Resolve the registered plugin that subscribes to that capability's event kind.
3. Publish the action event onto the bus.
4. Await the plugin's result event (correlated via `trace_id` / `reply_to`).
5. Forward `exec.result` to Verification.

## Permission gate

Before any step that touches the outside world, the Execution Engine consults the
**permission layer** (see `Security.md`). Forbidden actions are blocked by
default; everything else requires an approved allow-list or an explicit prompt.
No plugin may bypass this gate.

## Plugins owned by execution

| Capability     | Event kind prefix | Implementation language |
| -------------- | ----------------- | ----------------------- |
| Windows Skill  | `win.*`           | C#                      |
| Terminal       | `shell.*`         | Rust                    |
| Filesystem     | `fs.*`            | Rust                    |
| Browser        | `browser.*`       | Python                  |
| Coding         | `code.*`          | Python                  |
| Git            | `git.*`           | Rust                    |

## Determinism & safety

- Steps execute in plan order unless a step signals a dependency.
- Each action records an audit entry (who/what/when/trace id).
- The Execution Engine never loops on its own; retry/replan is driven by
  Verification (`Planner.md`).
