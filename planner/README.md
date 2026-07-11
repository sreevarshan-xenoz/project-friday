# planner/

The FRIDAY **Planner** — the brain that decides *what* should happen before any
plugin decides *how*. Implemented as four independent bus plugins in the
`friday-planner` crate, exactly matching the frozen design in
[`docs/Planner.md`](docs/Planner.md) and [`docs/Execution.md`](docs/Execution.md).

## Modules

```
planner/src/
├── events.rs            Standard event vocabulary (intent/plan/task/verify)
├── intent/              Intent Engine: parser.rs, classifier.rs, goal.rs
├── planning/            Planning Engine: task_graph.rs (DAG), planner.rs, scheduler.rs
├── execution/           Execution Engine: dispatcher.rs, executor.rs, plugin_selector.rs
├── verification/        Verification Engine: verifier.rs, retry.rs, result.rs
└── engines.rs           IntentEngine + PlanningEngine plugin implementations
```

## What it does

- **Intent Engine** — `goal.raw` → `intent.created` (rule-based parser/classifier).
- **Planning Engine** — `intent.created` → `plan.created` (builds a DAG `TaskGraph`).
- **Execution Engine** — `plan.created` → `task.started` / `exec.action` → `task.completed`. Routes tasks to executor plugins; never executes.
- **Verification Engine** — `task.completed`/`task.failed` → `verification.*`, driving retry or replan.

## Key design points

- Plans are a **Directed Acyclic Graph** (`TaskGraph`), not a flat list — independent tasks can later run in parallel (`scheduler::levels`).
- Every task has an explicit **lifecycle** (`TaskState`): Queued → Running → Completed / Failed / Retrying.
- Recovery lives in the Verification Engine: bounded retries (`RetryPolicy`) then a single bounded replan, with `MAX_HOPS` on the bus as a backstop against loops.

## Tests

`planner/tests/pipeline.rs` runs the full pipeline end-to-end against a
`MockExecutor` plugin (no real M4 plugins needed), covering both the happy path
and the retry-on-failure path.
