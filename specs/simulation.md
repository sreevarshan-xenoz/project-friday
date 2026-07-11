# Simulation Specification — World Model + Plan Simulation (M4.7)

**Status:** 🚧 to implement. **Spec-first:** implement against this file.

M4.7 is **not** "dry run." It is a **Digital Twin of the Agent**: a World Model the
simulator runs against, plus a Plan Simulator that virtualizes an entire execution.

```
Planner → World Model → Simulation → Prediction → Optimization → Approval → Execution
```

Every simulation runs against an **immutable snapshot** of the world — never against
live objects. The simulator asks each service, via a rich `Simulatable` contract,
not just "what would happen" but *what must be true*, *what changes*, *how expensive
it is*, and *how it could be undone*.

Deterministic by design: **no learning, no ML** inside the simulator. Prediction
improves later at M9 (Optimizer) and M9/M10 (Learning) against the stored reports.

## Crate

`services/simulation` — crate `friday-simulation`. Depends on `friday-planner`
(`TaskGraph`), `friday-capabilities` (`sim` types, `CapabilityRegistry`,
`CostEstimator`). `Simulatable` lives in `friday-capabilities::sim` so adapters
(`friday-actions`, `friday-windows`) implement it without a dependency cycle.

## `sim` module (in `friday-capabilities`)

### `SimRequest`

A virtual action request (mirrors `ActionRequest` minus id/timeout):
`{ tool, action, params }`.

### `SideEffect` (side-effect graph)

Structured, nestable effects:

```
SideEffect { kind, description, target, magnitude, children: Vec<SideEffect> }
```

`Create File → +file → +disk_usage → +filesystem_change`. The graph is a tree; the
simulator flattens/accumulates to chain predictions.

### `ResourceForecast`

`{ cpu_pct, memory_mb, network_bytes, disk_bytes, gpu_pct, tokens }` with
`combine` (sum) and `peak_with` (max for memory/cpu) helpers.

### `Precondition`

`{ description, kind ("permission"|"service"|"dependency"|"state"), satisfied }`.

### `RollbackPlan`

`{ steps: Vec<String>, automatic: bool }` — how the action could be undone.

### `Prediction`

`{ confidence: 0..1, uncertainty: 0..1, assumptions: Vec<String> }`. Explicit
assumptions ("git available", "browser healthy", "disk writable") make confidence
auditable and let M9/M10 improve it.

### `WorldState` (immutable snapshot)

```rust
WorldState {
    capabilities: Vec<Capability>,
    service_health: HashMap<String, Health>,
    windows: Vec<WindowInfo>,
    sessions: Vec<String>,
    resources: ResourceForecast,     // current usage
    filesystem: FilesystemState,     // path_counts + writable
    clipboard: Option<String>,
    browser_tabs: Vec<String>,
    terminal_state: String,
    active_user: String,
    time: DateTime<Utc>,
}
```

`WorldState::from_registry(&CapabilityRegistry)` builds the capability/health
snapshot; callers set the rest (windows, browser, filesystem) from live probes.
`WorldState::apply_effects(&[SideEffect]) -> WorldState` derives the *after* state
for the Plan Diff.

### `Simulatable` (the rich contract)

```rust
pub trait Simulatable: Send + Sync {
    fn preconditions(&self, req: &SimRequest, world: &WorldState) -> Vec<Precondition>;
    fn side_effects(&self, req: &SimRequest) -> Vec<SideEffect>;
    fn estimate(&self, req: &SimRequest) -> StepEstimate;
    fn forecast(&self, req: &SimRequest) -> ResourceForecast;
    fn rollback(&self, req: &SimRequest) -> Option<RollbackPlan>;
    fn simulate(&self, req: &SimRequest, world: &WorldState) -> SimResult { /* default bundles the above */ }
}
```

`SimResult { effects, forecast, estimate, preconditions, rollback }`.

Adapters implement `Simulatable` by dispatching on `req.action`. Built-ins
(`filesystem`, `terminal`, `git`) implement it in `friday-actions::adapters::sim`;
the Windows bridge in `friday-windows::sim`. A `sim_registry()` helper returns
`HashMap<tool, Arc<dyn Simulatable>>`.

## `friday-simulation` module layout

