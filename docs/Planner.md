# Planner Design (LOCKED)

This document is part of the frozen architecture. Changes require an explicit
review and a version bump.

> **Implementation:** `friday-planner` (`planner/`). Event names in code and
> `specs/events.md` use `intent.created` / `verification.*` (and task lifecycle
> events); some older prose below still says `intent.resolved` / `verify.*` —
> treat `specs/events.md` as the wire vocabulary. Rule-based engines ship today;
> model-driven planning plugs in via `friday-intelligence` behind the same bus
> contracts.

## Principle

Reasoning is separated from execution. The LLM decides **what** to do;
specialised executors decide **how**. The planner never generates fragile
automation logic directly.

## Four engines

The planner is not one component. It is four independent engines that communicate
purely through events. Each owns a single stage of the "think" half of the loop.

### 1. Intent Engine

- **Input:** `goal.raw` (free-text or transcribed user request).
- **Output:** `intent.resolved` (structured goal: `{ goal_id, objective,
  constraints, context_refs }`).
- **Job:** Resolve ambiguity, capture constraints, and reference relevant memory
  so downstream stages don't re-derive context.

### 2. Planning Engine

- **Input:** `intent.resolved`.
- **Output:** `plan.created` (ordered list of steps; each step names a target
  plugin/capability and parameters).
- **Job:** Goal decomposition into executable steps. Produces a task queue, not
  imperative code.

### 3. Execution Engine

- **Input:** `plan.created` (consumes one step at a time).
- **Output:** `exec.action` (chosen plugin + invocation) and `exec.result`.
- **Job:** Route each step to the correct plugin over the bus/transport, execute,
  and report the outcome. See [`Execution.md`](Execution.md).

### 4. Verification Engine

- **Input:** `exec.result`.
- **Output:** `verify.passed` or `verify.failed`.
- **Job:** Confirm the step/goal succeeded against observable state (not just
  "no error"). On failure, decide between **retry** (re-emit the step) or
  **replan** (re-emit `intent.resolved` with failure context).

## Reserved event kinds

```
goal.raw
intent.resolved
plan.created
plan.step
exec.action
exec.result
verify.passed
verify.failed
```

## Failure handling

- Every engine is idempotent on its input event where possible.
- `trace_id` is carried end-to-end so a failed goal can be replayed.
- Recovery lives in the Verification Engine, never buried inside an executor.
