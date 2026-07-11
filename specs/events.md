# Events Specification (locked)

The event vocabulary is the contract every subsystem speaks. It is **frozen**: do
not edit event names or payloads. New subsystems consume and produce these events;
they do not invent parallel vocabularies.

## Vocabulary

Defined in `friday-planner/src/events.rs`. All events are bus messages.

```
goal.raw            — raw user input arrives (produced by UI/ingress)
intent.created       — Intent engine classified the goal into an intent
plan.created         — Planning engine produced a TaskGraph
task.started         — a task node began execution
task.completed       — a task node finished successfully
task.failed          — a task node failed
task.retry           — a task node is being retried
exec.action          — kernel dispatches an action to a service
exec.result          — service returned an ActionResult
verification.started — Verification engine began checking a result
verification.passed  — result satisfied the goal/constraints
verification.failed  — result did not satisfy the goal/constraints
```

## Rules

- A subsystem must declare, in its spec, which events it **consumes** and which it
  **produces**.
- `exec.action` / `exec.result` are the only execution boundary events; everything
  between `plan.created` and `task.*` is planner-internal.
- `task.failed` must be followed by `task.retry` or `task.completed`/`task.failed`
  terminal; the kernel owns retry policy (`DEFAULT_MAX_RETRIES`).

## Event envelope (deferred, non-breaking)

Rather than retrofitting `execution_id` / `trace_id` onto every frozen type, future
work wraps events in:

```
EventEnvelope<T> {
    event_id: String,
    execution_id: String,
    trace_id: String,
    timestamp: DateTime<Utc>,
    payload: T,
}
```

The inner payload schema is unchanged; the envelope adds distributed tracing,
replay, and correlation. This is the preferred path over editing locked event
types, and is how the Simulation `report` (M4.7) and Context Builder (M4.10) attach
correlation without breaking the contract.
