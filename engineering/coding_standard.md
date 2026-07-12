# Coding Standard

Engineering discipline for PROJECT FRIDAY. This is process, not architecture
(`docs/*.md` are frozen design docs; `specs/` are the technical source of truth).
Where this file states a rule the frozen architecture already defines, the
architecture wins.

## Toolchain & gates

- Rust, target `x86_64-pc-windows-gnu`. No C compiler in the toolchain, so use
  `redb`, not `rusqlite`.
- Every change must leave CI green:
  - `cargo clippy --workspace --all-targets -- -D warnings`
  - `cargo fmt --all -- --check`
  - `cargo test --workspace`
- Run the benchmark with `cargo run -p friday-benchmark -- --save` and review the
  diff in `benchmarks/<date>.json` before committing reliability-related work.

## Core rule

No feature work without benchmark or reliability justification. A benchmark must
**describe reality**, never reward clever implementation. Prefer honest measurement
over a higher number.

## Naming conventions

- Crates are prefixed `friday-` (e.g. `friday-recovery`).
- Event kinds are dot-namespaced topics: `intent.resolved`, `plan.created`,
  `exec.result`, `verify.passed` (see `docs/Architecture.md`, Communication model).
- Enum variants `PascalCase`; functions/fields `snake_case`.

## Module boundaries

- The core contains **no intelligence**; all behaviour lives in plugins/services
  (frozen invariant).
- Modules are reachable only through the event bus or the protocol transport
  (frozen). Nothing bypasses these channels.
- A new feature must answer three questions before it is built: which event does it
  consume? which event does it produce? which plugin/service owns it?

## Dependency rules

- **No circular dependencies between crates.** Dependencies form a DAG.
- Public APIs are **small and stable**; prefer additive changes; version breaking
  changes.
- New subsystems must justify themselves with evidence (benchmarks + chaos) before
  merge — see `benchmark_policy.md` and `architecture_principles.md`.

## Error handling

- Errors are values, not exceptions; surface them, never swallow silently.
- A recovered failure is still a failure for the system — recovery must never hide
  it (see `recovery_policy.md`, `metrics.md`). Log the original error even when a
  fallback resolves the task.

## Logging & tracing

- Use structured logging (`tracing`); no `println!`/`eprintln!` in library crates.
- Every cross-module call carries a `trace_id` (on the `Event`) so a request is
  reconstructable end-to-end.
- Certified capabilities require telemetry + observability — see `certification.md`.

## Module discipline

- `friday-recovery` is now **infrastructure**: only bug fixes, performance, and
  benchmark improvements may touch it. No redesign.
- Keep unrelated work in separate commits/branches (e.g. the M5-intelligence
  work in `core/`, `e2e/`, `services/context/`, `services/intelligence/` is kept
  out of recovery commits).
- Prefer small, focused crates over large ones.

## State machines (future)

We are accumulating state machines: Provider Circuit Breaker, Recovery Lifecycle,
Service Lifecycle, Execution Lifecycle, Task Lifecycle, Qualification Lifecycle.
Once there are enough, unify the transition logic behind a tiny internal crate.
Not necessary today; do it after Beta to avoid duplicated transition logic.
