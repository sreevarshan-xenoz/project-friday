# FRIDAY Integration Test Campaign

This is the repeatable integration campaign for distinguishing working FRIDAY
paths from documentation-only or reference-inspired designs.

## Rules

- A reference project is **not** an integration target. Test the FRIDAY capability
  that borrowed the mechanism instead.
- `PASS` means the command/test ran and its acceptance condition was observed.
- `FAIL` means the test ran and exposed a product defect or unmet behavior.
- `BLOCKED` means a required external service/device/credential is unavailable.
- Do not mark a roadmap item complete from a unit test alone when its definition
  requires a wired runtime path.
- Keep the mock frontend provider. It is a deterministic UI test fixture, not a
  claim that production telemetry is live.

## Phase 0 — baseline

Run from the repository root:

```powershell
git status
git log -10 --oneline
cargo check --workspace
cargo test --workspace
cargo clippy --workspace --all-targets --all-features
cargo fmt --all -- --check
```

Run from `frontend/`:

```powershell
npm install
npm run build
npm run lint
npm test
```

### Baseline recorded 2026-08-11

| Check | Result | Notes |
| ----- | ------ | ----- |
| Git status/history | PASS | Branch `dev`, existing uncommitted work preserved |
| `cargo check --workspace` | PASS | Workspace compiled |
| `cargo test --workspace` | FAIL | `friday-e2e::local_model_drives_skill_wired_runtime` paused on real permission approval; external `mcp-server-git` also fails because its `Server` lacks `list_tools` |
| `cargo clippy --workspace --all-targets --all-features` | PASS with warnings | Existing warnings in MCP tests, context, goal execution, mobile, and plugin runtime |
| `cargo fmt --all -- --check` | FAIL | Existing formatting drift in action-runtime, agent-runtime, sensors, skills, and goal-execution files |
| `npm install` | PASS with audit findings | 4 reported vulnerabilities: 1 moderate, 3 high; no automatic dependency mutation performed |
| `npm run build` | PASS | Vite production build succeeded |
| `npm run lint` | PASS | 0 warnings/errors |
| `npm test` | PASS | 15 tests passed |

The workspace baseline is therefore **not green**. Targeted campaign results must
not be presented as a green full-workspace baseline until the live e2e permission
case and MCP server compatibility are resolved.

## Campaign A — information

### A1 AnyDoc matrix

Integrated FRIDAY path:

```text
docs.parse → Document → evidence → KnowledgeGraph/Memory → retrieval
```

Run the existing adapter and goal-execution suites, then add fixtures for each
format when the adapter is available:

```powershell
cargo test -p friday-actions docs
cargo test -p friday-goal-execution evidence
```

Required matrix: DOCX, PDF, PPTX, XLSX, CSV, ODT, RTF, malformed input, and
unsupported extension. Acceptance requires format detection, markdown/content,
metadata where available, structured errors, and no panic.

Status: DOCX path exists; the full format matrix is **PENDING** until fixture
availability and adapter support are confirmed.

### A2 Agent-Reach / resolver

```powershell
cargo test -p friday-investigation
cargo test -p friday-goal-execution web_evidence
```

Acceptance: healthy primary, fallback on offline/degraded/execution failure, all
providers failing, policy denial, dynamic health changes, and unknown capability
must remain provider-agnostic (`web.search` / `web.fetch`).

### A3 Evidence pipeline

```powershell
cargo test -p friday-goal-execution evidence_pipeline
cargo test -p friday-goal-execution evidence
```

Acceptance: AnyDoc + Web produce normalized `Evidence[]`; each result preserves
source, title, location, retrieval time, and evidence ID; failed evidence never
becomes verified memory.

## Campaign B — trust

### B1 Concrete verification

```powershell
cargo test -p friday-goal-execution verifiers
cargo test -p friday-goal-execution --test integration verification
```

Acceptance matrix: passing command, failing assertion, timeout, malformed command,
and unverified trajectory. A failed check must remain `Failed`, never `Verified`.

### B2 Skill learning boundary

```powershell
cargo test -p friday-goal-execution --test integration skill
cargo test -p friday-skills compound
```

Acceptance: verified trajectory compounds and is reusable; unverified,
low-confidence, low-support, empty-step, empty-capability, and empty-action
trajectories are refused.

### B3 Permission, egress, secrets, audit

```powershell
cargo test -p friday-intelligence egress
cargo test -p friday-intelligence secrets
cargo test -p friday-actions kernel
```

Acceptance: default egress block, provider-specific redact/log-only behavior,
secret redaction through Debug/Display/Serialize, denied actions produce no
adapter call, and every decision leaves an audit record.

## Campaign C — workforce and durability

### C1 Agent Director

The bounded multi-agent gate is documented in
[`AGENT-DIRECTOR-ACCEPTANCE.md`](AGENT-DIRECTOR-ACCEPTANCE.md).

