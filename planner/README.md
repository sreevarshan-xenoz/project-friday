# planner/

The FRIDAY **Planner** — the brain that decides *what* should happen before any
service decides *how*. Implemented as four independent bus plugins in the
`friday-planner` crate, matching the frozen design in
[`docs/Planner.md`](../docs/Planner.md) and [`docs/Execution.md`](../docs/Execution.md).

## Status

✅ **M3 complete.** Pipeline tested end-to-end; integrates with AgentKernel,
MCP adapters, capability discovery, and telemetry paths.

## Modules

```
planner/src/
├── events.rs            Standard event vocabulary (intent/plan/task/verify)
├── intent/              Intent Engine: parser, classifier, goal
├── planning/            Planning Engine: TaskGraph (DAG), planners, scheduler
├── execution/           Execution Engine: dispatcher, executor, plugin_selector
├── verification/        Verification Engine: verifier, retry, result
└── engines.rs           Engine plugin implementations
```

## What it does

| Engine | Consumes | Produces |
| ------ | -------- | -------- |
| Intent | `goal.raw` | `intent.created` |
| Planning | `intent.created` | `plan.created` (DAG `TaskGraph`) |
| Execution | `plan.created` | `task.started` / `exec.action` → `task.completed` |
| Verification | `task.completed` / `task.failed` | `verification.*` (retry / replan) |

The Execution Engine **routes** work; the **AgentKernel** (`friday-actions`)
performs actions via adapters after permission + audit.

## Key design points

- Plans are a **DAG** (`TaskGraph`) — independent tasks can run in parallel
  (`scheduler::levels`).
- Every task has an explicit **lifecycle** (`TaskState`).
- Recovery at the planner layer is bounded retries + replan; system-level
  recovery policies live in `friday-recovery` (qualification phase).

## Tests

```powershell
cargo test -p friday-planner
```

Includes pipeline tests against a `MockExecutor`, MCP integration, capability
discovery, supervisor, telemetry, and regression suites under `planner/tests/`.
