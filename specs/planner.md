# Planner Specification — Four Reasoning Engines

The Planner turns a goal into an executable `TaskGraph`. It is **four engines**, not
one monolithic planner, all implemented as bus plugins in `friday-planner`.

## Crate

`friday-planner`. Engines: Intent → Planning → Execution → Verification.

## TaskGraph (`TaskGraph`, `TaskState`)

A plan is a **DAG** of tasks (`TaskState`: `Pending | Running | Completed | Failed |
Retried`). Edges encode `dependencies`. The Simulator (M4.7) and Optimizer (M4.9)
operate on this DAG; circular plans are rejected by the Simulator validator.

## Engines

1. **Intent** — classifies `goal.raw` into an `intent.created`.
2. **Planning** — expands intent into a `plan.created` (`TaskGraph`).
3. **Execution** — drives `task.*` and emits `exec.action` for the kernel.
4. **Verification** — checks `exec.result` against goal/constraints, emits
   `verification.*`.

## Consumes / produces

- Consumes: `goal.raw`
- Produces: `intent.created`, `plan.created`, `task.started`, `task.completed`,
  `task.failed`, `task.retry`, `exec.action`, `verification.*`

## Constraints

- The Planner never knows plugin internals; it selects by **capability + ranked
  provider** from Capability Intelligence.
- At M5 the rule-based Intent/Planning are replaced by model-driven engines behind
  the **same event contract** and the **same TaskGraph** output — no downstream
  change.
- The Planner is the natural home of the Simulation Engine's call site: after
  `plan.created`, the engine simulates/validates/optimizes before `exec.action`.

## See also

- `Planner.md` (locked), `Execution.md` (locked).
- `specs/simulation.md` (M4.7 consumes `plan.created`).