```
simulation/
    simulator.rs     — SimulationEngine: topo-walk, simulate each task vs WorldState
    validator.rs     — pre-flight: cycle, missing permissions, unavailable services,
                       dependency failures, impossible actions
    optimizer.rs     — pure graph optimization (deterministic): prune duplicates,
                       reorder for supports_parallel, collapse redundant open/close
    estimator.rs     — aggregate StepEstimate + ResourceForecast + CostEstimator → PlanCost
    approval.rs      — PlanSummary + ApprovalRequest/ApprovalDecision (approve plans, not actions)
    report.rs        — human-readable SimulationReport
```

### `simulator.rs` — `SimulationEngine`

```
SimulationEngine::new(registry, sims: HashMap<tool, Arc<dyn Simulatable>>, world)
SimulationEngine::simulate_plan(&TaskGraph) -> SimulationReport
```

For each task in topo order: resolve `capability` → ranked providers; pick best;
get its `Simulatable`; gather `preconditions`, `simulate`, accumulate `effects`,
`forecast` (peak/combine), `StepEstimate`. Apply effects to a cloned `WorldState`
so later preconditions and the final Plan Diff see prior state.

### `validator.rs` — `Validator`

Returns `ValidationResult { ok, issues: Vec<ValidationIssue> }`:
- circular plan (DAG cycle),
- missing permissions (precondition `kind=="permission"` unsatisfied),
- unavailable service (no ranked provider; or `Health` offline/disabled),
- dependency failure (a dependency capability unavailable),
- impossible action (capability id unknown to registry).

### `optimizer.rs` — `Optimizer`

Deterministic graph rules (no LLM): merge consecutive identical `(capability,
params)`, collapse redundant `open`→`close` of the same target with nothing
depending on it in between, and reorder independent tasks to respect
`supports_parallel`. Returns `(TaskGraph, OptimizerDiff)`.

### `estimator.rs` — `Estimator`

Aggregates per-step `StepEstimate` into total latency; combines `ResourceForecast`
(peak memory/cpu, summed network/disk/tokens); wraps `CostEstimator::estimate` over
the used capabilities for `PlanCost` (success/risk/token). Feeds the report.

### `approval.rs` — `Approval`

User approves **plans, not actions**. `PlanSummary { actions, windows, browsers,
files_affected, estimated_seconds, risk }` rendered as:

```
Plan Summary
──────────────
17 actions
2 windows
1 browser
143 files
Estimated: 18 seconds
Risk: Medium
Approval Required: YES / NO
```

Contract: `ApprovalRequest { summary, world_before, world_after } →
ApprovalDecision { Approved | Rejected | ApprovedWith(Vec<Change>) }`. New approval
events attach via `EventEnvelope` (deferred) — never by editing frozen event types.

### `report.rs` — `Report`

Structured `SimulationReport` (machine-readable, `Serialize`) and a human-readable
render matching:

```
Simulation Report
──────────────
Plan Summary | Risk Analysis | Resource Forecast | Predicted Timeline
Side Effects | Approval Needed | Rollback Availability | Confidence Score
```

### Failure Tree (fallbacks)

When the preferred provider is unavailable, the engine consults a small static
`FALLBACK_MAP` (e.g. `browser.open_url → windows.open_url`) and the registry's
next-ranked provider; if nothing remains, it records an `ApprovalWarning` (the plan
can proceed only with explicit user acknowledgement). Demonstrates "Browser
unavailable → Windows OpenURL → still unavailable → approval warning."

### Confidence (`Prediction`)

`confidence = expected_success` (product of reliabilities) decayed by unsatisfied
preconditions; `assumptions` list derived from world state + preconditions;
`uncertainty = 1 - confidence + epistemic term (unknown-effect steps / total)`.

## Definition of Done

- [ ] Simulate a full DAG **without touching the real system** (assert no `exec.action`).
- [ ] Build and use an immutable `WorldState` (snapshot; `apply_effects` derives after-state).
- [ ] Validate preconditions for every action.
- [ ] Produce structured `SideEffect`s (side-effect graph).
- [ ] Estimate time, cost, and resource usage (`ResourceForecast`).
- [ ] Calculate a confidence score with **explicit assumptions**.
- [ ] Generate fallback paths for unavailable services (Failure Tree).
- [ ] Produce a human-readable simulation report.
- [ ] Return a machine-readable `Prediction`/`SimulationReport` for the Planner.

## Out of scope (deferred)

- Adaptive/ML estimation, learning — left to M4.9 (Optimizer) and M9 (Learning).
- Live `WorldState` capture from Windows/Browser (the simulator consumes a snapshot
  the caller supplies; real probes come with those subsystems).
