# Roadmap

Milestones are intentionally small and each leaves the project in a runnable
state. Status follows the agreed phase order.

## M0 — Foundation ✅

- Folder structure, git branches, tooling (cargo, uv, node).
- Documentation suite (`docs/*.md`).
- Coding standards + pre-commit + CI skeleton.

## M1 — Core Runtime ✅

- `friday-core` Rust crate: async **event bus**, **Plugin SDK** +
  `PluginManager` router, layered **memory** interfaces, **protocol**
  transport, **runtime** wiring, **logging** (tracing).
- Reference `echo_plugin` example + integration test (passing).

## M2 — Memory Engine ✅

- `WorkingMemory` (in-process, ephemeral).
- `SessionMemory` and `LongTermMemory` over an embedded **redb** store
  (pure-Rust ACID; chosen over SQLite because this dev machine's Rust toolchain
  is GNU with no C compiler, so `rusqlite` cannot build here — see `Memory.md`).
- `VectorMemory` with in-memory cosine-similarity search over stored embeddings.
- `MemoryEngine::default_layers` convenience.

## M3 — Planner Engine ✅

- `friday-planner` crate with four independent engine plugins:
  - **Intent Engine** — `goal.raw` → `intent.created` (rule-based parser/classifier).
  - **Planning Engine** — `intent.created` → `plan.created` (builds a DAG `TaskGraph`).
  - **Execution Engine** — `plan.created` → `task.started` / `exec.action` → `task.completed`.
  - **Verification Engine** — `task.completed`/`task.failed` → `verification.*`, driving retry/replan.
- Plans are a **DAG**; every task has an explicit lifecycle (`TaskState`).
- End-to-end pipeline test against a `MockExecutor` (happy path + retry path).

## M4 — Execution Plugins 🔶 (in progress)

Split into five sub-milestones, built strictly **generic-first** (the Action
Runtime knows nothing about terminals/files; every plugin consumes a normalized
`ActionRequest`/`ActionResult`, and every action funnels through the permission
gate + audit engine before reaching an adapter).

- **M4.1 — Action Runtime** ✅ — `friday-actions` crate (`services/action-runtime`).
  Generic dispatcher subscribing to `exec.action`: it translates the planner's
  `capability` into a normalized `ActionRequest`, asks the `PermissionEngine`,
  dispatches to the matching `ActionAdapter` with **timeout + bounded retries**,
  records an `AuditEntry`, and emits a normalized `exec.result`. Knows nothing
  about what an action *does*.
- **M4.2 — Permission & Audit** ✅ — `PermissionEngine` (sole authority;
  `AllowOnce` / `AllowAlways` / `Deny` / `Sandbox`; always-deny list for
  forbidden actions from `Security.md`) and `AuditEngine` (bounded in-memory
  breadcrumb ring: who/what/when/trace/latency/result). Plugins never decide
  their own permissions.
- **M4.3 — Filesystem Plugin** ✅ — `FilesystemAdapter`: `create_file`,
  `read_file`, `write_file`, `delete_file`, `rename`, `copy`, `move`,
  `list_directory`.
- **M4.4 — Terminal Plugin** ✅ — `TerminalAdapter` over a **persistent shell
  session** (same process/cwd/env across calls), with a sentinel-echo protocol
  over merged stdout/stderr.
- **M4.5 — Windows / Browser / Git** ⏳ — Windows via C# over IPC (isolated
  under `plugins/windows/{bridge,ipc,actions}`); Browser postponed; Git exposed
  as actions (not CLI wrapping).

All M4.1–M4.4 paths covered by unit + end-to-end tests wired into `Friday`.

## M4.4.5 — Execution Context & Resource Manager 📋 (next)

Today the flow is `Planner → Action Runtime → Adapter`, but every action is
stateless and independent. We are missing the **execution context** that binds a
related sequence of actions ("open VS Code → compile → run → read log → fix →
run again") into one unit, so state isn't hand-carried between plugins.

- **Execution Context** carried by every action:
  `session_id`, `cwd`, `environment`, `user`, `permissions`, `variables`,
  `cancellation_token`, `trace_id`, `started_at`.
- **Resource Manager** that *owns* long-lived resources so plugins can't leak
  them: terminal sessions, browser sessions, windows, websocket connections,
  file watches, microphones, cameras, clipboard. Each resource is created,
  referenced by id, and reclaimed through the manager.

## M4.5 — Windows Bridge, Git Plugin, Browser Runtime 📋 (split into three)

No single "execution plugins" blob; three independent deliverables:

1. **Windows Bridge** — *just IPC*. Rust → IPC → C# → Windows API. No planner
   logic, no permissions, no AI. Windows calls only. Isolate under
   `plugins/windows/{bridge,ipc,actions}`.
2. **Git Plugin** — define actions (`GitClone`, `GitCommit`, `GitPush`,
   `GitCheckout`, `GitBranch`, `GitDiff`, `GitStatus`). CLI-backed initially,
   swap to `libgit2` later with nothing above changing. Exposed as actions, not
   shell wrapping.
3. **Browser Runtime** — treat the browser as a *persistent session* exactly
   like Terminal: `Browser Session → Tab → Page → DOM → Actions`. Never launch
   Chrome per command.

## M4.6 — Capability Registry 📋 (next big addition)

Replace the flat `PluginManager → plugins` view with a `CapabilityRegistry`
that plugins advertise into. Each capability declares:

```rust
Capability {
    name, description, actions,
    permissions, cost, latency,
    requires_confirmation,
}
```

The planner then reasons over *capabilities* ("I need `create_file`"), not
plugin names. This becomes decisive once multiple plugins expose the same
capability and the platform must route by cost/latency/confirmation.


## M5 — Intelligence Layer ⏳

- LLM provider abstraction (local via Ollama + cloud fallback).
- Local/cloud model routing, structured output, tool selection, context
  assembly.
- Replaces the rule-based Intent classifier and Planning heuristics with
  model-driven ones behind the same event contract.

## M6 — Perception ⏳

- Screen understanding, OCR, UI automation, window discovery.

## M7 — Voice ⏳

- Wake word, VAD, streaming STT, streaming TTS, interruptions.

## M8 — Learning ⏳

- Habit detection, preference extraction, workflow learning, knowledge-graph
  updates.

## M9 — Autonomy ⏳

- Background tasks, long-running jobs, scheduling, goal monitoring,
  self-recovery.

> The four reasoning engines and the standard event vocabulary are **locked**
> (see `Architecture.md`, `Planner.md`, `Execution.md`). New features must answer:
> which event do they consume, which do they produce, which plugin owns them.
>
> **Architectural discipline (must hold through M5+):** the core stays ignorant —
> the planner never knows plugin internals, plugins never know about each other,
> and the runtime only knows about capabilities and contracts.