```powershell
cargo test -p friday-agent-runtime --test director
cargo test -p friday-agent-runtime --test workforce
cargo test -p friday-actions --test agent_director
cargo test -p friday-actions --test permission
cargo test -p friday-intelligence egress
cargo test -p friday-intelligence secrets
```

Acceptance: bounded role-typed children, verified aggregate, failed child is
visible and explicitly reassignable, persistence survives fresh workforce
instances, permissions/egress remain enforced, and an unapproved verifier
blocks completion.

### C2 Persistent workforce

```powershell
cargo test -p friday-agent-runtime workforce
cargo test -p friday-agent-runtime --test workforce
```

Acceptance: registry, task board, isolated workspace, progress/blocker state,
and task continuation survive fresh runtime instances over the same store.

### C3 Durable workflows

The canonical PD-01–PD-07 receipt is in
[`PARALLEL-DURABLE-ACCEPTANCE.md`](PARALLEL-DURABLE-ACCEPTANCE.md).

```powershell
cargo test -p friday-goal-execution workflow
cargo check --workspace
git diff --check
```

Acceptance matrix:

- **PD-01** — bounded concurrent fan-out; parent waits for every child
- **PD-02** — failed branch remains visible and fails the stage honestly
- **PD-03** — bounded branch retry preserves failed and verified attempts
- **PD-04** — crash recovery replays verified children and recovers in-flight work
- **PD-05** — stable idempotency keys prevent duplicate verified dispatch
- **PD-06** — parent success waits for every child to be `Verified`
- **PD-07** — fresh store/engine instances converge on one durable history,
  including terminal failed-branch evidence

Supporting checks also cover approval pause/resume, stale/foreign checkpoint
refusal, and exactly-once state folding.

## Campaign D — perception

### D1 Screen

```powershell
cargo test -p friday-sensors screen
cargo test -p friday-perception
```

Hardware acceptance: capture foreground window, detect focus/content changes,
handle unavailable process/capture, and publish exact workspace topics.

### D2 Audio

```powershell
cargo test -p friday-sensors audio
```

Hardware acceptance: mic available/unavailable, silence, speech, noise,
multiple turns, timestamps, and transcript-less honesty. Synthetic PCM tests
remain the deterministic CI gate.

### D3 Vision

```powershell
cargo test -p friday-sensors vision
```

Hardware acceptance: camera unavailable, baseline frame, static scene, object
motion, object entry/exit, and restart. Synthetic frame tests remain the CI gate.

## Campaign E — autonomy and console

### E1 Autonomous tick

```powershell
cargo test -p friday-cognitive
cargo test -p friday-goal-execution autonomy
```

Acceptance matrix:

- no salient evidence → no action
- permitted action → execute, verify, outcome
- denied action → no adapter call
- egress block → no provider call
- verification failure → `outcome.observed` with `verified: false`
- process restart → prepared/in-flight work is not blindly redispatched

The real kernel acceptance currently uses `git.status` against a temporary
repository and verifies real untracked-file output.

### E2 Bridge observability

```powershell
cargo test -p friday-bridge
```

Run the bridge and query:

```powershell
cargo run -p friday-bridge
curl http://127.0.0.1:18434/metrics
```

Acceptance: Prometheus output contains active/total connections, frames,
bytes, resyncs, snapshot bytes, and delta bytes.

The bridge protocol now accepts an optional `dashboard.autonomy` trace with
real workspace event ids, timestamps, and payloads, and the frontend renders it
when present. `DemoRuntimeSource` deliberately leaves it absent; a deployed
`RuntimeSource` must populate it from `GlobalWorkspace::autonomy_read_model()`.
`LiveRuntimeSource` now exercises the host-backed path in the actual bridge
process. The demo source remains intentionally non-autonomous.

### E3 Frontend console

```powershell
cd frontend
npm run build
npm run lint
npm test
```

Run both mock and runtime providers. Acceptance: connection, reconnect, frame
sequence, evidence provenance, timestamps, and failure presentation. The mock
provider remains required for deterministic screen tests.

### E4 End-to-end autonomous task

Host construction is centralized in `friday_goal_execution::AutonomyHost`:
it owns `GlobalWorkspace`, `AutonomousTick`, the real executor/verifier, and the
write-ahead journal. `LiveRuntimeSource` connects that host to the actual
`friday-bridge` executable and runtime WebSocket path:

```text
real sensor/evidence
→ AutonomousTick
→ permission
→ egress
→ AgentKernel
→ real adapter
→ real verification
→ durable journal
→ workspace outcome
→ bridge read model
→ frontend provenance view
→ reflection/memory/skill reuse
```

The automated Campaign E gate is now satisfied for the safe `git.status`
capability: it runs in the actual bridge process, survives a fresh runtime
instance, and exposes event/evidence provenance through the bridge DTO consumed
by the frontend runtime UI. Broader autonomous capabilities still require their
own acceptance cases.

### Campaign E acceptance receipt — 2026-08-11

