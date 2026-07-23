# M6 — Autonomous Execution Engine

Status: **Implemented (kernel)** — orchestration core, capability registry, risk
gating, reflection seam, event-sourced persistence, and a worker pool. The
rule-based reflector and `AllowAllPolicy` are in place; LLM reflection and
per-workspace policy are drop-in later swaps.

This delivers the first milestone from the roadmap: FRIDAY can take **one prompt**
and run a multi-step plan to completion with **no human in the loop**, pausing
only at explicit high-risk safety gates.

## Architecture

```
GoalSupervisor  (Plugin, planner crate — orchestration ONLY)
 ├─ CapabilityRegistry   capability id → ExecutionBinding{adapter,action,schema,permissions,risk}
 ├─ Scheduler            ready-queue + bounded worker pool → emits exec.action
 ├─ Risk gate            Safe/Medium dispatch; High → PAUSE (WAITING_FOR_APPROVAL)
 ├─ Reflection           Reflector trait → ReflectionAction (Continue/Retry/Repair/Split/Replan/Pause/Abort)
 ├─ Lifecycle            GoalState (Running/Paused/Completed/Failed/Cancelled) + ExecutionBudget
 └─ Persistence          event-sourced GoalEvent log + redb snapshots (resume/replay)
```

The supervisor contains **no execution logic**. It emits `exec.action` events;
the `AgentKernel` (M4) actually runs them. This keeps the core small and
testable and means MCP/plugin capabilities register through the same
`CapabilityRegistry` as native adapters — no future refactor.

## Modules (`planner/src/execution/`)

| File | Responsibility |
|---|---|
| `capability_registry.rs` | `CapabilityRegistry`, `CapabilitySpec`, `ExecutionBinding`, `Risk`. Resolution point for every capability. |
| `lifecycle.rs` | `GoalRuntime`, `GoalState` (separate from `TaskState`), `ExecutionBudget` (max_steps/retries/tokens/cost/deadline/timeout). |
| `scheduler.rs` | Builds `exec.action` events for the next ready batch, respecting pool size + step budget. |
| `reflection.rs` | `Reflector` trait + `RuleReflector` (rule-based) producing `ReflectionAction`. LLM reflector is a later swap. |
| `persistence.rs` | Append-only `GoalEvent` log + redb snapshot store. `GoalStore::{append,snapshot,load,replay}`. |
| `supervisor.rs` | The `Plugin`: owns goal lifecycle, risk gating, reflection dispatch, persistence. |

## Task lifecycle (separate from goal lifecycle)

```
Pending → Ready → Running → Verifying → Completed
                        ↘ Failed → Retrying → Running
                                    ↘ (retries exhausted) → Replan / Abort
```

`Task` now carries `progress`, `output`, `retries` (previously absent).

## Safety model

- **Safe** (`cargo.check`, `git.status`, …): execute immediately.
- **Medium** (`filesystem.write_file`, `git.branch`, …): policy-controlled; gated
  by `PolicyAuthority` (currently `AllowAllPolicy`).
- **High** (`git.push`, `filesystem.delete`, `terminal.rm_rf`, …): the supervisor
  **pauses** the goal (`GOAL_PAUSED` + `PERMISSION_PAUSED`), records the pending
  task, and waits. A `goal.resume` event (after human approval) resumes exactly
  that task. **The autonomous loop never dies — it pauses and resumes.**

`Risk` lives on the capability metadata; the `PermissionEngine` enforces policy.

## Events (clients subscribe to all of these)

```
goal.started  goal.paused  goal.resumed  goal.completed  goal.failed  goal.cancelled
task.ready  task.started  task.progress  task.verifying  task.completed  task.failed  task.retrying  task.blocked
plan.updated  goal.permission_paused
task.stream            (M6.4 — incremental output chunks for live rendering)
```
Plus the existing `exec.action` / `exec.result` / `verification.*` plumbing.

## Lock-ownership guideline (supervisor)

The supervisor holds `self.goals` (a `Mutex`) for the duration of each
`handle_event` call. **Never call a supervisor method that re-locks `self.goals`
while already holding it** (`std::sync::Mutex` is not reentrant → deadlock). All
helpers therefore take `&mut GoalRuntime` (already borrowed). The only methods that
take the lock are the top-level `on_*` entry points driven by `handle_event`.
External locks (`CapabilityRegistry`, `GoalStore`) never acquire `self.goals`, so
touching them while holding the goal lock is safe.

## M6.1 — Native capability registration

The M6 `CapabilityRegistry` resolves a **canonical per-action id**
(`filesystem.write_file`, `git.push`, …) to an `ExecutionBinding`. The existing
`friday_capabilities` crate holds the grouped native definitions (one
`Capability` per adapter); `planner/src/execution/native_caps.rs` expands each
into per-action `CapabilitySpec`s and assigns a `Risk`:

| Adapter | Safe | Medium | High |
|---|---|---|---|
| filesystem | read_file, list_directory | create_file, write_file, rename, copy, move | delete_file |
| git | status, diff, log | clone, init, add, checkout, branch, merge, pull, commit | push |
| terminal | list_sessions, kill | — | run, exec |

