# Agent Runtime Specification

**Status:** 🚧 Spec-first — implement against this file.
**Depends on:** `friday-actions` (AgentKernel), `friday-planner`, `friday-capabilities`
**Crate:** `services/agent-runtime` — crate `friday-agent-runtime`

## 1. Purpose

The Agent Runtime owns the **execution lifecycle** of actions. Not the planner.
Not the mission engine. The runtime handles retries, checkpoints, cancellation,
rollback, timeout, progress, and recovery.

This separation keeps the planner focused on *what to do* and the runtime
focused on *how to do it reliably*.

## 2. Separation of concerns

```
Planner:       "Run cargo build, then run tests, then deploy"
Agent Runtime: Executes each step with retries, checkpoints, recovery
Mission Engine: "Frontend done, starting Backend, 60% complete"
```

| Component | Owns |
|-----------|------|
| **Planner** | What steps, in what order, with what dependencies |
| **Agent Runtime** | How to execute reliably (retry, checkpoint, rollback) |
| **Mission Engine** | Multi-step decomposition, progress, milestones |

## 3. Execution Lifecycle

```
Action received
  │
  ▼
AgentRuntime::execute(action)
  │
  ├── Create checkpoint (save state before action)
  │
  ├── Execute via Tool Selector → Capability Registry
  │
  ├── Check result
  │   ├── Success → record, continue
  │   └── Failure →
  │       ├── Log failure
  │       ├── Check retry budget
  │       ├── If retries remaining → retry with backoff
  │       ├── If budget exhausted →
  │       │   ├── Try recovery (search error, apply fix)
  │       │   ├── If recovery works → retry
  │       │   └── If no recovery → escalate to Mission Engine
  │       └── Record failure → Memory
  │
  ├── Update checkpoint
  │
  └── Return result
```

## 4. Core capabilities

### 4.1 Retries

```rust
pub struct RetryPolicy {
    pub max_retries: u32,          // default: 3
    pub backoff: BackoffStrategy,  // exponential with jitter
    pub retry_on: Vec<ErrorClass>, // which errors are retryable
}

pub enum BackoffStrategy {
    Exponential { base_ms: u64, max_ms: u64 },
    Linear { step_ms: u64, max_ms: u64 },
    Fixed { delay_ms: u64 },
}
```

Retryable errors: transient failures, timeouts, rate limits.
Non-retryable: permission denied, invalid input, not found.

### 4.2 Checkpoints

After each successful step, the runtime saves:

```rust
pub struct Checkpoint {
    pub id: String,
    pub mission_id: String,
    pub step_index: usize,
    pub action: Action,
    pub result: ActionResult,
    pub world_state: WorldSnapshot,  // relevant subset of World Model
    pub timestamp: DateTime<Utc>,
}
```

Checkpoints enable:
- **Resume:** if FRIDAY restarts, continue from last checkpoint
- **Rollback:** undo to last good state
- **Debugging:** inspect what happened at each step
- **Progress:** calculate % complete from checkpoints

### 4.3 Cancellation

User can cancel at any time:

```rust
pub fn cancel(&self, execution_id: &str) -> Result<()> {
    // 1. Signal the running action to stop
    // 2. Save checkpoint at current state
    // 3. Clean up resources
    // 4. Notify Mission Engine of cancellation
    // 5. Record cancellation in Memory
}
```

### 4.4 Pause/Resume

```rust
pub fn pause(&self, execution_id: &str) -> Result<Checkpoint>;
pub fn resume(&self, checkpoint_id: &str) -> Result<()>;
```

Pause saves state. Resume restores from checkpoint and continues.
Useful for long-running missions (build + test + deploy over lunch).

### 4.5 Rollback

```rust
pub fn rollback(&self, to_checkpoint: &str) -> Result<()> {
    // 1. Restore world state from checkpoint
    // 2. Undo side effects if possible (git revert, docker stop)
    // 3. Update Mission Engine
    // 4. Record rollback in Memory
}
```

Rollback is best-effort. Some actions (sent email, deployed code) can't
be fully undone. The runtime records what was rolled back and what wasn't.

### 4.6 Timeout

```rust
pub struct TimeoutPolicy {
    pub action_timeout_ms: u64,    // per-action timeout
    pub mission_timeout_ms: u64,   // total mission timeout
    pub idle_timeout_ms: u64,      // no progress timeout
}
```

If an action exceeds its timeout, the runtime:
1. Kills the action
2. Logs the timeout
3. Checks retry budget
4. If retries remain, retries with a fresh execution
5. If budget exhausted, escalates

### 4.7 Budget tracking

```rust
pub struct ExecutionBudget {
    pub max_time_ms: u64,
    pub max_tokens: u64,
    pub max_cost: f64,
    pub spent_time_ms: u64,
    pub spent_tokens: u64,
    pub spent_cost: f64,
}
```

