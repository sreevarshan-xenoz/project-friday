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

## M4.5 — Execution Plugins (three independent deliverables) 🔶

Built one at a time to validate the stable kernel before introducing
OS-specific or browser complexity.

- **M4.5.1 — Git Plugin** ✅ — `GitAdapter` (`adapters::git`), `tool = "git"`.
  Git is exposed as **capabilities** (`clone`, `init`, `status`, `add`,
  `commit`, `checkout`, `branch`, `merge`, `diff`, `push`, `pull`), each backed
  by the Git CLI behind a normalized `ActionResult`. Swappable to `libgit2`
  later with nothing above changing. Proves the action model is abstract enough.
- **M4.5.2 — Windows Bridge** ✅ — *just IPC*, a device driver for Windows.
  Rust (`plugins/windows`, crate `friday-windows`, platform-agnostic) →
  **Named Pipes** (versioned `BridgeFrame` over `friday_core::protocol`) →
  standalone C# `windows-host` microservice → Win32 / UI Automation / COM /
  PowerShell. Exposes *intent*, not Win32 APIs: `windows.open/close/focus/
  move/resize/capture/list/foreground` plus vision-ready `windows.click/type/
  scroll` and `windows.clipboard.*`. The Rust side never sees an `HWND` or
  `HRESULT`; all errors are normalized into a `BridgeError` set; every action is
  async + lifecycle-aware (maps onto the kernel `LifecycleManager`). A capability
  manifest is shipped (feeds M4.6). Design locked in `docs/WindowsBridge.md`;
  covered by cross-platform mock-host tests + a real-Notepad harness (skipped in
  CI, opt-in via `FRIDAY_WINDOWS_HOST_TEST`).
- **M4.5.3 — Browser Runtime** 🚧 — a **browser session manager**, not "Chrome
  automation": `BrowserRuntime → Session → Tab → Page → DOM`. Actions like
  `OpenTab`, `Navigate`, `Click`, `Fill`, `Scroll`, `WaitFor`, `Screenshot`,
  `ExtractText`, `EvaluateScript`. Every action is session-aware.

## M4.6 — Capability Intelligence ✅

Renamed from "Capability Registry": it is now the **Knowledge** subsystem — the
single source of truth for *what FRIDAY can do, who can do it, and how well*.

- `friday-capabilities` crate (`services/capabilities`):
  - **Rich `Capability` model** — `id`, `name`, `group`, `actions`,
    `permissions`, `platform`, `latency_ms`, `reliability`, `version`,
    `dependencies`, `resource_requirements`, `supports_parallel`, `dry_run`,
    `available`, `health`, `cost`, `confidence`.
  - **`CapabilityRegistry`** — `provider → capabilities`; query, rank, filter by
    availability. Planner reasons over capabilities + ranked providers, not
    hardcoded plugin names.
  - **Provider ranking** — `score = reliability*100 - latency*0.05 - cost`
    (Browser `open_url` → 98 vs Windows → 81; no hardcoding).
  - **Health + `Monitor`** — `Healthy/Busy/Offline/Recovering/Disabled` states,
    periodic probes, and a self-healing recovery hook (runtime restarts a
    crashed service).
  - **Manifest discovery** — services ship `manifest.json`; startup scans →
    validates → registers. Dynamic capabilities (e.g. `browser.chrome.*` appears
    when Chrome is installed) are data, not code.
  - **`CapabilityGraph`** — capabilities grouped by `group` for LLM traversal
    (token savings at M5).
- Existing services publish capabilities: `filesystem`, `terminal`, `git`,
  `windows` (verified by an integration test aggregating real adapters).
- Design locked in `docs/CapabilityIntelligence.md`; full test coverage
  (ranking example, health/monitor, manifest, graph, integration).

> **Terminology shift (authorized restructure):** we now speak of **services**
> (a filesystem service, a git service, a windows service, a future browser
> service) rather than bare plugins, and of **subsystems** rather than a flat
> milestone list — see the `docs/Architecture.md` addendum. The `ActionAdapter`
> trait is unchanged; "service" is the organizing term above it.

## M4.7 — Dry Run / Plan Preview 🚧

