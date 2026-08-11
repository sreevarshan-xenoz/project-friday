# Parallel Durable Execution Acceptance

This receipt is the canonical acceptance record for work item **#8 — Parallel
Durable Workflows + Agent Journaling**. It audits the existing
`WorkflowEngine`; it does not introduce a second workflow engine or change the
frozen `Workflow`, `WorkflowCheckpoint`, or `WorkflowStore` interfaces.

## Contract

```text
Parent workflow
  ├── bounded parallel stage
  │     ├── child A: journal → execute → verify
  │     ├── child B: journal → execute → verify
  │     └── child C: journal → execute → verify
  ├── durable checkpoint / write-ahead journal
  ├── restart recovery + idempotent replay
  └── verified map/reduce result
```

A stage is successful only when every child reaches `ExecutionState::Verified`.
Failed attempts remain in the evidence and journal trail. A verified dispatch
is never redispatched; an in-flight dispatch is explicitly recovered and
re-executed under the remaining attempt budget.

## PD-01–PD-07 receipt

| ID | Acceptance condition | Executable evidence | Result |
| --- | --- | --- | --- |
| PD-01 | Children execute concurrently within `max_parallel`, all results remain associated, and the parent waits | `parallel_stage_runs_children_with_bounded_concurrency_and_folds` | PASS |
| PD-02 | A failed branch remains visible; the stage fails without silently dropping other terminal evidence | `stage_child_failure_fails_the_workflow_at_the_stage` | PASS |
| PD-03 | A failed branch retries within its child budget and the verified retry is the final parent result | `stage_child_retries_recover_within_the_stage` | PASS |
| PD-04 | After a crash, verified children replay while in-flight children recover explicitly | `stage_crash_resume_replays_verified_children_and_recovers_in_flight_ones` | PASS |
| PD-05 | Stable journal keys prevent verified work from being dispatched twice; attempt budgets survive resume | `verified_step_replay_never_redispatches`, `crashed_single_attempt_step_fails_honestly_on_resume` | PASS |
| PD-06 | The parent crosses the stage barrier only after every child is verified | `parallel_stage_runs_children_with_bounded_concurrency_and_folds` (`stage_verified == 4`) | PASS |
| PD-07 | A fresh store/engine instance loads one coherent execution history, including failed branch evidence | `json_store_persists_and_loads_atomically`, `failed_stage_evidence_persists_for_fresh_store_instance` | PASS |

Additional durability invariants are covered by:

- `engine_with_store_persists_write_ahead_before_dispatch_and_terminal_after`
- `durable_verified_entry_always_carries_its_fold_and_cursor`
- `stage_fold_is_applied_exactly_once_after_crash_before_eager_persist`
- `foreign_checkpoint_is_rejected_not_resumed`
- `stale_cursor_into_shrunken_workflow_is_failed_not_completed`

## Commands

From the repository root:

```powershell
cargo test -p friday-goal-execution workflow
cargo check --workspace
git diff --check
```

The workflow tests are embedded in `services/goal-execution/src/workflow.rs`,
not in a separate integration-test target.

## Receipt — 2026-08-11

```text
cargo test -p friday-goal-execution workflow  PASS — 35 passed, 0 failed
cargo check --workspace                         PASS
git diff --check                                 PASS
```

The command also exercises the non-workflow unit-test target; its filtered
result is expected to report zero selected integration tests. Existing compiler
warnings and the documented full-workspace baseline defects are not attributed
to this campaign.

## Boundary

This proves durable parallel execution and convergence for the existing
workflow engine. It does not claim generalized autonomy, arbitrary swarm
coordination, or that reference projects are embedded. DeepZero/Temporal/
LangGraph inform durability patterns; OpenMontage informs bounded fan-out and
merge. FRIDAY's journal, checkpoint, evidence, and verification contracts
remain authoritative.
