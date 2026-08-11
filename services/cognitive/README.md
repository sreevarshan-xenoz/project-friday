# FRIDAY Cognitive Runtime

`friday-cognitive` provides the small, dependency-light cognitive loop built
around a shared `GlobalWorkspace`. It can run independently of the planner,
memory, tools, and AI providers.

## Runtime phases

`CognitiveRuntime` schedules the registered phase handlers through:

```text
Observe → Understand → Plan → Act → Verify → Reflect → Learn
```

The runtime itself does not invent intelligence. Handlers subscribe to phases,
read workspace events, and publish structured results.

## Autonomous cognitive tick

`AutonomousTick` is the first implementation slice of the autonomous drive. It
can run without a user goal and is available either as a standalone loop or as
a handler on the cognitive runtime's `Act` phase.

The default `SalienceDecision` watches these workspace inputs:

| Input | Evidence requirement | Action emitted |
| ----- | -------------------- | -------------- |
| `sensor.audio.turn` | Non-empty `transcript` | `respond.to_user` |
| `sensor.vision.objects` | `count > 0` | `vision.inspect` |
| `sensor.screen` | `change == "content"` | `screen.observe` |
| `sensor.window.focus_changed` | Focus change that is not content | `screen.attend` |

Each tick evaluates the newest undecided evidence and emits **at most one**
`TickAction`. A decision contains the action, reason, priority, and the UUIDs
of the workspace events that justify it. Decisions are published as
`Topic::ActionDecided` / `autonomy.action` for auditing and downstream
consumers.

The default decision function is deterministic and does not call an LLM. Empty
or non-salient input stays quiet, transcript-less audio does not become a
fabricated user request, and already-considered events are not fired again.
`DecisionFunction` is the pluggable seam for a richer policy later.

The next continuation is now available through `execute_and_verify`: inject an
`ActionExecutor` and `ActionVerifier` to dispatch the decided action, observe its
result, and publish `tool.executed` plus `outcome.observed` with the original
evidence trace. The cognitive crate stays independent of action-runtime. The production
`KernelActionExecutor` lives in `services/goal-execution/src/autonomy.rs` and
connects this seam to the kernel's permission, timeout, retry, audit, and
lifecycle boundary. `JsonActionJournal` provides write-ahead persistence:
prepared records survive restart, in-flight actions are not redispatched, and
completed outcomes are recovered. `GlobalWorkspace::autonomy_read_model()`
provides a read-only join of the three real events for observers such as the
console bridge. The deployed host boundary is
`friday_goal_execution::AutonomyHost`, which owns the workspace, injected kernel
executor/verifier, and journal; the cognitive crate does not construct those
privileged dependencies implicitly.

## Test

From the repository root:

```powershell
cargo test -p friday-cognitive
```

The unit and async tests cover evidence gating, salience ordering, one-action
per-tick behavior, deduplication, workspace publication, and shutdown of the
standalone loop.
