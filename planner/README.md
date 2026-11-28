# planner/

The FRIDAY **Planner** — the brain that decides *what* should happen before any
service decides *how*. Implemented as four independent bus plugins in the
`friday-planner` crate, matching the frozen design in
[`docs/Planner.md`](../docs/Planner.md) and [`docs/Execution.md`](../docs/Execution.md).

## Status

✅ **M3 complete + skill-aware planning.** Pipeline tested end-to-end;
integrates with AgentKernel, MCP adapters, capability discovery, telemetry,
and the Skill Engine. The LLM planner prompt is **skill-aware**: it receives a
**RELEVANT SKILLS** section (ranked candidate skills from `SkillRegistry::context_for()`)
joined by an **EVIDENCE REQUIRED** section from the Investigation Engine, so
planning starts from skills + evidence requirements, not raw capabilities.

## Modules

```
planner/src/
├── events.rs            Standard event vocabulary (intent/plan/task/verify)
├── intent/              Intent Engine: parser, classifier, goal
├── planning/            Planning Engine: TaskGraph (DAG), planners (rule / llm / router),
│                        plan context (skills + investigation requirements)
├── execution/           Execution Engine: executor, supervisor, capability registry,
│                        native capabilities, persistence, lifecycle
├── experience/          Experience engine integration: indexer, miner, recorder,
│                        reflection, replay, suggestion, timeline, metrics
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
  recovery policies were in `friday-recovery`, archived 2026-11-28.

## Tests

```powershell
cargo test -p friday-planner
```

Includes pipeline tests against a `MockExecutor`, MCP integration, capability
discovery, supervisor, telemetry, planning-context (skills + evidence),
experience retrieval, and regression suites under `planner/tests/`.
