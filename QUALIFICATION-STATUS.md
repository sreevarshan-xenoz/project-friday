# Qualification Status

This document is the current baseline for FRIDAY qualification. It separates
implementation, acceptance-tested receipts, and production qualification.

Status legend:

- `PASS` = verified against evidence in this repo
- `FAIL` = test or criterion ran and failed
- `PARTIAL` = partially implemented or only bounded-path accepted
- `BLOCKED` = not currently verifiable without an external dependency or deeper
  diagnosis
- `DEFERRED` = intentionally out of scope for the current qualification pass

Board legend: 🟢 PASS · 🟢\* PASS (mechanism; final endurance deferred) ·
🟡 PARTIAL · ⏸ deferred/paused

## Board

```text
QUALIFICATION
─────────────────────────────
Workspace / Hermetic     🟢
C2 memory mechanism       🟢
RTT mechanism             🟢*
CC-11                     🟢
World Model               🟢
LifeOS Intent             🟢
Agent Director            🟢
Parallel Durability       🟢
Frontend                  🟢

Final endurance           ⏸️
Browser runtime           🟡
```

`RTT *`: the short C2 runs strongly demonstrate the mechanism, but the final
endurance validation (protocol-identical 24 h decision run) remains deferred to
the final qualification gate, exactly as decided.

## Baseline

- Core workspace is buildable.
- Several subsystems have bounded acceptance receipts and are not claimed as
  generalized autonomy.
- The test gate is split: a **hermetic gate** (fast, deterministic,
  CI-safe) and a **live integration gate** (network/model dependent,
  explicitly slower). The hermetic gate is green; live/slow e2e suites are
  documented separately with their own run commands and latency.
- Feature expansion was frozen while the remaining qualification items were
  dispositioned. As of the qualification-blockers audit (2026-08-12) those
  blockers are resolved and feature development may resume, with the caveat that
  **final endurance qualification (24 h decision runs) remains pending**.

## Matrix