The runtime tracks budget consumption across all steps. If a mission
exceeds its budget, the runtime pauses and asks the user for approval
to continue.

### 4.8 Progress tracking

```rust
pub struct Progress {
    pub total_steps: usize,
    pub completed_steps: usize,
    pub failed_steps: usize,
    pub current_step: usize,
    pub percent_complete: f64,
    pub estimated_remaining_ms: u64,
}
```

Progress is reported to the Mission Engine and exposed to the UI.

## 5. Recovery Engine integration

When all retries fail, the Agent Runtime consults the Recovery Engine:

```
Action failed after 3 retries
  │
  ▼
Recovery Engine
  ├── Search error in Knowledge Engine
  │   Found: "missing ANDROID_HOME"
  ├── Generate fix: "set env ANDROID_HOME"
  ├── Apply fix via Tool Selector
  ├── Retry original action
  │   Result: ✓
  └── Record: "this error = set ANDROID_HOME" → Memory
```

The Recovery Engine uses Knowledge + Memory to find solutions to
previously seen errors. This is the self-healing loop.

## 6. Events

| Event | Payload | Producer |
|-------|---------|----------|
| `runtime.action.started` | `{ execution_id, action, step }` | AgentRuntime |
| `runtime.action.completed` | `{ execution_id, action, result, duration_ms }` | AgentRuntime |
| `runtime.action.failed` | `{ execution_id, action, error, attempt }` | AgentRuntime |
| `runtime.retry` | `{ execution_id, attempt, backoff_ms }` | AgentRuntime |
| `runtime.checkpoint` | `{ execution_id, checkpoint_id, step }` | AgentRuntime |
| `runtime.rollback` | `{ execution_id, to_checkpoint }` | AgentRuntime |
| `runtime.cancelled` | `{ execution_id, reason }` | AgentRuntime |
| `runtime.paused` | `{ execution_id, checkpoint_id }` | AgentRuntime |
| `runtime.resumed` | `{ execution_id, checkpoint_id }` | AgentRuntime |
| `runtime.timeout` | `{ execution_id, action, timeout_ms }` | AgentRuntime |
| `runtime.budget.exceeded` | `{ execution_id, budget_type }` | AgentRuntime |
| `runtime.progress` | `{ execution_id, progress }` | AgentRuntime |

## 7. Metrics

```rust
pub struct RuntimeMetrics {
    pub total_executions: u64,
    pub success_rate: f64,
    pub average_retry_count: f64,
    pub checkpoint_count: u64,
    pub rollback_count: u64,
    pub cancellation_count: u64,
    pub timeout_count: u64,
    pub average_execution_ms: f64,
    pub budget_utilization: f64,
}
```

## 8. Acceptance criteria

- [ ] `friday-agent-runtime` builds clean (clippy/fmt/tests)
- [ ] Retries with exponential backoff work correctly
- [ ] Checkpoints save and restore state
- [ ] Cancel stops execution and saves state
- [ ] Pause/Resume works across restarts
- [ ] Rollback restores to last checkpoint
- [ ] Timeout kills stuck actions
- [ ] Budget tracking prevents runaway execution
- [ ] Progress is accurately reported
- [ ] Recovery Engine integration: error → fix → retry → success
- [ ] Test: execute with 3 retries, 1 recovery, success on 4th attempt

## 9. What this unlocks

- **Reliability:** automatic retries and recovery
- **Resumability:** missions survive restarts via checkpoints
- **User control:** pause, cancel, rollback at any time
- **Safety:** budget and timeout prevent runaway execution
- **Self-healing:** error → knowledge → fix → retry
- **Observability:** every action is checkpointed and trackable

---

# Part II — Agent Spawning & Orchestration (sub-agents)

**Status:** 🚀 Implemented (crate `friday-agent-runtime`, driver `SkillAgentDriver` in `friday-actions`).

Part I above is the execution lifecycle of *actions*. Part II is the
**sub-agent orchestration** layer: worker agents run scoped, bounded missions
and report back to the planner. One architectural rule governs everything:

> **The planner is the orchestrator. The model is only the planner's reasoning
> engine — it never owns the architecture.**

