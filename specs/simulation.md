# Simulation Specification — Plan Simulation Engine (M4.7)

**Status:** 🚧 to implement. **Spec-first:** implement against this file.

Not "Dry Run" — a whole **subsystem** that simulates an entire execution before it
happens, like a CPU simulator for plans. Dry Run is only one feature of it. The
planner emits a `TaskGraph`; the engine virtualizes every action by asking each
capability *"what would happen?"* — with **no side effects**.

## Crate

`services/simulation` — crate `friday-simulation`. Depends on `friday-planner`
(`TaskGraph`), `friday-capabilities` (`Capability`, `CapabilityRegistry`,
`CostEstimator`), `friday-actions` (`AgentKernel` for `dry_run` mode).

## Module layout

```
simulation/
    simulator.rs     — virtual execution of a TaskGraph
    validator.rs     — pre-flight correctness checks
    optimizer.rs     — pure graph optimization
    estimator.rs     — wraps CostEstimator into a PlanCost
    approval.rs      — plan-level approval contract
    report.rs        — Prediction vs Actual vs Difference
```

## `SimulatedEffect`

The unit of virtual outcome. Each capability's `simulate` returns a list:

```rust
pub enum SimulatedEffect {
    FilesDeleted(usize),
    FilesCreated(usize),
    Commits(usize),
    TabsOpened(usize),
    WindowLaunched(String),
    ProcessSpawned(String),
    NetworkRequest { method: String, url: String },
    ShellCommand(String),
    NoOp,
    Custom(String),
}
```

## `simulate` contract (extends `services.md`)

Each `ActionAdapter` implements an additional trait so the Simulator can virtualize
it without executing:

```rust
pub trait Simulatable {
    fn simulate(&self, action: &ActionRequest) -> Vec<SimulatedEffect>;
}
```

- Providers that cannot predict effects safely set `Capability.dry_run = false` and
  the Simulator **skips** them (falls back to "unknown effect" in the report).
- `AgentKernel` already carries `ExecutionContext` hooks to no-op side effects; the
  Simulator may also run real `run_action` in `dry_run` mode where a service supports
  it, but the default path is `simulate`.

## `simulator.rs` — `Simulator`

```
Simulator::simulate(graph, registry) -> SimulationReport
```

Walks the `TaskGraph` topologically; for each task resolves the capability via the
registry (ranked provider), calls `simulate`, accumulates `SimulatedEffect`s.
Produces a `SimulationReport { effects: Vec<SimulatedEffect>, per_task: Map<task_id, Vec<SimulatedEffect>> }`.

## `validator.rs` — `Validator`

Pre-flight checks **before execution**:

- **missing permissions** — required `Capability.permissions` not granted.
- **unavailable services** — provider `Health` is `Offline`/`Disabled`/`Recovering`.
- **dependency failures** — a `TaskGraph` dependency's capability is unavailable.
- **circular plans** — DAG cycle detection (reject).
- **impossible actions** — e.g. `windows.open` when no Windows host, or action not
  in any provider's `actions`.

Returns `ValidationResult { ok: bool, issues: Vec<ValidationIssue> }`.

## `optimizer.rs` — `Optimizer`

Pure graph optimization, **no LLM**. Operates on the `TaskGraph`:

- collapse redundant `open → close → open` of the same resource into one open;
- merge repeated navigations (`Navigate` to same target);
- reorder independent tasks for `supports_parallel` concurrency;
- drop no-op / dead tasks.

Example: `Open → Nav → Close → Open → Nav` becomes `Open → Nav → Nav → Close`.
Returns an optimized `TaskGraph` (and a diff for the report).

## `estimator.rs` — `Estimator`

Wraps `CostEstimator::estimate` over the plan's capabilities into a `PlanCost`
(`estimated_ms`, `expected_success`, `risk`, `token_cost`). Feeds the `approval`
summary and the `report` prediction.

## `approval.rs` — `Approval`

The user approves **plans, not actions**. The engine renders a `PlanSummary`:

```rust
pub struct PlanSummary {
    pub actions: usize,
    pub windows: usize,
    pub browsers: usize,
    pub files_affected: usize,
    pub estimated_seconds: u64,
    pub risk: Risk,            // Low | Medium | High
}
```

Rendered as:

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

Contract: `ApprovalRequest { summary, graph }` → `ApprovalDecision::Approved |
Rejected | ApprovedWith(changes)`. New approval events are added via the
`EventEnvelope` (deferred, non-breaking — see `events.md`), not by editing frozen
types.

## `report.rs` — `Report`

After (real) execution, compare prediction to actual:

```
Predicted: 5 s
Actual:    7 s
Prediction Accuracy: 71%
```

`ExecutionReport { predicted: PlanCost, actual: ActualMetrics, accuracy: f64 }`.
Accuracy feeds Learning (M9) and lets the Plan Optimizer (M4.9) tune its weights.
Stored in memory (`specs/memory.md`).

## Acceptance criteria

- [ ] `friday-simulation` builds with `cargo clippy -D warnings` and `cargo fmt`.
- [ ] Simulator virtualizes a known plan (e.g. "Delete Downloads") into the correct
      `SimulatedEffect`s with **zero** side effects (assert no `exec.action` fired).
- [ ] Validator rejects a plan with a missing permission and a circular plan.
- [ ] Optimizer collapses the `Open→Nav→Close→Open→Nav` example to `Open→Nav→Nav→Close`.
- [ ] Estimator produces a `PlanCost` matching `CostEstimator` math.
- [ ] Approval renders a `PlanSummary` and gates execution on `ApprovalDecision`.
- [ ] Report computes prediction accuracy from simulated vs actual metrics.
- [ ] Integration test: Planner → Simulation → (approve) → Kernel executes, and a
      `report` is produced.
