# Roadmap

Milestones are intentionally small and each leaves the project in a runnable
state. Status follows the agreed phase order.

## Phase Model — Alpha Qualification (current)

All architecture milestones (M0–M5) are **complete** and the runtime API is
**frozen**. We are now in **Alpha Qualification**: the job is no longer to add
architecture — it is to *prove the system is reliable*. Every change from here is
measured against real numbers, not vibes.

Qualification phases:

1. **Alpha 1** — internal dogfooding build; everything wired end-to-end.
2. **Qualification** — Benchmark Sprint(s): break the platform with hundreds of
   tasks; track **MTBF**, **simulation accuracy**, **planner accuracy**,
   **recovery success**, and **confidence calibration**. No feature PR is merged
   without a benchmark that demonstrates the need or a measurable reliability gain.
3. **Reliability** — drive MTBF up; **Chaos Mode (`CHAOS=1`)** hardened; fallback
   paths made real, not declared.
4. **Dogfooding** — daily use by the team; telemetry + dashboards live.
5. **Beta** — external testers.
6. **Public Testing** — open.
7. **v1.0** — stable.

**PR policy:** reject any PR that adds a feature without (a) a benchmark that
demonstrates the need, or (b) a measurable reliability improvement. The hardest
real-world risk is **Windows** (Explorer, focus stealing, UAC, race conditions, AV,
keyboard hooks, clipboard, multi-monitor) — the benchmark harness weights
Windows-heavy task mixes accordingly.

The benchmark harness lives in `services/benchmark` (`friday-benchmark`); its spec
is `specs/benchmark.md`. Run it with `CHAOS=1 cargo run -p friday-benchmark -- 600`.

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
and wired into the `AgentKernel`.

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
- **Wiring** — `AgentKernel::run_action` is the single public dispatch path:
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

## M4.6.5 — Service Orchestrator + Cost Estimator ✅

Built **before** Dry Run so the kernel owns not just execution but the *lifecycle
and economics* of every service. Implemented in the `friday-capabilities` crate
(`services/capabilities`):

- **`ServiceOrchestrator`** — lifecycle state machine per service:
  `Created → Starting → Healthy → Recovering → Disabled`. Each service registers a
  `ServiceProbe` (liveness/health) and a `RecoveryPolicy` (steps to heal it).
- **`RecoveryPolicy` + `RecoveryStep`** — a `StaticRecoveryPolicy` runs an ordered
  set of steps (e.g. `RecoveryStep::Restart`) via a recovery hook; the orchestrator
  drives `Recovering` and maps outcomes back to `Healthy`/`Disabled`.
- **Background loop** — `start_loop(interval)` probes on a timer, transitions
  `Healthy → Recovering` on probe failure, and records **`Telemetry`** (restarts,
  failures, errors, last latency) per service.
- **`sync_to_registry()`** — pushes orchestrator health into the
  `CapabilityRegistry` (`set_provider_health`), so the Knowledge subsystem and the
  planner see live service health without knowing how it is produced.
- **`CostEstimator`** — `CostEstimator::estimate(capabilities)` returns a
  `PlanCost { estimated_ms, expected_success, risk, token_cost }` from a slice of
  `Capability`. Used by Dry Run (M4.7) to render cost/risk before a plan runs.
- Full test coverage (register/probe/healthy, offline→recover, disabled,
  background loop, telemetry, cost math).

> **`ActionRuntime` → `AgentKernel` rename.** The execution runtime type is
> renamed to reflect its role as the **kernel** of the Agent OS: it owns execution,
> permissions, audit, metrics, lifecycle, sessions, resources, and capability
> routing. The `run_action` dispatch path and all event contracts are unchanged.

> **Terminology shift (authorized restructure):** we now speak of **services**
> (a filesystem service, a git service, a windows service, a future browser
> service) rather than bare plugins, and of **subsystems** rather than a flat
> milestone list — see the `docs/Architecture.md` addendum. The `ActionAdapter`
> trait is unchanged; "service" is the organizing term above it. The execution
> runtime type is `AgentKernel` (`friday-actions`).

## M4.7 — Plan Simulation Engine 🚧

Not "Dry Run" — a whole **subsystem** that simulates an entire execution before it
happens, like a CPU simulator for plans. Dry Run is only one feature of it. The
planner produces a `TaskGraph`; the engine virtualizes every action by asking each
capability *"what would happen?"* — the filesystem reports 143 files deleted, git
3 commits, browser 4 tabs, windows "Notepad launched" — with **no side effects**.

Implemented as a new crate `friday-simulation` (`services/simulation`) with:

- **`simulator`** — walks the `TaskGraph`, calls each capability's `simulate`
  (virtual execution), accumulates `SimulatedEffect`s into a `SimulationReport`.
- **`validator`** — pre-flight checks: missing permissions, unavailable services,
  dependency failures, circular plans (DAG cycle), impossible actions.
- **`optimizer`** — pure graph optimization (no LLM): collapse redundant
  open/close, merge repeated navigations, reorder for `supports_parallel`
  concurrency. Turns `Open→Nav→Close→Open→Nav` into `Open→Nav→Nav→Close`.
- **`estimator`** — wraps `CostEstimator` into a `PlanCost` (time, success
  probability, risk, token cost) for the preview.
- **`approval`** — the user approves **plans, not actions**: a `PlanSummary`
  (N actions, windows, browsers, files, ~time, risk) → YES/NO.
- **`report`** — after execution, `Prediction vs Actual vs Difference` (e.g.
  predicted 5s, actual 7s → 71% accuracy). Feeds Learning (M9).

Capabilities must expose a `simulate` path; the `dry_run` flag gates providers that
cannot simulate safely. `AgentKernel`'s `ExecutionContext` already carries the
hooks to no-op side effects. Full spec: `specs/simulation.md`.