| Area | Status | Evidence |
| ---- | ------ | -------- |
| Core workspace | PASS | `cargo check --workspace` |
| Autonomous Tick | PASS | [docs/WORLD-MODEL-ACCEPTANCE.md](docs/WORLD-MODEL-ACCEPTANCE.md), [docs/RUNTIME-WORLD-FEEDBACK-ACCEPTANCE.md](docs/RUNTIME-WORLD-FEEDBACK-ACCEPTANCE.md) |
| World Model | PASS | [docs/WORLD-MODEL-ACCEPTANCE.md](docs/WORLD-MODEL-ACCEPTANCE.md) |
| LifeOS Intent | PASS | [docs/LIFEOS-INTENT-ACCEPTANCE.md](docs/LIFEOS-INTENT-ACCEPTANCE.md) |
| Closed cognitive loop | PASS | [docs/CLOSED-COGNITIVE-LOOP-ACCEPTANCE.md](docs/CLOSED-COGNITIVE-LOOP-ACCEPTANCE.md) — CC-11 closed (structured child observation, Director-delegated live path) |
| Agent Director | PASS | [docs/AGENT-DIRECTOR-ACCEPTANCE.md](docs/AGENT-DIRECTOR-ACCEPTANCE.md) |
| Persistent Workforce | PASS | [docs/AGENT-DIRECTOR-ACCEPTANCE.md](docs/AGENT-DIRECTOR-ACCEPTANCE.md), `services/agent-runtime/tests/workforce.rs` |
| Parallel Durable Execution | PASS | [docs/PARALLEL-DURABLE-ACCEPTANCE.md](docs/PARALLEL-DURABLE-ACCEPTANCE.md) |
| Browser capability | PARTIAL | `services/action-runtime/src/adapters/browser.rs` exists, but [docs/Roadmap.md](docs/Roadmap.md) still defers browser runtime to M6 |
| Frontend | PASS | `frontend` build, lint, and 15 tests passed |
| Full workspace tests (hermetic gate) | PASS | `cargo test --workspace --exclude friday-e2e --no-fail-fast` — exit 0, 117 results, 1427 passed / 0 failed / 4 ignored (2026-08-12) |
| E2E vertical slice | PASS | `cargo test -p friday-e2e --test e2e_tests` — 1 passed, 0.9s |
| E2E live_task (fallback) | PASS | `cargo test -p friday-e2e --test live_task` — native fallback fixed; 1 passed, 30.5s (2026-08-12) |
| E2E local_model_task (live) | PASS | `cargo test -p friday-e2e --test local_model_task` — 1 passed, 110.5s, provider-dependent (2026-08-12) |
| E2E skill_planning_suite (live) | SLOW | `cargo test -p friday-e2e --test skill_planning_suite` — 16 goals × 40s windows ≈ ≥10 min; not fully re-run this pass |
| Formatting | PASS | `cargo fmt --all -- --check` — clean across the workspace (2026-08-12) |
| Clippy | PASS WITH WARNINGS | `cargo clippy --workspace --all-targets --all-features` completed with existing warnings |
| Memory-growth qualification | PASS | [MECH-0002](docs/mech/MECH-0002-windowed-emission-evaluation.md) C2 implemented + implementation-validated (smoke + 64/256/512 sweep: RSS flat ~19–21 MB, bounded window); accepted as an implementation-level fix. Decision run (24 h, `< 1 MB/hr`) deferred to the final qualification gate (`Final endurance` ⏸) |
| RTT-drift qualification | PASS\* | [MECH-0002](docs/mech/MECH-0002-windowed-emission-evaluation.md) C2 bounds the O(history) emission path (wire volume scales with window, plateaus past it); sub-ms RTT / ≤0.1 ms drift in sweep. Decision run (24 h, `< 10%`) deferred to the final qualification gate (`Final endurance` ⏸) |
| Final endurance (24 h decision runs) | DEFERRED | protocol-identical VP-001 (seed 42, count 140, 5 s interval) + 2 h soak deferred to the final qualification gate, per the build→validate-continuously→endure-once workflow |
| E2E capability stub | PARTIAL | `e2e/src/runner.rs` contains a stubbed capability field; current qualification dependency not yet established |
| UIAutomation TODO | DEFERRED | `windows-host/UIAutomation/ElementFinder.cs` is a placeholder; no current acceptance receipt depends on it |
| Skill Compounding | PASS* | Verified supervisor completion now enters `SkillLearningService` through the production boot path (`register_engines_with_skills` → supervisor completion sink → `SkillLearningObserver` → `SkillLearningService`), creates a disabled learned candidate, requires explicit admission, and reselects after admission; receipt: `services/context/tests/skill_runtime.rs::supervisor_verified_execution_can_enter_skill_learning_path` and `services/context/tests/skill_learning.rs`; final endurance qualification remains deferred |

## Open qualification items

### CC-11 — RESOLVED

The exact intent action is delegated through the Agent Director: child results
carry a structured observation (`AgentResult.observation`), the director hands it
up (`DirectorReport::verified_observation`), `DirectorActionExecutor` folds the
verified child's observation into the kernel-shaped
`stdout`/`world_observation` envelope, and the live bridge
(`LiveRuntimeSource`) runs the intent action through `KernelChildDriver` +
`AgentDirector`. Evidence: `director_child_runs_exact_intent_action_and_exposes_world_observation`
and `live_source_exposes_real_autonomy_provenance`.

### Workspace test completion — RESOLVED (hermetic gate)

The >5-minute `cargo test --workspace --no-fail-fast` timeout was isolated to
`friday-e2e`: its live integration binaries wait on real LLM inference and
`run_with_timeout` always burns the full window (the bus only closes on
shutdown), so windows of 180s / 90s / 16×40s stack far past a 5-minute budget.
All 31 other workspace packages complete.

The qualification gate is now split:

