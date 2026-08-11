# Runtime World Feedback Acceptance

This receipt covers the runtime bridge from verified action outcomes back into the
single authoritative `WorldModel`. It does not introduce a second world-state
abstraction.

## Contract

```text
verified ActionOutcome
        ↓
explicit world_observation
        ↓
WorldModel::apply_verified_observation
        ↓
world.updated workspace event
        ↓
next tick consumes the updated state
```

Unverified, missing, or malformed observations are refused. The runtime does not
infer state from action names, intentions, or free-form detail text.

## WF-01–WF-05 receipt

| ID | Acceptance condition | Executable evidence | Result |
| --- | --- | --- | --- |
| WF-01 | A verified action emits an explicit observation payload | `KernelActionExecutor` emits `world_observation` for verified `git.status`; `verified_observation_updates_world_but_failed_outcome_does_not` | PASS |
| WF-02 | A verified observation updates the sole WorldModel owner | `AutonomyHost::tick` → `apply_verified_action_outcome` → `WorldModel::apply_verified_observation`; `two_ticks_consume_verified_world_feedback` | PASS |
| WF-03 | Failed or malformed outcomes do not create a successful world transition | `verified_observation_updates_world_but_failed_outcome_does_not`; `malformed_observation_is_refused_without_world_mutation` | PASS |
| WF-04 | Stale state is refused, then re-observation enables a fresh plan | `stale_runtime_projection_requires_reobservation_before_replanning` | PASS |
| WF-05 | The second autonomous tick consumes the first verified world update | `two_ticks_consume_verified_world_feedback`: first tick publishes `world.updated`, second decision cites that event and changes the authoritative model state | PASS |

## Runtime wiring

The live bridge now creates one `Arc<Mutex<WorldModel>>` for its repository
runtime and attaches it to `AutonomyHost`. The safe live `git.status` action maps
actual porcelain output into an explicit observation containing the repository
working directory, `.git`, and observed paths. The bridge continues to expose the
existing autonomy event DTOs over the WebSocket read model.

## Commands and receipt — 2026-08-11

```powershell
cargo test -p friday-goal-execution autonomy
cargo test -p friday-goal-execution causal
cargo test -p friday-goal-execution intent
cargo test -p friday-bridge --test bridge -- --test-threads=1
cargo check -p friday-bridge
git diff --check
```

Observed results:

```text
friday-goal-execution autonomy  PASS — 5 tests
friday-goal-execution causal    PASS
friday-goal-execution intent    PASS
friday-bridge bridge            PASS — 23 tests
friday-bridge check              PASS
git diff --check                 PASS
```

## Boundary

This proves a bounded, deterministic runtime feedback loop and the live bridge
path. It does not claim generalized autonomous behavior across arbitrary tools,
goals, perception devices, or long-running daemon workloads.