## M4.8 — Workflow Recorder 🚧

Record successfully executed plans as reusable workflows. Built on the
`CapabilityGraph` (which tasks map to which capabilities) so a recorded workflow
is portable across providers — if Browser later beats Windows for `open_url`, the
same workflow re-routes automatically.

## M4.9 — Plan Optimizer 🚧

Re-order and prune plan steps using the Capability Intelligence knowledge: prefer
higher-ranked providers (`score = reliability*100 - latency*0.05 - cost`), respect
`dependencies`/`resource_requirements`/`supports_parallel` for concurrency, and
skip `Recovering`/`Disabled` providers (via `ServiceOrchestrator` health). Uses
`CostEstimator` to choose the cheapest safe plan and to surface trade-offs to the
user before execution. Mostly statistics once M4.7 exists: it reuses the
simulator's `validator` + `optimizer` + `estimator` and adds learned weights from
Simulation `report`s (M9).

> **Event envelope (deferred, non-breaking):** rather than retrofitting
> `execution_id`/`trace_id` onto every frozen event type, future work wraps
> events in `EventEnvelope<T> { event_id, execution_id, trace_id, timestamp,
> payload }`. The inner event schema is unchanged; the envelope adds distributed
> tracing, replay, and correlation. This is the preferred path over editing the
> locked event types.

## M4.10 — Context Builder 🚧

The missing architectural piece before M5. M5 must **not** be `User → LLM`. It is
`User → Context Builder → LLM`. The Context Builder assembles everything the model
needs and the **LLM never assembles context itself**:

- relevant **memories** (`friday-core` memory),
- available **capabilities** + ranked providers (`CapabilityRegistry`),
- **active services** + their health (`ServiceOrchestrator`),
- **current windows** (Windows bridge) and **open browser tabs**,
- the **current project** (detected from cwd / git / recent files),
- **execution history** (`AgentKernel` metrics + audit),
- **recent failures** (`ServiceOrchestrator` telemetry),
- **user preferences** (memory).

Produces an `LLMContext` handed to the Intelligence Layer. Full spec:
`specs/context.md`. This is what makes M5 "intelligence" instead of "a prompt."

## Revised ordering (subsystems, not just milestones)

```
✅ M1  Foundation
✅ M2  Memory
✅ M3  Planner
✅ M4  Agent Kernel (AgentKernel: runtime, lifecycle, context, resources)
✅ M4.5 Services (filesystem, terminal, git, windows)
✅ M4.6 Capability Intelligence
✅ M4.6.5 Service Orchestrator + Cost Estimator
✅ M4.7  Plan Simulation Engine
✅ M4.8  Experience Engine (Procedural Memory)
✅ M4.10 Context Builder
✅ M5  Cognitive Engine (System 1/2 Router, Compiler, Response Interpreter)
🚧 M5.5 Evaluation & Benchmarking Framework
🚧 M5.6 Knowledge Graph
🚧 M5.7 Reasoning Session
⏳ M6  Browser Runtime           (now trivial: just declares capabilities)
⏳ M7  Desktop World Model (Vision)
⏳ M8  Voice
⏳ M9  Learning
⏳ M10 Autonomy

# Alpha Qualification (current phase)
Alpha 1 → Qualification → Reliability → Dogfooding → Beta → Public Testing → v1.0
```

## Reliability Sprint (the current work)

**North-star metric is graceful degradation, not feature count.** We reject feature
PRs that cannot justify themselves against the reliability benchmark. The headline
number is **Chaos MTBF** — mean tasks between failures when `CHAOS=1` randomly kills
providers. A recovered failure does **not** count against MTBF.

- **Recovery Graph** (`friday-recovery`, spec `specs/recovery.md`): action-level
  recovery policies keyed by `FailureCategory`. Pure, deterministic, serializable
  (authored in Rust or JSON). A missing policy → `recovered = false` → MTBF drops
  (regression detector).
- **Recovery Confidence**: every `RecoveryAction` carries a base confidence; below
  `APPROVAL_THRESHOLD` (0.5) or on `AskUser` the recovery escalates to the approval
  policy (human-in-the-loop).
- **Benchmark** (`friday-benchmark`): reports Recovery Success, mean Recovery
  Confidence, **per-subsystem Recovery %**, and an **MTBF trend** from historical
  `benchmarks/<date>.json` snapshots (`cargo run -p friday-benchmark -- --save`).

**Chaos MTBF target ladder:** `3 → 10 → 25 → 100 → 500`. We start from the honest
`3` (browser→windows fallback only) and climb as the Recovery Graph covers every
subsystem. Each run is a day snapshot so progress is auditable.

Because Capability Intelligence exists, M5 becomes `LLM → need capability →
registry → best provider → runtime → result`; the LLM never sees implementation
details. And the Browser service (M6) is now tiny — it only declares
`Navigate/Click/Type/Screenshot/ExtractText`; the registry handles routing,
ranking, health, and discovery.

## Subsystem map

Stop thinking in "plugins"; think in **services** inside **subsystems**:

- **Kernel** — `AgentKernel`: execution, lifecycle, context, resources, permissions,
  audit, sessions, capability routing (`friday-actions`).
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

## Specification discipline

From M4.7 onward, every subsystem has a stable specification **before**
implementation. The authoritative specs live in **`/specs`** (`README.md` indexes
them): `kernel`, `events`, `capabilities`, `planner`, `services`, `ipc`, `memory`,
`simulation` (M4.7), `context` (M4.10), `intelligence` (M5). `docs/` remains as
design history; `/specs` supersedes it for new work. The LLM sits **above** the
kernel (fed by the Context Builder), exactly as shown in `specs/README.md`.
