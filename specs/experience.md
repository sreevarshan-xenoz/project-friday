# Experience Engine Specification — Procedural Memory (M4.8)

**Status:** ✅ Implemented. **Spec-first:** authoritative contract for FRIDAY's procedural memory.

The Experience Engine is the memory subsystem that records plan execution metadata, task outcomes, timings, retries, and resource forecasts vs. actual consumption. It acts as FRIDAY's procedural memory tier.

```
Planner/Executor ──> Experience Record ──> ExperienceEngine (redb) ──> Learning (M9)
```

## Crate

The core data structures and engine live in `friday-core::memory::experience` (`core/src/memory/experience.rs`) to prevent circular dependencies between `friday-simulation` and `friday-actions` (the kernel).

## Data Models

### `ExperienceTask`

Represents the execution telemetry of a single task in the plan:

```rust
pub struct ExperienceTask {
    pub id: Uuid,
    pub name: String,
    pub capability: String,
    pub success: bool,
    pub duration_ms: u64,
    pub retry_count: u32,
    pub error: Option<String>,
}
```

### `ExecutionExperience`

Represents the aggregated execution record of an entire plan/intent:

```rust
pub struct ExecutionExperience {
    pub experience_id: Uuid,
    pub intent: String,
    pub plan_id: Uuid,
    pub success: bool,
    pub actual_duration_ms: u64,
    pub predicted_duration_ms: u64,
    pub timing_drift: f64,
    pub tasks: Vec<ExperienceTask>,
    pub resource_forecast: serde_json::Value, // Decoupled ResourceForecast JSON
    pub resource_actual: serde_json::Value,   // Decoupled actual resources JSON
    pub timestamp: DateTime<Utc>,
}
```

## `ExperienceEngine` API

Exposes the persistence interface over an embedded `redb` database table:

```rust
pub struct ExperienceEngine {
    db: Database,
}

impl ExperienceEngine {
    /// Open the experience database file.
    pub fn open(path: &Path) -> Result<Self>;

    /// Persist a new execution experience.
    pub fn record(&self, exp: &ExecutionExperience) -> Result<()>;

    /// Retrieve an experience by its ID.
    pub fn retrieve(&self, id: Uuid) -> Result<Option<ExecutionExperience>>;

    /// List all stored experiences.
    pub fn list(&self) -> Result<Vec<ExecutionExperience>>;

    /// Query experiences by an intent search term.
    pub fn query_by_intent(&self, intent: &str) -> Result<Vec<ExecutionExperience>>;
}
```

## Alignment with Future Subsystems

- **Plan Simulation (M4.7)**: Pre-execution resource/time forecasts are fed into `resource_forecast` and `predicted_duration_ms`.
- **Agent Kernel (M4)**: During and after execution, task latencies, errors, and actual resource consumption are collected and saved into `resource_actual` and `tasks`.
- **Learning (M9)**: Periodically reads `TimingDrift` and task success/failure logs to optimize cost functions and adjust capability reliability ratings dynamically.
