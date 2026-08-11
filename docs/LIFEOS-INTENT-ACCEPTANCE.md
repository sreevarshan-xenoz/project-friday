# LifeOS Intent Acceptance

This receipt audits the existing `IntentLayer`, `WorldModel`, causal planner, and
autonomy seams. It does not introduce a second goal or intent abstraction.

## Contract

```text
WorldModel current state + DesiredState ideal
                    ↓
                   Gap
                    ↓
                Objective
                    ↓
             existing Planner
                    ↓
              verified transition
                    ↓
             gap recalculation
```

A closed gap produces no objective and no plan. Intent never bypasses permission,
egress, execution, verification, or the WorldModel.

## INT-01–INT-09 receipt

| ID | Acceptance condition | Executable evidence | Result |
| --- | --- | --- | --- |
| INT-01 | Current state is obtained from the authoritative WorldModel | `IntentLayer::analyze` evaluates `WorldModel::state()` and its causal entities; `analyze_reports_open_gap_and_describe` | PASS |
| INT-02 | Desired state is represented independently of current state | `DesiredState` evaluation tests for entities, properties, files, tools, `All`, and `Any` | PASS |
| INT-03 | Current + desired produce an explicit gap and reasons | `analyze_reports_open_gap_and_describe`, `Gap::describe` | PASS |
| INT-04 | A satisfied ideal produces no actionable work | `closed_gap_yields_no_objective`, `no_plan_when_gap_already_closed` | PASS |
| INT-05 | An open gap becomes an Objective rather than a direct tool call | `objective_proposes_closing_action_from_causal_simulation` | PASS |
| INT-06 | The Objective is routed through the existing planner | `plan_is_generated_from_a_desired_state_gap`; result source is `PlanSource::GapDriven` | PASS |
| INT-07 | Progress is recalculated from updated WorldModel state | `progress_recalculates_from_current_world_state`; progress moves from `0.0` to `0.5` to `1.0` | PASS |
| INT-08 | Completion stops objective generation when the ideal is reached | `IntentLayer::is_complete`, `closed_gap_yields_no_objective`, and progress test | PASS |
| INT-09 | A full AutonomousTick directly recalculates intent after verified feedback | `IntentDecision` is the sole tick decision function; `intent_decision_drives_gap_action_and_stops_when_complete`; live `LiveRuntimeSource` uses the same adapter | PASS for the bounded tested path — gap-driven action, verified WorldModel update, and completion no-op are proven | 

## Safety boundary

The intent path remains:

```text
Intent → Plan → Action → Permission → Egress → Execution → Verification → WorldModel
```

`IntentLayer::objective` uses causal simulation to identify possible closing
actions; it does not execute them. A failed or unverified action cannot be used as
an intent completion signal.

## Commands and receipt — 2026-08-11

```powershell
cargo test -p friday-goal-execution intent
cargo test -p friday-goal-execution autonomy
cargo test -p friday-bridge --test bridge -- --test-threads=1
cargo check --workspace
git diff --check
```

Observed results:

```text
intent filter       PASS — 16 unit tests + 1 integration test
autonomy filter     PASS — 6 tests
bridge test         PASS — 23 tests
general check       PASS
git diff --check     PASS
```

## Boundary

INT-01–INT-09 are implemented on the existing WorldModel/IntentLayer/planner
seams for the bounded tested path. `IntentDecision` is the composition seam:
it consumes the existing `IntentLayer::plan` result, returns no action for a
closed gap, and is used by the live bridge. This does not claim generalized
intent-driven autonomy across arbitrary goals or tools.