| ID | Acceptance | Result | Evidence |
| --- | --- | --- | --- |
| AT-AUT-01 | Cognitive no-op | PASS | `cargo test -p friday-cognitive autonomy` |
| AT-AUT-02 | Real permitted action | PASS | `LiveRuntimeSource` real `git.status` test |
| AT-AUT-03 | Permission denial | PASS | `cargo test -p friday-actions --test permission` |
| AT-AUT-04 | Egress denial + secret safety | PASS | `cargo test -p friday-intelligence egress`; `... secrets` |
| AT-AUT-05 | Verification failure/outcome path | PASS | cognitive execution/outcome tests |
| AT-AUT-06 | Crash/restart recovery | PASS | two fresh live-source instances; journal length remains 1 |
| AT-AUT-07 | Live daemon → bridge provenance | PASS | bounded `friday-bridge --live-repo` process smoke + live WebSocket test |

The process smoke used a temporary Git repository and `live-autonomy-marker.txt`;
no destructive action or real credential was used. Frontend build/lint/tests
also pass, and the runtime UI consumes the same optional `dashboard.autonomy`
DTO. The full workspace baseline remains separately documented as blocked by
live MCP/model compatibility and pre-existing formatting drift.

## Campaign F — World Model and runtime feedback

The canonical WM-01–WM-09 audit is in
[`WORLD-MODEL-ACCEPTANCE.md`](WORLD-MODEL-ACCEPTANCE.md). The runtime bridge
receipt is in [`RUNTIME-WORLD-FEEDBACK-ACCEPTANCE.md`](RUNTIME-WORLD-FEEDBACK-ACCEPTANCE.md).

```powershell
cargo test -p friday-goal-execution autonomy
cargo test -p friday-goal-execution causal
cargo test -p friday-goal-execution intent
cargo test -p friday-knowledge
cargo test -p friday-bridge --test bridge -- --test-threads=1
cargo check -p friday-bridge
git diff --check
```

WM-01 through WM-08 and the bounded WM-09 runtime path are PASS. The runtime
feedback cases prove explicit verified observations, failed/malformed refusal,
stale projection refusal followed by fresh re-observation/replanning, and a
two-tick loop in which the second decision consumes the first `world.updated`
event. The live bridge attaches one `WorldModel` to `AutonomyHost`; it does not
create a second state owner. This remains a bounded deterministic acceptance,
not a claim of generalized autonomy.

## Campaign G — LifeOS intent

The canonical INT-01–INT-09 receipt is in
[`LIFEOS-INTENT-ACCEPTANCE.md`](LIFEOS-INTENT-ACCEPTANCE.md).

```powershell
cargo test -p friday-goal-execution intent
cargo check -p friday-goal-execution
git diff --check
```

INT-01–INT-09 PASS for the bounded tested path: the existing `WorldModel`
supplies current state, `DesiredState` supplies the independent ideal, `Gap`
explains unmet conditions, closed gaps produce no work, objectives are generated
before tools are selected, `PlanSource::GapDriven` routes through the existing
planner, and progress/completion are recalculated after WorldModel transitions.
`IntentDecision` is now the sole decision function for the live bridge path and
stops when the desired state is complete. This remains bounded acceptance, not a
claim of generalized autonomy.

### G2 Closed cognitive loop

The canonical CC-01–CC-12 receipt is in
[`CLOSED-COGNITIVE-LOOP-ACCEPTANCE.md`](CLOSED-COGNITIVE-LOOP-ACCEPTANCE.md).

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

The combined deterministic host path passes IntentDecision → GapDriven plan →
durable tick → verified observation → WorldModel update → completion no-op.
`DirectorActionExecutor` now provides the bounded child delegation seam and gates
on `DirectorReport::all_verified()`. CC-11 remains partial for the live path:
the bridge still uses the direct Kernel executor because child results do not yet
carry a structured observation payload. Do not claim generalized autonomy from
this campaign.

## Reference-inspired mechanisms

These are tested through FRIDAY rather than by embedding reference projects:

| Reference idea | FRIDAY-native test |
| --------------- | ------------------ |
| DeepZero / Temporal / LangGraph | `cargo test -p friday-goal-execution workflow` |
| Google ADK sequential/parallel/conditional/loop | Workflow stages, approval, retry, and merge tests |
| Prime Agent recursive delegation | Agent Director bounded-child tests |
| Multica workforce | Persistent workforce tests across instances |
| Meetily audio pipeline | `friday-sensors` VAD/turn/ASR seam tests |
| OpenMontage fan-out/merge | Durable parallel workflow stage tests |
| PR-Agent/Sonar verification | Concrete verifier and evidence-gated result tests |
| Archestra policy plane | Permission, egress, secrets, and audit tests |
| DrawDB | `docs/world-model.dbml`; visualization is not runtime truth |

## Campaign receipt

Record each run with:

- date/time and commit SHA
- exact command
- PASS/FAIL/BLOCKED
- failing test and first actionable error
- external dependencies/devices used
- evidence artifact path

A green subsystem suite does not override a failed full-workspace baseline.