Every plan should be executable **without side effects** before it runs. The
runtime renders a preview (steps, estimated time, affected resources) and awaits
approval — critical for safety, debugging, and user trust (e.g. "Delete
everything in Downloads" → preview of 143 files + recycle bin, ~18s, awaiting
approval). Implemented as a mode on the execution path. The Capability model's
`dry_run` flag lets the planner skip providers that cannot simulate safely, and
the kernel's `ExecutionContext` already carries the hooks to no-op side effects.

## M4.8 — Workflow Recorder 🚧

Record successfully executed plans as reusable workflows. Built on the
`CapabilityGraph` (which tasks map to which capabilities) so a recorded workflow
is portable across providers — if Browser later beats Windows for `open_url`, the
same workflow re-routes automatically.

> **Event envelope (deferred, non-breaking):** rather than retrofitting
> `execution_id`/`trace_id` onto every frozen event type, future work wraps
> events in `EventEnvelope<T> { event_id, execution_id, trace_id, timestamp,
> payload }`. The inner event schema is unchanged; the envelope adds distributed
> tracing, replay, and correlation. This is the preferred path over editing the
> locked event types.

## Revised ordering (subsystems, not just milestones)

```
✅ M1  Foundation
✅ M2  Memory
✅ M3  Planner
✅ M4  Execution Kernel (runtime, lifecycle, context, resources)
🚧 M4.6 Capability Intelligence
🚧 M4.7 Dry Run Engine
🚧 M4.8 Workflow Recorder
⏳ M5  Intelligence Layer
⏳ M6  Browser Runtime      (now trivial: just declares capabilities)
⏳ M7  Vision
⏳ M8  Voice
⏳ M9  Learning
⏳ M10 Autonomy
```

Because Capability Intelligence exists, M5 becomes `LLM → need capability →
registry → best provider → runtime → result`; the LLM never sees implementation
details. And the Browser service (M6) is now tiny — it only declares
`Navigate/Click/Type/Screenshot/ExtractText`; the registry handles routing,
ranking, health, and discovery.

## Subsystem map

Stop thinking in "plugins"; think in **services** inside **subsystems**:

- **Kernel** — execution, lifecycle, context, resources (`friday-actions` + kernel).
- **Knowledge** — memory (`friday-core`), Capability Intelligence
  (`friday-capabilities`), registry.
- **Services** — filesystem, git, terminal, windows, (future) browser, vision.
  A service may internally use multiple plugins/bridges/adapters.
- **Perception** — vision, voice.
- **Cognition** — planner, LLM routing, reasoning (`friday-planner`, M5).
- **Autonomy** — learning, workflows, background agents.

This matches the architecture already built and scales to Windows / Android /
Linux / cloud workers / remote machines.

## M5 — Intelligence Layer ⏳

- LLM provider abstraction (local via Ollama + cloud fallback).
- Local/cloud model routing, structured output, tool selection, context
  assembly.
- Replaces the rule-based Intent classifier and Planning heuristics with
  model-driven ones behind the same event contract, querying Capability
  Intelligence instead of knowing plugins.

## M6 — Browser Runtime ⏳

A **browser session manager**, not "Chrome automation": `BrowserRuntime →
Session → Tab → Page → DOM`. Actions `OpenTab`, `Navigate`, `Click`, `Fill`,
`Scroll`, `WaitFor`, `Screenshot`, `ExtractText`, `EvaluateScript`. Session-aware.
Thanks to M4.6 it only declares capabilities; the registry handles the rest.

## M7 — Vision ⏳

- Screen understanding, OCR, UI automation, window discovery (consumes the
  Windows bridge's `click`/`type`/`capture` intents).

## M8 — Voice ⏳

- Wake word, VAD, streaming STT, streaming TTS, interruptions.

## M9 — Learning ⏳

- Habit detection, preference extraction, workflow learning, knowledge-graph
  updates.

## M10 — Autonomy ⏳

- Background tasks, long-running jobs, scheduling, goal monitoring,
  self-recovery.

> The four reasoning engines and the standard event vocabulary are **locked**
> (see `Architecture.md`, `Planner.md`, `Execution.md`). New features must answer:
> which event do they consume, which do they produce, which plugin owns them.
>
> **Architectural discipline (must hold through M5+):** the core stays ignorant —
> the planner never knows plugin internals, plugins never know about each other,
> and the runtime only knows about capabilities and contracts.
