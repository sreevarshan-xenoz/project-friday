# Qualification Campaign — Gate 1–7 execution checklist

> Execution companion to [`QUALIFICATION-STATUS.md`](QUALIFICATION-STATUS.md)
> (the source of truth for `PASS` / `PARTIAL` / `BLOCKED` / `DEFERRED`) and the
> [Master Plan milestone gates](docs/MasterPlan.md#milestone-gates). This is the
> **run checklist**: each gate's scope, its verification method (commands +
> acceptance receipts), and the outcome recorded after execution. The campaign
> is a **qualification run, not a feature branch** — no feature expansion during
> the run.

**Campaign rules:**

1. Every failure is classified, never hand-waved:
   - `real regression` — code at HEAD behaves worse than the accepted baseline;
   - `environment-dependent` — depends on network, provider availability,
     hardware, or local state (Chrome, camera, mic, host sleep, quotas);
   - `harness/test-design issue` — the test itself is wrong or flaky by design
     (timing, ordering, missing env gate), not the product;
   - `pre-existing` — known/fixed-in-documentation issue that reproduces at HEAD
     and is unrelated to recent work;
   - `documentation-only` — docs/specs out of sync with code; no behavior change.
2. A gate is `PASS` only when its verification method exits green (or all
   failures are classified and accepted with an owner/date).
3. The final endurance (24 h decision runs) is **not** part of this campaign —
   it remains deferred to the last gate (`Final endurance` ⏸ in
   QUALIFICATION-STATUS.md).

## Run ledger

| Run | Date | Campaign result |
| --- | ---- | --------------- |
| R1  | 2026-08-13 | ✅ ALL GATES PASS — hermetic 1484/0 + live e2e/browser/provider green |

## Gates

### Gate 1 — Information Organ

**Scope:** AnyDoc · Web Search · Web Fetch · Evidence · Memory.
**Verification:** `cargo test -p friday-actions` (AnyDoc/Docs adapter) ·
`cargo test -p friday-goal-execution` (evidence pipeline, web resolver wiring) ·
`cargo test -p friday-planner` (resolver-backed planning tools) ·
`cargo test -p friday-core -p friday-memory` (memory layer + admission).
**Receipts:** `docs/CLOSED-COGNITIVE-LOOP-ACCEPTANCE.md` (episodic admission
gate), Master Plan #1–#6.
**Result:** PASS — hermetic evidence (R1, 2026-08-13)

### Gate 2 — Trustworthy Executor

**Scope:** Planning · Execution · Verification · Permissions · Audit · Durability.
**Verification:** `cargo test -p friday-goal-execution` (planning/execution,
`CodeTestVerifier`, workflow durability, admission) ·
`cargo test -p friday-actions` (AgentKernel, permission engine, audit, egress
boundary) · `cargo test -p friday-intelligence` (secrets vault, egress gate).
**Receipts:** `docs/CLOSED-COGNITIVE-LOOP-ACCEPTANCE.md`, `QUALIFICATION-STATUS.md`
matrix (autonomous tick, skill compounding).
**Result:** PASS — hermetic evidence (R1, 2026-08-13)

### Gate 3 — Multi-Agent FRIDAY

**Scope:** Agent Director · Subagents · Parallel workflows · Recovery.
**Verification:** `cargo test -p friday-agent-runtime` (director/workforce
clamp + inheritance) · `cargo test -p friday-actions --test agent_director`
(real-driver director) · `cargo test -p friday-goal-execution` (durable
workflow) · `cargo test -p friday-recovery`.
**Receipts:** `docs/AGENT-DIRECTOR-ACCEPTANCE.md`,
`docs/PARALLEL-DURABLE-ACCEPTANCE.md`.
**Result:** PASS — hermetic evidence (R1, 2026-08-13)

### Gate 4 — World-Aware FRIDAY