`register_native_builtins` runs once inside `register_engines` so a default build
has every native capability available; MCP/plugin capabilities register through the
same `register` call later — the supervisor never changes. The grouped definitions
live in `friday_capabilities::builtins` so both the action adapters and the planner
share one source of truth (this also broke a cyclic `friday-actions → friday-planner`
dependency, which is why the metadata was relocated).

## M6.3 — Shared goal execution context

`GoalRuntime.context` is a goal-scoped `GoalContext` (a serializable bag of
`ContextEntry`s) that tasks publish into and later tasks read from. A task declares
`Task.produces: Option<String>`; on completion its output is published under that
key (`supervisor.rs` → `rt.context.publish`). Downstream tasks (or an LLM planner)
consume it to avoid recomputing shared work (e.g. `cargo metadata` →
`workspace_root`, `git.branch`). It is part of the persisted goal state, so a
resumed goal keeps its accumulated knowledge. `GoalEvent::ContextPublished` records
the publish for replay.

## M6.4 — Streaming task output

`EXEC_RESULT` may carry `progress` (0..1) and `stream` (array of text chunks). The
supervisor emits `task.progress` (live fraction) and `task.stream` (one event per
chunk) so the Android app, CLI, and future desktop UI render incremental output
(`Checking serde…`, `Compiling planner…`) instead of a single `Done.` — with no
architecture change downstream.

## M6.2 — LLM Planner (planner is just another reasoning module)

The planning engine now owns a `PlannerRouter` and produces a `TaskGraph` via one
of several `Planner` implementations — both yield the **same** `TaskGraph`, so the
supervisor is oblivious to which planner ran:

```
User Goal → Intent → PlannerRouter → { RulePlanner | LlmPlanner }
                            ↓
                   validated TaskGraph → GoalSupervisor
```

* **`Planner` trait** (`planning/planners/planner_trait.rs`) — the single contract
  every planner satisfies: `create_plan(&Intent, &PlanContext) -> Result<TaskGraph>`.
* **`RulePlanner`** — the original deterministic decomposition (browser.search /
  open.app / routine.morning / generic). Free, O(1), no model.
* **`LlmPlanner`** (`planning/planners/llm.rs`) — asks a `ReasoningBackend` for a
  *structured* plan (goal + available capabilities + budget + workspace + risk
  policy in, `{tasks, artifacts, estimated_cost}` out), parses it, rebuilds the
  graph with fresh UUIDs + index→id dependency resolution, and runs it through the
  same deterministic `validate_task_graph` the rule planner faces.
* **`PlannerRouter`** — pure-function selection: trivial/known intent types stay
  rule; `Unknown` / complex keywords (refactor, deploy, build, implement, …) use
  the LLM when a backend is wired. Never strands a goal (falls back to rule).
* **`ReasoningBackend`** (trait in `backend.rs`) — a deliberately minimal
  `complete(prompt) -> text` so the planner has **no dependency on
  `friday-intelligence`** (which depends on `friday-planner` — a cycle would
  otherwise form). `InferenceFabric` is wired as `InferenceBackend` in the
  intelligence crate; tests use `MockBackend`.
* **`validate_task_graph`** (`planning/planners/validation.rs`) — all-at-once
  checks before the supervisor runs: dependency cycles, unknown capabilities,
  dangling dependencies, empty names, `max_steps` budget, and high-risk escalation
  against the `RiskPolicy`.

The host opts into LLM planning via `register_engines_with_llm(friday, backend)`
(or `planning_engine_with`); `register_engines` stays rule-only and deterministic.
This makes the planner a drop-in reasoning consumer — the same `InferenceFabric`
that powers planning can later back the `Reflector`, `Summarizer`, and `Explainer`.

## Persistence & resume

Every transition is appended as an immutable `GoalEvent` (redb). A compact
snapshot is written on each transition for fast `friday resume <id>`. Current
state is always reconstructible by replaying the log — giving debugging,
timeline, analytics, and deterministic crash recovery.

## Out of scope (explicitly deferred, per design)

- Background scheduling / long-running daemon goals
- Memory evolution / continuous memory writer
- Multi-agent orchestration (the worker pool is the seam where specialized agents attach)
- LLM-driven reflection (rule engine today; `Reflector` trait is the swap point)
- Per-workspace permission policy (currently `AllowAllPolicy`)
- MCP registration (architecture ready via `CapabilityRegistry`; M6.1 proves the seam)
- LLM-driven reflection (rule engine today; `Reflector` trait is the swap point — done)
- Shared execution context (M6.3 — done) and streaming output (M6.4 — done)
- LLM planner (M6.2 — done); `PlannerRouter` chooses rule vs LLM per goal
- Deterministic `TaskGraph` validation before the supervisor (M6.2 — done)

## Tests

`planner/tests/supervisor_tests.rs` covers: autonomous completion (DAG, two
tasks), high-risk pause→resume→complete, and failure→retry→complete. Run with
`cargo test -p friday-planner --test supervisor_tests`.
