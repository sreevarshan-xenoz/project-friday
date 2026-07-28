# Cognitive Runtime Specification

**Status:** 🚧 Spec-first — implement against this file.
**Depends on:** `friday-core` (event bus), all engines
**Crate:** `services/cognitive` — crate `friday-cognitive`

## 1. Purpose

FRIDAY should not be a collection of engines that react to events. It should
be a **continuously operating intelligence**. The Cognitive Runtime is the
loop that connects all engines into a coherent cycle, using the **Global
Workspace** pattern where every engine "listens" and only relevant engines
wake up:

```
OBSERVE → UNDERSTAND → PLAN → ACT → VERIFY → REFLECT → LEARN → OBSERVE...
```

Without this loop, engines fire independently and the system feels reactive.
With it, FRIDAY is always processing, always aware, always ready.

## 2. The Loop

```
     ┌──────────────────────────────────────────────┐
     │              Cognitive Runtime               │
     │                                              │
     │  OBSERVE ──→ UNDERSTAND ──→ PLAN ──→ ACT    │
     │     ▲                              │         │
     │     └── LEARN ◄── REFLECT ◄── VERIFY ◄──┘    │
     │                                              │
     │  Cycle rate: configurable (default 100ms)    │
     │  Always running. Always observing.           │
     └──────────────────────────────────────────────┘
```

### 2.1 Global Workspace Pattern

The Cognitive Runtime uses the Global Workspace pattern:

```
Perception
    │
    ▼
Workspace (shared state)
    │
    ▼
Attention (what matters right now?)
    │
    ▼
Broadcast (notify relevant engines)
    │
    ▼
Engines (only those that match wake up)
```

Every engine registers interest in specific event namespaces.
The workspace broadcasts. Only matching engines process.
This scales: 18 engines or 180, the bus handles it.

### 2.2 Cycle phases

| Phase | Duration | What happens | Engines involved |
|-------|----------|-------------|-----------------|
| **Observe** | ~10ms | Gather signals from sensors, telemetry, events | Perception, World Model |
| **Understand** | ~20ms | Interpret signals, detect patterns, update state | Knowledge, Memory, Learning |
| **Plan** | ~50ms | Decide what to do (if anything), prioritize | Reasoning, Planning, Mission |
| **Act** | variable | Execute the plan via tools | Agent Runtime, Tool Selector, Skills |
| **Verify** | ~10ms | Confirm action succeeded, detect failures | Agent Runtime, Planning |
| **Reflect** | ~10ms | Post-action analysis, lesson extraction | Reflection Engine |
| **Learn** | ~10ms | Record outcome, update knowledge and preferences | Memory, Learning, Knowledge |

Total cycle: ~100ms (configurable). The Act phase can be longer for
real-world actions (building, deploying), but the loop continues processing
other signals.

### 2.2 What the loop enables

- **Continuous observation:** FRIDAY is always aware of system state,
  even when the user isn't talking.
- **Proactive behavior:** The loop detects problems and suggests actions
  without being asked.
- **Priority arbitration:** Urgent events (service crash) break the current
  cycle and get immediate attention.
- **State coherence:** Every engine reads the same world state, updated
  every cycle.

## 3. Cycle State Machine

```
              ┌──────────┐
              │   Idle    │  No signals, nothing to do
              └────┬─────┘
                   │ signal received
              ┌────▼─────┐
              │ Observing │  Gathering input
              └────┬─────┘
                   │ input gathered
              ┌────▼──────┐
              │Understanding│  Interpreting
              └────┬──────┘
                   │ understood
              ┌────▼─────┐
              │  Planning  │  Deciding action
              └────┬─────┘
                   │ plan ready
              ┌────▼─────┐
              │  Acting    │  Executing
              └────┬─────┘
                   │ action done
              ┌────▼──────┐
              │ Verifying  │  Checking result
              └────┬──────┘
                   │ verified
              ┌────▼─────┐
              │  Learning  │  Recording outcome
              └────┬─────┘
                   │
                   ▼
              (back to Idle or Observing)
```

### 3.1 Interrupt handling

Urgent signals (service crash, security alert, user input) can interrupt
any phase:

```
Acting ──INTERRUPT──→ Observing (urgent signal)
                     → Understanding (what's happening?)
                     → Planning (what should we do?)
                     → Acting (handle the urgent thing)
                     → Resume previous act (if still relevant)
```

The runtime saves the current cycle state before handling interrupts
and resumes if the interruption doesn't invalidate the previous plan.

## 4. Cycle Coordinator

The core component that orchestrates the loop.

```rust
pub struct CognitiveRuntime {
    config: RuntimeConfig,
    state: CycleState,
    priority_queue: PriorityQueue,
    cycle_count: u64,
    metrics: EngineMetrics,
}

impl CognitiveRuntime {
    /// Start the cognitive loop. Runs until shutdown.
    pub async fn run(&self, bus: EventBus) -> Result<()>;

    /// Handle a single cycle. Called by the run loop.
    pub async fn cycle(&self, bus: &EventBus) -> Result<CycleResult>;

    /// Inject an urgent signal that breaks the current cycle.
    pub fn interrupt(&self, signal: UrgentSignal);

    /// Get current cycle state.
    pub fn state(&self) -> &CycleState;

    /// Get metrics for this engine.
    pub fn metrics(&self) -> &EngineMetrics;
}
```