**Scope:** World Model · Intent · Gap Analysis · Causal Planning.
**Verification:** `cargo test -p friday-goal-execution` (causal model, intent
layer, world state) · `cargo test -p friday-cognitive` (workspace) ·
`cargo test -p friday-perception` (perception engine).
**Receipts:** `docs/WORLD-MODEL-ACCEPTANCE.md`,
`docs/RUNTIME-WORLD-FEEDBACK-ACCEPTANCE.md`, `docs/LIFEOS-INTENT-ACCEPTANCE.md`.
**Result:** PASS — hermetic evidence (R1, 2026-08-13)

### Gate 5 — Learning FRIDAY

**Scope:** Reflection · Verification · Skill Compounding · Persistent Procedures.
**Verification:** `cargo test -p friday-skills` (compiler, registry, admission)
· `cargo test -p friday-context` (skill learning seam) ·
`cargo test -p friday-reflection` · `cargo test -p friday-goal-execution`
(compounding end-to-end).
**Receipts:** `docs/CLOSED-COGNITIVE-LOOP-ACCEPTANCE.md`, Master Plan #11
admission-gate reconciliation.
**Result:** PASS — hermetic evidence (R1, 2026-08-13)

### Gate 6 — Perceptive FRIDAY

**Scope:** Screen · Audio · Vision.
**Verification:** `cargo test -p friday-sensors` (screen/audio/vision capture +
pure pipelines) · `cargo test -p friday-perception` (engine + sensor loop).
Real-desktop/mic/webcam polls are environment-dependent and are run tolerantly
(their tests are written to skip or degrade without hardware).
**Receipts:** Master Plan #13–#15.
**Result:** PASS — hermetic evidence (R1, 2026-08-13)

### Gate 7 — Autonomous FRIDAY

**Scope:** Observe → Reason → Decide → Act → Verify → Learn → Repeat.
**Verification:** `cargo test -p friday-cognitive` (autonomous tick, journal) ·
`cargo test -p friday-bridge` (live runtime source, autonomy provenance) ·
live e2e campaign: `cargo test -p friday-e2e --test e2e_tests` (hermetic
vertical slice), `cargo test -p friday-e2e --test live_task -- --nocapture`
(native fallback), `cargo test -p friday-e2e --test live_task -- --nocapture`
with a reachable provider, browser live tests
(`FRIDAY_BROWSER_TEST=1` + `FRIDAY_CHROME_PATH`).
**Receipts:** `docs/FRIDAY-INTEGRATION-TEST-PLAN.md` (Campaign E),
`docs/CLOSED-COGNITIVE-LOOP-ACCEPTANCE.md`.
**Result:** PASS — live gate green (R1, 2026-08-13): `e2e_tests` 1/1 (vertical
slice), `live_task` 1/1 (provider probe failed → mock fallback; degraded as
designed — env-dependent), `local_model_task` 1/1 with a **real reachable
provider** (`nvidia-nim/nvidia/nemotron-3-super-120b-a12b`, 56.5 s plan →
`git.status`/`list_directory` executed + verified), browser live 15/15
(incl. real-Chrome BROWSER_LIVE + kernel audit test).

## Supporting gates (whole-workspace, hermetic)

| Check | Command | Result |
| ----- | ------- | ------ |
| Format | `cargo fmt --all -- --check` | PASS (clean after mechanical `cargo fmt --all` of 8 pre-existing files) |
| Lint | `cargo clippy --workspace --all-targets --all-features` | PASS WITH WARNINGS (exit 0, 108 pre-existing warnings, no errors) |
| Build | `cargo check --workspace` | PASS (clean) |
| Hermetic tests | `cargo test --workspace --exclude friday-e2e --no-fail-fast` | PASS — exit 0, 121 suites, 1484 passed, 0 failed, 4 ignored (pre-existing `#[ignore]`) |

The hermetic test gate is the deterministic aggregate behind Gates 1–7; the
per-gate rows above name the packages that carry each gate's distinctive
evidence. `friday-e2e` is excluded from the hermetic gate and run under the
live gate (network/model-dependent, slower).

## Failure register

| # | Gate | Failure | Classification | Owner | Disposition |
| - | ---- | ------- | -------------- | ----- | ----------- |
| — | (all) | none — no failing tests at HEAD | — | — | — |

