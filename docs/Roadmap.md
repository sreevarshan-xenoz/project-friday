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

All M4.1–M4.4 paths covered by unit + end-to-end tests wired into `Friday`.
(The next sub-milestones — M4.4.5 (kernel), M4.5 (Windows/Git/Browser), M4.6
(Capability Registry) — are documented as dedicated sections below.)

## M4.4.5 — Execution Context & Resource Manager ✅ (kernel frozen)

Today the flow is `Planner → Action Runtime → Adapter`, but every action was
stateless and independent. M4.4.5 introduces the **stable kernel** every future
capability depends on, implemented in a `kernel` module inside `friday-actions`
and wired into the `ActionRuntime`.

- **Execution Context** — immutable [`ExecutionContext`] carried by every action:
  `execution_id`, `session_id`, `parent_execution`, `user_id`, `cwd`,
  `environment`, `variables`, `permissions`, `trace_id`, `deadline`,
  `metadata`, plus a shared [`CancellationToken`]. Child contexts inherit the
  parent's session/identity and are cancelled together; parallel DAG branches
  get isolated children and [`ExecutionContext::merge`] folds them back.
- **Lifecycle Manager** — one [`LifecycleManager`] state machine
  (`Created → Waiting → Running → Paused → Cancelled → Completed → Archived`)
  shared by tasks, sessions, resources, and workflows.
- **Resource Manager** — the sole owner of long-lived resources via the
  [`ManagedResource`] trait (`acquire`/`release`/`heartbeat`/`shutdown`);
  `shutdown_all` reclaims everything on stop so nothing leaks.
- **Session Manager** — [`SessionManager`] preserves continuity (cwd, env,
  shell history, variables, auth) so work continues instead of restarting.
- **Metrics** — automatic per-execution [`MetricSample`] (planning/queue/
  execution time, retries, plugin used, success, resource placeholders) with
  `success_rate` and `avg_execution_ms` for "why is Browser slower than
  Terminal?"-style questions.
- **Wiring** — `ActionRuntime::run_action` is the single public dispatch path:
  permission → timeout + deadline + **cancellation** + bounded retries →
  metrics → audit → lifecycle, attaching `execution_id` to `exec.result`.

> **Runtime API is now declared STABLE.** M4.5 (Windows/Git/Browser), M4.6
> (Capability Registry), and M5+ must build on these contracts without changing
> them. All kernel features covered by unit + integration tests (context
> inheritance/merge, lifecycle transitions, resource acquire/release,
> cancellation propagation, metrics recording).

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