### 4.1 RuntimeConfig

```rust
pub struct RuntimeConfig {
    /// Target cycle duration in milliseconds.
    pub cycle_rate_ms: u64,
    /// Maximum actions per cycle.
    pub max_actions_per_cycle: usize,
    /// Priority threshold for interrupts.
    pub interrupt_threshold: Priority,
    /// Whether to run continuously or on-demand.
    pub mode: RuntimeMode,
}

pub enum RuntimeMode {
    Continuous,  // Always running (default)
    OnDemand,    // Only runs when signals arrive
    Hybrid,      // Continuous at low rate, accelerates on signals
}
```

## 5. Priority System

Not all signals are equal. The runtime arbitrates based on priority:

```rust
pub enum Priority {
    Critical,  // Security alert, system crash → interrupt immediately
    High,      // User input, service failure → next cycle
    Normal,    // Telemetry update, task completion → queue
    Low,       // Background learning, index updates → when idle
    Idle,      // Cleanup, optimization → never interrupts
}
```

**Priority rules:**
- Critical: interrupts any phase except Act (let current action finish)
- High: interrupts Observe/Understand/Plan/Learn
- Normal: queued, processed in order
- Low: only processed when no Normal/High/Critical pending
- Idle: only processed when nothing else is pending

## 6. Integration with engines

Each engine registers with the Cognitive Runtime during startup:

```rust
runtime.register_phase(Phase::Observe, Box::new(PerceptionEngine::new()));
runtime.register_phase(Phase::Observe, Box::new(WorldModel::new()));
runtime.register_phase(Phase::Understand, Box::new(KnowledgeEngine::new()));
runtime.register_phase(Phase::Understand, Box::new(MemoryEngine::new()));
runtime.register_phase(Phase::Plan, Box::new(ReasoningEngine::new()));
runtime.register_phase(Phase::Plan, Box::new(PlanningEngine::new()));
runtime.register_phase(Phase::Act, Box::new(AgentRuntime::new()));
runtime.register_phase(Phase::Verify, Box::new(VerificationEngine::new()));
runtime.register_phase(Phase::Reflect, Box::new(ReflectionEngine::new()));
runtime.register_phase(Phase::Learn, Box::new(LearningEngine::new()));
```

Each phase handler receives the current `CycleContext` (world state,
current goal, priority queue) and returns a `PhaseResult`.

## 7. Events

| Event | Payload | Producer |
|-------|---------|----------|
| `cognitive.cycle.started` | `{ cycle_id, phase }` | CognitiveRuntime |
| `cognitive.cycle.completed` | `{ cycle_id, duration_ms, actions }` | CognitiveRuntime |
| `cognitive.phase.started` | `{ cycle_id, phase }` | CognitiveRuntime |
| `cognitive.phase.completed` | `{ cycle_id, phase, duration_ms }` | CognitiveRuntime |
| `cognitive.interrupted` | `{ cycle_id, reason, priority }` | CognitiveRuntime |
| `cognitive.priority.escalated` | `{ signal, from_phase }` | CognitiveRuntime |
| `cognitive.workspace.broadcast` | `{ topic, payload, targets }` | CognitiveRuntime |
| `cognitive.attention.focus` | `{ target, priority }` | CognitiveRuntime |

## 8. Metrics

```rust
pub struct CognitiveMetrics {
    pub total_cycles: u64,
    pub average_cycle_ms: f64,
    pub phase_distribution: HashMap<Phase, u64>,
    pub interrupt_count: u64,
    pub actions_per_cycle_avg: f64,
    pub idle_time_pct: f64,
    pub priority_escalations: u64,
}
```

## 9. Acceptance criteria

- [ ] `friday-cognitive` builds clean (clippy/fmt/tests)
- [ ] Loop runs continuously at configured cycle rate
- [ ] All six phases execute in order per cycle
- [ ] Interrupt handling: Critical signals break current cycle
- [ ] Priority queue: lower priority waits for higher
- [ ] Phase coordination: each phase receives CycleContext
- [ ] Metrics: cycle count, phase distribution, interrupt count
- [ ] Mode: Continuous, OnDemand, Hybrid all work
- [ ] Integration: existing engines register as phase handlers
- [ ] Test: loop runs 1000 cycles without drift or memory leak

## 10. What this unlocks

- **Continuous intelligence:** FRIDAY is always thinking, not just reacting
- **Proactive behavior:** detects problems and acts without being asked
- **Priority handling:** urgent things get attention immediately
- **State coherence:** every engine reads the same world state
- **Measurability:** every cycle is tracked and mappable
- **Extensibility:** new engines plug into existing phases