## R1 run notes (2026-08-13) — observations, no failures

- `live_task`: live LLM probe reported `No eligible providers for this request.` and the
  test used its deterministic mock backend (`[BOOT] Live LLM probe failed ... using mock
  backend`). Classification: `environment-dependent` (local provider config absent) and the
  fallback is the designed behavior — the full pipeline (intent → plan → write_file →
  git add/commit → verify → cleanup) still ran and passed.
- `local_model_task`: the fabric routed to a real provider
  (`nvidia-nim/nvidia/nemotron-3-super-120b-a12b`) in `Hybrid` mode and planned in ~56.5 s;
  plan = `git.status` + `filesystem.list_directory`, both executed AllowOnce and verified.
  The mock `git.commit` (`missing_string_parameter`) and gated `git.push`/`terminal.exec`
  steps are part of the deterministic mock sub-agent exercise, not product failures.
- `browser::tests`: 15/15 passed including 5 real-Chrome `BROWSER_LIVE` tests and
  `kernel_audits_browser_redirect_denial_and_cancels` (#12 sweep live test).

## R2 failure-injection register (campaign R2, baseline `3b07180`)

> R2-2 qualification ledger for deliberate failure injection against the
> Observe ? Reason ? Decide ? Act ? Verify ? Learn loop. Each record maps one
> injected failure onto the **existing** seam that detects, contains, and
> recovers from it. No recovery machinery may be added to make a scenario
> pass: a missing capability is recorded as a **GAP CANDIDATE** until a crash
> injection proves it. Baseline: R2-1 plan-grounding revision +
> stale-plan admission (commit `3b07180`, gates 1676/0).

Acceptance criterion for every record:

> FRIDAY recovers without lying about what happened � no fabricated success,
> no world-model advancement for refused or failed work, an explicit
> machine-distinguishable identity for every refusal.

| ID | Failure injected | Detection | State preserved | Recovery path | Verification result | World-model effect | Memory/lesson effect | Next-tick behavior |
| -- | ---------------- | --------- | --------------- | ------------- | ------------------- | ------------------ | -------------------- | ------------------ |
| F1 | Reasoning provider death | `run_attempt` timeout/`Err`; per-provider circuit breaker in `select_candidates`; `check_health()` probes | Immutable `RoutingDecision.failures` audit (candidates/chosen/served/failures) | Candidate failover across the ranked list; planner falls back to `RulePlanner` (`engines.rs`: "deterministic fallback so the goal is never stranded"); both failing ? `FridayError::Internal("planning failed")` | `plan_result` Failure trace span carries backend error; tick ends Failed | None fabricated � planning died before execution | None recorded (provider death is not a prediction error) � design-consistent | Fresh routing decision; breaker skips/probes dead provider |
| F2 | Worker crash mid-stage (`Workforce`) | Driver returns `Err` ? `run()` marks task honestly; hard-crash orphan detection **UNVERIFIED** | Board entry + `AgentResult` persisted atomically (temp + rename); `a_failing_run_marks_the_task_failed_and_never_done`: "the honest failure must be persisted" | `resolve_blocker` ? re-Assignable; Director reassigns failed children (`failed_child_report_can_be_reassigned_and_verified`); cross-instance survival proven (`persistent_worker_runs_a_real_docs_task_across_instances`). **Auto-resume of orphaned `InProgress`/`Failed` tasks after hard crash = GAP CANDIDATE / UNVERIFIED** | Task is never marked Done on failure; persisted status is Failed/Blocked | None fabricated for the crashed run | Trajectory/result preserved as evidence on the board entry | Depends on injection outcome � this is the open question of R2-2 |
| F3 | Verification failure | Verifier rejects execution (`VerificationStatus::Failed`, `ExecutionState::Failed` terminal) | Execution result kept as honest evidence | Orchestrator stops (`failed_verification_yields_failure_outcome_and_stops`); Director blocks child (`failed_child_blocks_the_verified_outcome`) | Goal NOT successful; refusal is explicit, not silent | `apply_verified_action_outcome` refuses unverified/failed/malformed outcomes (WF-03) � no false world advance | Mismatch feeds Experience admission (golden 8B Case A/B; lesson only after corroboration) | Next plan sees unadvanced world; may replan |
| F4 | Egress secret block | Two independent defenses: **content** `EgressGate` (Block/Redact/LogOnly per provider) and **destination** `EgressPolicy` (fail-closed scope check at kernel pre-dispatch, redirect-hop re-check, CDP interception) | Block recorded on the routing decision BEFORE the payload leaves (`providers.rs` `egress_process`); deny labels never echo destinations | Blocked request is cancelled, audited, not retried (`kernel_audits_redirect_denial_cancels_and_does_not_retry`); SSRF/internal destinations denied unconditionally | Outbound side effect count = 0 proven with hit-counting mock servers (`internal_and_private_destinations_are_denied_before_adapter_dispatch`, `internal_redirect_is_refused_before_the_second_request`, `denied_url_with_secret_leaks_nothing_into_audit_or_error`) | None � nothing executed | Rejection is audit-only by design | Caller sees the explicit egress error |
| F5 | Stale-plan rejection (world advanced past grounding) | Revision admission gate compares `plan.grounding_revision` to live world revision | Journal untouched, executor untouched | Refusal IS the correct outcome � replanning against current revision is the recovery | Explicit `DurableTickOutcome::StalePlanRejected{planned,current}` + `stale_plan_rejected` trace span + `plan.stale_rejected` workspace event | World NOT touched by the stale plan; next tick grounds at the new revision (R0?R1?R2 two-cycle proof) | No experience synthesized for refused work | Re-observe ? replan ? execute against current revision |

### Current classification

| ID | Scenario | Current classification |
| -- | -------- | ---------------------- |
| F1 | Reasoning provider death | ?? GREEN � injection needed to pin deterministic failover order |
| F2 | Worker crash mid-stage | PARTIAL - normal failure recovery GREEN; hard-crash orphan recovery RED / **PROVEN GAP** (injection `b93d43b`) |
| F3 | Verification failure | ?? GREEN |
| F4 | Egress secret block | ?? GREEN � content EgressGate and destination EgressPolicy are separate defenses, both pre-side-effect |
| F5 | Stale-plan rejection | ?? GREEN � proven by R2-1 (`3b07180`): executor calls 0, journal empty |


### F2 crash-injection evidence (2026-08-25, commit `b93d43b`)

Injected: hard termination between `Workforce::run`'s durable pre-dispatch
checkpoint (worker `Running`, task `InProgress`, persisted atomically to
`JsonWorkforceStore`) and any terminal write. No driver `Err` was injected -
this is case B+C (process disappears, store reloaded), not the honest-failure
path. A fresh `Workforce` was reconstructed over the same root.

Observed:

- state before termination: worker=Running(current_task bound), task=InProgress(assignee bound)
- persisted: both records survive restart atomically (persistence itself GREEN)
- reconstructed: fresh instance sees the orphan exactly as left
- ownership/lease/heartbeat: none exists anywhere in the Workforce surface
- pick_up(replacement): Ok(None) - orphan invisible (board scans Open only)
- assign(replacement): Err "task X is in_progress, not open"
- resolve_blocker: Err "not blocked"
- suspend / resume / retire: all refused ("running")
- post-restart state: InProgress forever - no automatic AND no manual path out

Classification: **F2 = PARTIAL**

- Normal worker failure recovery (driver Err -> honest Failed, persisted): GREEN
- Hard-crash orphan recovery: RED / **PROVEN GAP** - an InProgress task whose
  owner died is unrecoverable through any public API.

Design question for the eventual smallest repair (NOT implemented yet): how does
FRIDAY distinguish a legitimately long-running `InProgress` task from an orphaned
one? Candidate shapes (lease/heartbeat/stale-ownership sweep) are deferred until
this gap is dispositioned.

Campaign order: F1 injection ? F2 crash injection (nasty enough to create a
genuinely orphaned `InProgress` task) ? F3/F4/F5 registration verification ?
update this ledger with observed evidence ? only then decide whether F2 needs
implementation.
