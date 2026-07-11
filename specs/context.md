# Context Builder Specification (M4.10)

**Status:** 🚧 to implement. **Spec-first:** implement against this file.

The missing architectural piece before M5. M5 must **not** be `User → LLM`. It is
`User → Context Builder → LLM`. The Context Builder assembles everything the model
needs; the **LLM never assembles context itself**.

## Crate

`services/context` — crate `friday-context`. Depends on `friday-core` (memory),
`friday-capabilities` (registry + orchestrator), `friday-planner` (project/task
context), `plugins/windows` (current windows), future browser (open tabs).

## `LLMContext`

The assembled context handed to the Intelligence Layer:

```rust
pub struct LLMContext {
    pub goal: String,
    pub memories: Vec<Memory>,
    pub capabilities: Vec<Capability>,        // available, ranked
    pub active_services: Vec<ServiceStatus>,   // with health
    pub current_windows: Vec<Window>,
    pub open_browser_tabs: Vec<Tab>,           // when browser exists
    pub current_project: ProjectInfo,
    pub execution_history: Vec<ExecutionRecord>,
    pub recent_failures: Vec<FailureRecord>,
    pub user_preferences: Preferences,
}
```

## Sources (the Context Builder decides, not the LLM)

| Field               | Source                                  |
|---------------------|-----------------------------------------|
| `memories`          | `friday-core` memory (relevance query)  |
| `capabilities`      | `CapabilityRegistry` (available + ranked)|
| `active_services`   | `ServiceOrchestrator` health/telemetry   |
| `current_windows`   | Windows bridge (`windows.list`)          |
| `open_browser_tabs` | Browser service (M6)                     |
| `current_project`   | detected from cwd / git root / recent files |
| `execution_history` | `AgentKernel` metrics + audit (memory)   |
| `recent_failures`   | `ServiceOrchestrator` telemetry + memory |
| `user_preferences`  | `friday-core` memory                      |

## `ContextBuilder`

```rust
pub trait ContextBuilder {
    async fn build(&self, goal: &str) -> LLMContext;
}
```

`build` gathers the above, prunes to a token budget, and returns a single
`LLMContext`. Selection logic (which memories, how many capabilities) is **here**,
not in the model.

## Consumes / produces

- Consumes: `goal.raw` (or the resolved `intent.created`).
- Produces: an `LLMContext` handed to the Intelligence Layer (M5). Not an event;
  it is an in-process handoff (later correlatable via `EventEnvelope`).

## Relationship to other subsystems

- Feeds **Intelligence (M5)** — the only thing M5 receives besides the goal.
- Reads **Capability Intelligence** — so the model sees ranked, available providers.
- Reads **Service Orchestrator** — so the model knows what is healthy right now.
- Unlocks **Learning (M9)** — history/failures in context let the model avoid
  repeating mistakes.

## Acceptance criteria

- [ ] `friday-context` builds clean (clippy/fmt/tests).
- [ ] `build("...")` returns an `LLMContext` populated from real registry + memory
      (mocked orchestrator/windows in tests).
- [ ] Token budget pruning keeps `LLMContext` under a configurable limit.
- [ ] A test asserts the LLM path is `User → ContextBuilder → Intelligence`, i.e.
      the Intelligence Layer receives only an `LLMContext`, never raw memory/registry
      handles.
