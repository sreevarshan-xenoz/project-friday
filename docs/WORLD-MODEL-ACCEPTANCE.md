# World Model Acceptance

This receipt audits the existing FRIDAY-native `WorldModel`, causal model, and
intent/planner seams. It does not introduce another world-model abstraction.

## Contract

```text
Observed entities + WorldState
              ↓
        causal conditions
              ↓
      pure action simulation
              ↓
       stale-safe commit
              ↓
     gap-driven planning
```

The model is a stateful prediction substrate, not a visualization database.
Simulation is counterfactual and side-effect free. A projection can only be
committed if it was derived from the model's current state.

## WM-01–WM-09 receipt

| ID | Acceptance condition | Executable evidence | Result |
| --- | --- | --- | --- |
| WM-01 | Entity identity, kind, properties, and world-state snapshots are represented | `entity_graph_lookup_by_kind_and_id`, `add_entity_is_an_upsert`, `WorldState`/`Entity` serde coverage | PASS |
| WM-02 | Actions project a state transition with before/after/diff data | `simulate_fires_edge_and_projects_state_purely`, `apply_simulation_commits_the_projected_state`, `StateTransition` integration coverage | PASS |
| WM-03 | Causal relations have typed conditions, effects, and confidence | `condition_holds_and_reports_unmet`, `all_and_any_compose`, `effects_apply_across_world_and_entities`, `causal_model_serde_round_trips` | PASS |
| WM-04 | An action predicts its consequence before execution | `simulate_fires_edge_and_projects_state_purely`, `objective_proposes_closing_action_from_causal_simulation` | PASS |
| WM-05 | Simulation does not mutate the real model or input world | `simulate_fires_edge_and_projects_state_purely`, `entity_removal_and_relations` | PASS |
| WM-06 | Verified/projected effects can be committed to the world model | `apply_simulation_commits_the_projected_state`, `applying_closing_action_closes_the_gap`, `RUNTIME-WORLD-FEEDBACK-ACCEPTANCE.md` WF-01–WF-03 | PASS — verified runtime observations are explicit, applied by `AutonomyHost`, and rejected when failed or malformed |
| WM-07 | Stale projections are rejected rather than clobbering newer state | `apply_simulation_commits_the_projected_state` stale-projection branch | PASS |
| WM-08 | Planner/intent consumes world state and causal projections | `plan_is_generated_from_a_desired_state_gap`, `intent_gap_closes_after_committing_the_proposed_action`, `missing_prerequisites_reflect_the_entity_graph` | PASS |
| WM-09 | Autonomous tick consumes and updates the World Model | `two_ticks_consume_verified_world_feedback`, live bridge acceptance, `RUNTIME-WORLD-FEEDBACK-ACCEPTANCE.md` WF-05 | PASS for the bounded tested runtime path — `AutonomyHost` owns the attached model, publishes `world.updated`, and the second tick consumes that feedback |

## Commands and receipt — 2026-08-11

From the repository root:

```powershell
cargo test -p friday-goal-execution autonomy
cargo test -p friday-goal-execution causal
cargo test -p friday-goal-execution intent
cargo test -p friday-knowledge
cargo test -p friday-bridge --test bridge -- --test-threads=1
cargo check --workspace
git diff --check
```

Observed results:

```text
autonomy filter     PASS — 5 tests
causal filter       PASS — 14 unit tests + 1 integration test
intent filter       PASS — 1 integration test
friday-knowledge   PASS — 18 tests
bridge test         PASS — 23 tests, single-threaded
cargo check         PASS
git diff --check     PASS
```

The causal filter also selects the integration test
`intent_gap_closes_after_committing_the_proposed_action`; the intent filter
selects that same integration test because the repository's test names are
shared across the package filters.

## Explicit boundary

The existing `WorldModel::apply_simulation` is intentionally not described as
proof that a real external action happened. The runtime feedback seam now maps a
verified `ActionOutcome` into an explicit world observation before updating the
model, and stale projections trigger refusal before a fresh observation/replan.

This receipt proves a bounded deterministic world-model-backed autonomous loop.
It does not claim generalized autonomy across arbitrary tools, goals, perception
streams, or long-running daemon workloads.
