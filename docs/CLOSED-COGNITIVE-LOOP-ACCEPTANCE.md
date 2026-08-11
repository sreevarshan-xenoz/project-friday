# Closed Cognitive Loop Acceptance

Campaign G is the regression campaign across the existing information, intent,
execution, verification, durability, and workforce seams. It does not introduce
another orchestration or goal abstraction.

## Target loop

```text
WorldModel → IntentLayer → Gap → Objective → GapDriven plan
          → AutonomousTick → ActionRuntime → Verification
          → explicit observation → WorldModel → next tick
```

## CC-01–CC-12 receipt

| ID | Acceptance condition | Evidence | Result |
| --- | --- | --- | --- |
| CC-01 | No gap produces no autonomous action | `closed_cognitive_loop_replans_then_stops_after_verified_completion`; `no_plan_when_gap_already_closed` | PASS |
| CC-02 | Open gap produces a `PlanSource::GapDriven` action | `intent_decision_drives_gap_action_and_stops_when_complete`; `plan_is_generated_from_a_desired_state_gap` | PASS |
| CC-03 | Action remains behind the runtime boundary | Live bridge uses `IntentDecision` → `AutonomyHost` → `KernelActionExecutor`; permission/egress suites pass separately | PASS for tested bounded path |
| CC-04 | Real/result execution is explicitly verified | `real_tick_executes_verifies_and_recovers_git_result`; `closed_cognitive_loop_replans_then_stops_after_verified_completion` | PASS |
| CC-05 | Verified result updates WorldModel through an explicit observation | `apply_verified_action_outcome`; `closed_cognitive_loop_replans_then_stops_after_verified_completion` | PASS |
| CC-06 | Second tick consumes the updated state rather than stale intent | `closed_cognitive_loop_replans_then_stops_after_verified_completion`; `two_ticks_consume_verified_world_feedback` | PASS |
| CC-07 | Progress is recalculated after WorldModel transitions | `progress_recalculates_from_current_world_state` proves `0.0 → 0.5 → 1.0` | PASS |
| CC-08 | Completion stops further pursuit | Closed-loop host test asserts second `tick()` returns `None` and no second execution event | PASS |
| CC-09 | Verification failure does not create false world completion | `verified_observation_updates_world_but_failed_outcome_does_not`; malformed observation refusal test | PASS |
| CC-10 | Journal recovery avoids duplicate dispatch | `real_tick_executes_verifies_and_recovers_git_result`; live bridge restart/recovery tests | PASS |
| CC-11 | The exact intent action is delegated through Agent Director | `DirectorActionExecutor` delegates one action to a bounded child and gates on `DirectorReport::all_verified`; `director_executor_delegates_intent_action_and_gates_child_result`; `director_executor_refuses_failed_child_result`; existing Director reassignment/security/recovery suites | PARTIAL — the adapter and parent gate pass, but the live bridge still uses the real Kernel executor directly because child results do not carry a structured observation payload |
| CC-12 | Provenance survives the loop | Live bridge tests validate event IDs, timestamps, payloads, evidence, execution, and outcome DTOs; `world.updated` is published by the host | PASS |

## Canonical deterministic acceptance

`closed_cognitive_loop_replans_then_stops_after_verified_completion` proves the
combined host path:

```text
screen evidence
  → IntentDecision
  → IntentLayer::plan
  → PlanSource::GapDriven
  → AutonomousTick
  → durable journal
  → deterministic executor
  → verifier
  → world_observation
  → WorldModel
  → world.updated
  → completed intent
  → no second action
```

The live bridge separately proves the intent composition reaches the real Action
Kernel and Git adapter. `DirectorActionExecutor` is the frozen composition seam
for inserting bounded children, but it deliberately does not infer a WorldModel
observation from a child's human summary. Permission, egress, secrets, Agent
Director, and parallel durability retain their existing dedicated receipts.

## Commands and receipt — 2026-08-11

```powershell
cargo test -p friday-goal-execution autonomy
cargo test -p friday-goal-execution intent
cargo test -p friday-bridge --test bridge -- --test-threads=1
cargo test -p friday-agent-runtime --test director
cargo test -p friday-agent-runtime --test workforce
cargo test -p friday-actions --test permission
cargo test -p friday-intelligence egress
cargo test -p friday-intelligence secrets
cargo check --workspace
git diff --check
```

Observed results:

```text
autonomy filter       PASS — 7 tests
intent filter         PASS — 16 unit tests + 1 integration test
bridge suite          PASS — 23 tests
director suite        PASS — 11 tests
workforce suite       PASS — 2 tests
permission suite      PASS — 3 tests
egress suite          PASS
secrets suite         PASS
workspace check       PASS
git diff --check       PASS
```

## Qualification boundary

Campaign G proves a bounded closed cognitive loop. It does not claim generalized
autonomy. CC-11 is partially closed: bounded delegation and parent verification
are implemented, while live intent-driven delegation still needs a structured
child-result observation contract before it can replace the direct Kernel path.