The `SkillAgentDriver` plans a worker's goal through the skill engine
(scoped to the agent's allowed skills), not through an LLM — deterministic,
reproducible, and model-agnostic. Replacing Gemma with Qwen/Llama/DeepSeek
changes nothing at this layer.

## II.1 Layering

```text
Planner (orchestrator)
    │  spawn(AgentSpec, driver)
    ▼
AgentRuntime ──▶ AgentHandle (mailbox: state + result + cancel)
    │
    ▼
worker task ── driver (SkillAgentDriver: scoped skill plan → kernel) ──▶ result
    │
    ▼
await_all ──▶ merge ──▶ MergedReport ──▶ planner decides what becomes permanent
```

| Component | Owns |
|-----------|------|
| **Planner** | Decides *whether* to fan out (Decomposer evidence), spawns, merges |
| **AgentRuntime** | Lifecycle, cancellation, deadlines, budgets, bounded concurrency, mailbox |
| **AgentDriver** | What one worker does (scoped skill selection + execution) |
| **SkillAgentDriver** | Plans via skill engine restricted to `allowed_skills`; executes via AgentKernel |

## II.2 AgentSpec — constraints, never provider names

```rust
struct AgentSpec {
    id: String,
    goal: String,
    allowed_skills: Vec<String>,        // empty = all; worker physically cannot see the rest
    preferred_models: Vec<ModelCapability>, // routing constraints (Local, Coding, Vision, ...)
    memory_scope: Vec<String>,          // private-memory keys; empty = none
    time_limit_ms: u64,                 // 0 = runtime default (60 s)
    max_steps: u32,                     // step budget; 0 = unlimited
    params: Map<String, Value>,         // per-capability JSON params
    cwd: Option<PathBuf>,
}
```

`preferred_models` is a routing *constraint*, not a provider selection — the
fabric still decides which provider serves each worker, exactly like every
other request.

## II.3 Lifecycle & mailbox

```text
Pending → Running → Succeeded | Failed | Cancelled | TimedOut | BudgetExhausted
```

- **Spawner** — `spawn(spec, driver) -> AgentHandle`; bounded by a semaphore
  (`max_concurrency`, default 4). `spawn_many` starts a batch; queued workers
  wait in `Pending` until a slot frees.
- **Mailbox** — the handle exposes a watch channel for live state and a one-shot
  for the final `AgentResult`; `await_result` consumes it.
- **Hard timeout** — every worker's run is wrapped in `timeout_at(deadline)`;
  a hung driver is killed as `TimedOut`, it can never block the pool.
- **Cancellation** — `handle.cancel()` flips a shared token; the driver
  observes it via `ctx.check()` between steps.
- **Budget** — `BudgetTracker` limits steps/cost; the driver calls
  `ctx.record_step()` per capability and stops at `BudgetExhausted`.

## II.4 Scoped skill execution (SkillAgentDriver)

1. `SkillRegistry::subset(allowed_skills)` builds a registry containing only
   the allowed skills (plus their dependency closure).
2. `select_for(goal)` picks the best usable skill **within that scope** —
   deterministic, no LLM.
3. Each resolved capability of the chosen skill is gated: high-risk
   capabilities are skipped (mirroring the supervisor's gate), then the
   scoped `SkillPolicy` must auto-approve (readonly skill, safe capability) or
   be session-approved.
4. Approved capabilities execute through the same `AgentKernel.run_action`
   path as everything else (permission, timeout, retries, audit).
5. Steps are recorded as an audit trail; the worker returns `Succeeded` only
   if at least one capability executed.

**Allowed-skills isolation is structural, not advisory:** a worker scoped to
`[git]` cannot reach `docs` even if the goal is about documentation (proven by
`services/action-runtime/tests/agent_driver.rs`).

## II.5 Evidence-based scheduling

Workers are spawned because the planner has **evidence**, not because a prompt
said "think step by step":

```rust
trait Decomposer { fn decompose(&self, goal: &str, candidate_skills: usize) -> DecomposeHint; }
// DecomposeHint { estimated_subtasks, independent }
// HeuristicDecomposer: conjunctive goals / numbered items / multi-domain goals → fan-out
```

A goal that does not decompose stays a single worker.

## II.6 Private memory

Workers never write into FRIDAY's global long-term memory. The shipped
`SkillAgentDriver` gives workers **no memory access at all** (the strictest
isolation); `AgentSpec::memory_scope` is a **reserved contract** for when a
private, ephemeral in-memory memory source lands behind the `SkillMemorySource`
seam — the planner alone promotes what becomes permanent (the returned
`AgentResult` is the only channel out). Likewise `preferred_models` is a
**reserved routing constraint** (the deterministic skill driver does not
route models; the LLM-planning driver will feed it into the fabric).

## II.7 Merge

`MergedReport` folds worker results into `{ agents, succeeded, failed,
cancelled, total_steps, total_duration_ms }` plus per-worker summary lines the
planner (or a human) can read directly.

## II.8 Acceptance criteria (Part II)

- [x] `friday-agent-runtime` builds clean (clippy/fmt/tests)
- [x] Spawn → mailbox lifecycle with observable states (Pending/Running/terminal)
- [x] Cancellation stops a running worker (`Cancelled`)
- [x] Hard timeout kills a hung worker (`TimedOut`)
- [x] Budget exhaustion stops a worker at its step cap (`BudgetExhausted`)
- [x] Bounded concurrency never exceeds `max_concurrency`
- [x] Scoped selection: out-of-scope skills are unreachable
- [x] Readonly auto-approval and approval-gated execution through the real kernel
- [x] Result merge aggregates a fan-out
- [x] Decomposer gives evidence-based fan-out decisions
