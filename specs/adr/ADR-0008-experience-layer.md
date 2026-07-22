# ADR-0008: Experience Layer — read-mostly architecture for execution history

- **Status:** Accepted
- **Date:** 2026-07-22
- **Deciders:** FRIDAY architecture (M11)
- **Supersedes:** — (no prior experience/history subsystem)
- **Superseded by:** —

## Context

FRIDAY's execution stack (PR4–M10) now produces a rich stream of facts:
goals started/completed/failed, plans created, actions executed, artifacts
written, git commits made. Today these facts live only in the event bus — they
are consumed inline and then gone. There is no durable history, no way to learn
from past executions, and no mechanism to replay successful workflows.

The opportunity: build an **Experience Layer** that observes execution facts,
stores them immutably, and derives reusable knowledge — without coupling back
to the execution path.

## Decision

**Build the Experience Layer as a read-mostly architecture with four invariants:**

### Invariant 1: Adapters annotate, kernel emits

```
Adapters never publish events.
Adapters annotate ActionResult.metadata.
AgentKernel owns all event emission.
```

This is inherited from M9.3/M10.2 and must not be violated.

### Invariant 2: Experience never changes execution

```
Execution → Events → Timeline → Recorder → Reflection
```

Nothing in the Experience Layer is allowed to mutate execution while it is
happening. Experience observes facts; execution produces facts. Never reversed.

### Invariant 3: Timeline is immutable

Append only. No updates, no deletes. The timeline is a durable, ordered log of
everything that happened during an execution:

```
GoalStarted → PlanCreated → ActionExecuted → ArtifactCreated
→ GitCommitCreated → GoalCompleted
```

Reflection can later derive summaries, but the underlying history never changes.

### Invariant 4: Experience stores semantics

Actions are recorded as semantic capability identifiers:

```
filesystem.write
git.commit
git.diff
```

Never raw UI events (mouse clicks, key presses, window opens). This keeps
workflows resilient to implementation changes.

### Invariant 5: Reflection never edits history

Reflection produces `ReflectionRecord` entries. It does not rewrite timeline
entries.

```
Timeline Entry → Reflection → Insight
```

not

```
Timeline Entry → Reflection → Timeline modified
```

## What the Experience Layer is NOT

- Not an execution engine.
- Not a scheduler.
- Not a UI macro recorder.
- Not a second planner.
- Not allowed to emit execution events.
- Not allowed to mutate timeline history.

## Architecture

```
┌─────────────────────────────────────────────────┐
│                  Experience Layer                │
│                                                  │
│  ┌──────────┐  ┌──────────┐  ┌───────────────┐  │
│  │ Timeline │  │ Recorder │  │  Reflection   │  │
│  │ (append) │  │ (derive) │  │  (derive)     │  │
│  └────┬─────┘  └────┬─────┘  └──────┬────────┘  │
│       │              │               │            │
│       └──────────────┴───────────────┘            │
│                      │                            │
│              Read-mostly store                    │
└──────────────────┬───────────────────────────────┘
                   │
          Events from AgentKernel
```

### Timeline (M11.1)

Append-only log of execution facts. Every execution step produces a
`TimelineEntry`:

```rust
TimelineEntry {
    id: Uuid,
    timestamp: DateTime<Utc>,
    goal_id: Uuid,
    plan_id: Uuid,
    execution_id: Uuid,
    event_kind: String,       // "exec.action", "exec.result", "artifact.created", etc.
    capability: String,       // "filesystem.write", "git.commit", etc.
    metadata: Value,          // event payload
    parent: Option<Uuid>,     // links to preceding entry in same execution
}
```

### Recorder (M11.2)

Watches the timeline. When a goal completes successfully, extracts the semantic
action sequence and persists it as a `Workflow`:

```rust
Workflow {
    id: Uuid,
    name: String,
    actions: Vec<String>,     // ["filesystem.write", "git.commit"]
    capabilities: Vec<String>,
    created_at: DateTime<Utc>,
    source_goal_id: Uuid,
}
```

### Reflection (M11.3)

Enriched reflection that consumes timeline entries instead of execution
internals. Produces `ReflectionRecord` entries:

```rust
ReflectionRecord {
    id: Uuid,
    timeline_entry_id: Uuid,
    insight: String,          // "succeeded because…", "failed because…"
    category: ReflectionCategory,
    timestamp: DateTime<Utc>,
}
```

### Replay (M12, deferred)

Replays stored workflows by handing them to the planner as pre-structured
action sequences with applicability checks.

## Consequences

**Positive**

- Timeline is trivially testable: append, query, done.
- Recorder and Reflection are pure derivations — they read timeline, produce
  output, never touch execution.
- Pattern mining (M12) becomes a query over timeline entries.
- Debugging is dramatically easier: immutable history means you can always
  reconstruct exactly what happened.
- The Experience Layer can be developed, tested, and deployed independently
  of the execution stack.

**Negative / trade-offs**

- Append-only storage grows without bound; a compaction/retention policy is
  needed eventually (M12+).
- Reflection insights are derived at read time (or on a background pass),
  not inline. This is intentional but means there is a lag between execution
  and insight availability.

## Alternatives considered

- **Inline recording in the supervisor.** Rejected: couples experience to
  execution and violates "experience never changes execution."
- **Event sourcing replay.** Rejected for M11: too complex. The timeline is
  a simplified, semantic version of event sourcing without the replay machinery.
- **Direct timeline mutation by reflection.** Rejected: violates immutability
  invariant and makes debugging unreliable.

## Follow-ups (deferred to M12+)

- Pattern mining over timeline entries.
- Workflow suggestions ("this sequence appears frequently — save it?").
- Replay with applicability checks.
- Timeline compaction / retention policy.
- Experience retrieval (semantic search over past executions).