- **Hermetic gate** (fast, deterministic, CI-safe):
  `cargo test --workspace --exclude friday-e2e --no-fail-fast`
  `cargo test -p friday-e2e --test e2e_tests`
  Observed 2026-08-12: exit 0, 117 test results, 1427 passed / 0 failed /
  4 ignored; vertical slice 0.9s.
- **Live integration gate** (network/model dependent, explicitly slower; run
  with `--nocapture` and a ≥10-minute budget):
  - `cargo test -p friday-e2e --test live_task -- --nocapture`
    (30s hermetic fallback; 180s when a provider is reachable)
  - `cargo test -p friday-e2e --test local_model_task -- --nocapture`
    (observed 110.5s live, 2026-08-12)
  - `cargo test -p friday-e2e --test skill_planning_suite -- --nocapture`
    (16 goals × 40s windows ≈ ≥640s live; chunk with
    `FRIDAY_SUITE_GOALS` / `FRIDAY_SUITE_OFFSET`)

`live_task` had a real fallback defect: its mock plan referenced
`mcp:git.git_add` / `mcp:git.git_commit`, but the `uvx mcp-server-git`
dependency is version-incompatible with the Rust MCP client
(`AttributeError: 'Server' object has no attribute 'list_tools'`), so the git
server never registered. The invalid capabilities failed plan validation, the
planner fell back, and tasks retried until the 180s window — a false-negative
retry burn, not a live-model wait. Fix: the fallback now executes through the
already-proven native Filesystem/Git adapters
(`filesystem.write_file` → `git.add` → `git.commit`), no external MCP runtime.
Observed 2026-08-12: README written and committed, test ok in 30.5s.

### Validation criteria

`docs/VALIDATION.md` tracks the memory-growth and RTT-drift criteria. Classified
via [MECH-0002](docs/mech/MECH-0002-windowed-emission-evaluation.md): both
residuals trace to the single O(history) full-list emission path
(Finding VP-001-1 + VP-001-2, one root cause). C2 (retention window + windowed
emission) is implemented and implementation-validated (smoke + 64/256/512
sweep, all PASS). The decision thresholds (`< 1 MB/hr`, `< 10%`) are judged
only on a protocol-identical 24 h run, **deferred to the final qualification
gate** — after C2, RTT work, CC-11, the remaining blockers, and final
integrations — per the build→validate-continuously→endure-once workflow.

### Browser status

The conservative reading is:

- browser adapter exists in code
- browser runtime is not production-qualified
- roadmap remains authoritative until a browser acceptance receipt exists

### Stubs

The current stubs are not promoted to new features.

- `e2e/src/runner.rs` capability stub: harness data-fidelity only; no
  qualification claim depends on it
  - `windows-host/UIAutomation/ElementFinder.cs`: deferred unless current
    qualification claims depend on it
  - `Experience::invalidate()` (`services/experience/src/lib.rs`): `todo!`
    scoped to Phase 5H; has no production call sites (a test documents the panic)

### Qualification blockers resolved — 2026-08-12

Consistency audit over the matrix:

- **`BLOCKED` entries:** none exist in the matrix. Nothing is blocked by an
  external dependency.
- **Remaining `FAIL`:** `Formatting` — resolved via the workspace-wide
  `cargo fmt` cleanup (now PASS).
- **Stubs:** all three dispositioned above; none blocks an architecture claim.
  - **Browser boundary:** conservative reading stands — adapter exists, runtime
    not production-qualified, roadmap defers to M6, no receipt claims it.

Conclusion: **core qualification blockers are resolved.** The closed-loop
  architecture is no longer blocked; this is not the same as claiming production
  qualification, which remains gated on final endurance (24 h decision runs) and
  the explicit deferred boundaries (browser runtime, UIAutomation,
  `Experience::invalidate`).

## Current freeze line

The feature-freeze rationale has changed. Previously: *feature expansion is
frozen because blockers exist.* Now:

> **Core qualification blockers resolved. Feature expansion may resume, but
> final endurance qualification remains pending.**

Feature development may continue; the 24 h endurance decision runs and the
explicit deferred boundaries above remain outstanding at the final qualification
gate.
