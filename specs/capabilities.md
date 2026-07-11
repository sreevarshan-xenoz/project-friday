# Capabilities Specification — Capability Intelligence

The **Knowledge** subsystem: the single source of truth for *what FRIDAY can do,
who can do it, and how well*. Planner and Intelligence reason over capabilities and
ranked providers, never over plugin names.

## Crate

`services/capabilities` — crate `friday-capabilities`.

## `Capability` model (`model.rs`)

```
id, name, group, actions: Vec<String>,
permissions: Vec<Permission>, platform: Platform,
latency_ms: u64, reliability: f64, version: String,
dependencies: Vec<String>, resource_requirements: Vec<Resource>,
supports_parallel: bool, dry_run: bool,
available: bool, health: Health, cost: u64, confidence: f64
```

## `CapabilityRegistry` (`registry.rs`)

- `provider -> capabilities` map.
- `ranked_providers_for(action)` — returns providers sorted by
  `score = reliability*100 - latency*0.05 - cost` (Browser `open_url` → 98 vs
  Windows → 81; no hardcoding).
- `available(action)`, `set_health`, `set_provider_health` (used by Orchestrator).
- Filters by availability and platform.

## Health + `Monitor` (`health.rs`)

`Health` = `Healthy | Busy | Offline | Recovering | Disabled`. `HealthProbe` +
`Monitor` run periodic probes and drive a self-healing recovery hook.

## Manifest discovery (`manifest.rs`)

Services ship `manifest.json`; startup scans → validates → registers. Dynamic
capabilities (e.g. `browser.chrome.*` when Chrome installed) are data, not code.
Existing services also expose an in-code `capabilities()` function
(`adapters::all_capabilities()`).

## `CapabilityGraph` (`graph.rs`)

Capabilities grouped by `group` for LLM traversal (token savings at M5) and for
Workflow Recorder (M4.8) portability.

## `ServiceOrchestrator` (`orchestrator.rs`, M4.6.5)

Per-service lifecycle `Created → Starting → Healthy → Recovering → Disabled`.
`ServiceProbe` + `RecoveryPolicy` (`StaticRecoveryPolicy` + `RecoveryStep::Restart`).
Background `start_loop`, `Telemetry` (restarts/failures/errors/latency),
`sync_to_registry()` pushes health into the registry.

## `CostEstimator` (`cost.rs`, M4.6.5)

`CostEstimator::estimate(&[Capability]) -> PlanCost { estimated_ms, expected_success,
risk, token_cost }`. Consumed by Simulation (M4.7) and Plan Optimizer (M4.9).

## See also

- `docs/CapabilityIntelligence.md` (locked design).
- `capabilities.md` in `docs/Architecture.md` addendum.
