# Master Plan — the corrected build order (tracker)

> This is the **operational tracker** for FRIDAY's build order. The audit trail
> (what exists, with `file:line` evidence, and the P0–P5 engineering tiers)
> lives in
> [`specs/architecture/friday-architecture-vnext.md`](../specs/architecture/friday-architecture-vnext.md).
> This doc re-sequences those tiers into the order we actually build and records
> **why**. The same table (condensed) is mirrored in the repo
> [README](../README.md#master-plan--the-corrected-build-order).
>
> **Update this table every time a work item lands** — see
> [Update discipline](#update-discipline).

**Status legend:** 🟢 DONE (code + tests exist and are wired) · 🟡 partial /
framework exists but key paths not wired · 🔴 not started · **NEXT** = current
work.

## Master order

| # | Work | Status | Definition of done |
| - | ---- | ------ | ------------------ |
| 1 | AnyDoc `docs.parse` | 🟢 DONE | Docs adapter parses real documents; constitution P0 row DONE (2026-08-10) |
| 2 | Agent-Reach resolver | 🟢 DONE | `BindingResolver` with health-gated fallback + structured `ResolvedOutcome` — `c337e7e`, `820ec5d` |
| 3 | `web.search` resolver wiring | 🟢 DONE | `ResolvingWebTool`: resolver → Action Runtime → normalized `Evidence[]`; healthy + fallback acceptance tests; constitution 🟢 — `318127a` |
| 4 | `web.fetch` resolver wiring | 🟢 DONE | Resolver-backed fetch tool (`ResolvingWebTool::for_web`/`for_web_fetch`) with fallback tests + normalized fetch evidence; Web resolver row fully 🟢 in the constitution |
| 5 | Real local Code/Test Verifier | 🟢 DONE | `CodeTestVerifier`: workspace + command → real execution → stdout/stderr/exit code → `checks[]` → `VerifiedResult` (`services/goal-execution/src/verifiers.rs`). First concrete verifier, **not** Sonar — plus `OutputContains`/`OutputExcludes`/`ToolSuccess` sub-verifiers; 10 tests green |
| 6 | Evidence pipeline hardening | 🟢 DONE | AnyDoc + Web → unified `Evidence[]` → Verification → Memory. `EvidenceSet` blends both organs (dedupe + cross-channel corroboration), `EvidenceVerifier` gates the body behind the frozen `Verifier` seam, the executive publishes the verified body and consolidates it through the admission gate (`services/goal-execution/src/evidence_pipeline.rs`) |
| 7 | Agent Director / bounded subagents | 🟢 DONE | One parent safely creates bounded children and collects **verified** results. `AgentDirector` over `AgentRuntime`: role-typed children (Researcher/Analyst/Verifier), `DirectorConfig` bounds (pool/deadline/steps), `verify()` gates every result (`default_gate`: state + summary + verifier-ran-a-step) — goal completes only when `DirectorReport::all_verified`. Wired through the real `SkillAgentDriver` (acceptance: `services/action-runtime/tests/agent_director.rs`). No persistent workforce yet (that's #16) |
| 8 | Parallel durable workflows + agent journaling | 🟢 DONE | `WorkflowStep::stage` (bounded-concurrency map/reduce, folded exactly once), write-ahead in-flight journaling with stable FNV-1a idempotency keys (`journal_key`), idempotent replay (verified steps never re-dispatched), mid-agent recovery (`recovered` counter), engine-owned persistence (`with_store`) — `services/goal-execution/src/workflow.rs` |
| 9 | Causal World Model | 🟢 DONE | Entity graph + causal edges + typed effects + pure deterministic simulation (`CausalModel::simulate` — "if action A runs, S→S'"): `services/goal-execution/src/causal.rs`. `WorldModel::simulate`/`apply_simulation` close the predict→commit loop (stale projections refused); `find_missing_capability_prerequisites` reflects the entity graph |
| 10 | LifeOS current→ideal/gap layer | 🟢 DONE | `DesiredState` (declarative ideal over entities/files/tools with All/Any algebra) evaluated against world state + causal entity graph → `Gap` (unmet conditions) → `Objective` whose closing actions come from **causal simulation** (an action closes the gap only when its projection satisfies the ideal — never execution) → `IntentLayer::plan` feeds the objective into the planner producing `PlanSource::GapDriven` plans — `services/goal-execution/src/intent.rs`. Honest: a closed gap yields no objective/plan |
| 11 | Skill compounding | 🟢 DONE | `SkillCompiler` (`services/skills/src/compound.rs`): neutral `TrajectorySkill` input (goal, Verified outcome + evidence ids, steps, confidence, support) → refusal gate that IS the verification step (Unverified / below-bar confidence / below-bar support / no steps / no capabilities / no actions all refuse with structured `RefusalCode`s) → `SkillSource::Learned` skill registered with collision-safe unique slug id, resolved capabilities, `skill.compounded` event carrying the full evidence trail → a later goal's `select_for` finds and chooses it. End-to-end proof in goal-execution integration using the real `CodeTestVerifier` |
| 12 | Egress policy + Secrets vault | 🟢 DONE | `SecretsVault` (`services/intelligence/src/secrets.rs`): scope-keyed store whose `Secret` values are structurally unobservable (Debug/Display/Serialize redact; only `reveal()` exposes), `scoped()` capability handles, `seed_from_env` for provider keys. `EgressGate` (`services/intelligence/src/egress.rs`): scans outbound prompts for registered secrets and applies per-provider Block/Redact/LogOnly (default Block); wired into `InferenceFabric` at both dispatch sites before any provider call; a Block is recorded on the routing decision (auditable "which provider receives it?"). Off by default — enabling is one line |
| 13 | Screen perception | 🟢 DONE | `WindowsScreenSensor` (`services/sensors/src/screen.rs`): real foreground-window capture (title/pid/process/bounds/state) + **continuous screen diff** (FNV-1a of client-area pixels via BitBlt+GetDIBits) → pure `Focus`/`Content` change events → `Topic::Screen` → `GlobalWorkspace` via `SensorLoop::poll_all_with_topics`. First poll emits the initial state; capture failure degrades honestly to `content_hash: None`. `PerceptionEngine::with_sensors` polls the real loop (reports the exact channels that fired) with the synthetic RNG fallback preserved |
| 14 | Audio perception | 🟢 DONE | `AudioSensor` (`services/sensors/src/audio.rs`): WASAPI shared-mode mic capture (preferred 16 kHz mono PCM; `GetMixFormat` fallback with SubFormat GUID check) → pure energy VAD (hangover extends speech, never creates it) → pure turn detector (speech run closed by silence) → optional `SpeechToText` seam → `Topic::TurnDetected` events. Honest: `transcript: null`/`asr: null` until a real ASR backend is attached; no device → no events. Full pipeline tested end-to-end over synthetic PCM + tolerant real-mic poll |
| 15 | Vision perception | 🟢 DONE | `CameraSensor` (`services/sensors/src/vision.rs`): Media Foundation webcam capture (RGB32 negotiated; dimensions from `MF_MT_FRAME_SIZE`, never guessed) → pure `frame_diff` (BT.601 luma) → `motion_score` → `changed_regions` connected-component blob detection → optional `ObjectDetector` ML seam → `Topic::ObjectDetected` events. Honest: first frame is a baseline (one frame sees nothing), static scenes emit nothing, `detector: "diff"` names the built-in. Full pipeline tested over synthetic frames |
| 16 | Persistent Agent Workforce | 🔴 **NEXT** | Assignable task board, persistent per-agent workspaces — requires Agent Director (#7) first |
| 17 | Autonomous Cognitive Tick | 🔴 | Observe → Evaluate → Decide → Act → Verify → Update State, looped. **Last** — needs perception + world model + intent + planning + director + execution + verification + security. Never a blind LLM heartbeat |
| 18 | Observability (OTel/Prometheus) | 🔴 | `/metrics` endpoint consumed by Prometheus |
| 19 | Cost telemetry | 🔴 | Per-model cost accounting ledger, queryable per goal |
| 20 | Walrus Memory backend | optional | Fifth backend behind the existing `MemoryLayer` abstraction — not a missing architectural capability |
| 21 | World-model visualization / DrawDB | last | Sugar; only after the causal world model (#9) is real |

## Milestone gates

1. 🟢 Gate 1 — **Information Organ**: AnyDoc · Web Search · Web Fetch · Evidence ·
   Memory — ✅ **complete (2026-08-10)**.
2. 🟢 Gate 2 — **Trustworthy Executor**: Planning · Execution · Verification ·
   Permissions · Audit · Durability — *the next serious milestone*.
3. 🟢 Gate 3 — **Multi-Agent FRIDAY**: Agent Director · Subagents · Parallel
   workflows · Recovery.
4. 🟢 Gate 4 — **World-Aware FRIDAY**: World Model · Intent · Gap Analysis ·
   Causal Planning.
5. 🟢 Gate 5 — **Learning FRIDAY**: Reflection · Verification · Skill Compounding ·
   Persistent Procedures.
6. 🟢 Gate 6 — **Perceptive FRIDAY**: Screen · Audio · Vision.
7. 🟢 Gate 7 — **Autonomous FRIDAY**: Observe → Reason → Decide → Act → Verify →
   Learn → Repeat.

**Current position: Gate 1 complete; #5 (real local Code/Test Verifier), #6
(the Evidence pipeline — AnyDoc + Web → `Evidence[]` → verification → memory),
#7 (the Agent Director — bounded role-typed children with gated, verified
results), #8 (parallel durable workflows + agent journaling) and #9 (the
causal World Model — entity graph + causal edges + pure simulation) are done.
Gate 2 (Trustworthy Executor) is real: Planning · Execution · Verification ·
Permissions · Audit · **Durability** are all wired, Gate 3 (Multi-Agent
FRIDAY) has its first two bricks (the supervisor topology, made durable), and
Gate 4 (World-Aware FRIDAY) has its foundation: the world model can now
predict "if action A runs, S→S'". Next: #10 — the LifeOS current→ideal/gap
layer done — the intent layer turns `desired_state − current_state` into an
Objective whose closing actions come from causal simulation, and the planner
consumes it as `PlanSource::GapDriven` plans. #11 done — the learning loop is
closed: `SkillCompiler` turns a verified trajectory into a `SkillSource::Learned`
skill a later goal reuses (refusal gate = verification; `skill.compounded`
carries the evidence trail). #12 done — the autonomy prerequisites are real:
`SecretsVault` (values never leak through Debug/Display/Serialize, capability-
scoped handles) + `EgressGate` (per-provider Block/Redact/LogOnly, wired into
`InferenceFabric` at both dispatch sites; blocks recorded on the routing
decision). #13 done — the first real sensor is live:
`WindowsScreenSensor` (`services/sensors/src/screen.rs`) captures the real
foreground window (identity/geometry/state) plus a **continuous pixel diff**
(FNV-1a of the client area) and emits `Focus`/`Content` `Topic::Screen`
events into the workspace; the perception engine's `with_sensors` mode polls
the real loop and reports the exact channels that fired. **#14 done — the
audio organ is real:** `AudioSensor` (`services/sensors/src/audio.rs`) runs
WASAPI mic capture → pure energy VAD (hangover never creates speech) → pure
turn detection → optional ASR seam, publishing honest `Topic::TurnDetected`
events (`transcript: null` until a real backend is attached; no device →
silence). **#15 done — the vision organ is real, completing the perception
triad:** `CameraSensor` (`services/sensors/src/vision.rs`) runs Media
Foundation webcam → pure frame diff → connected-component blob detection →
optional ML seam, publishing honest `Topic::ObjectDetected` events (first
frame is a baseline; `detector: "diff"` never fabricates labels). Screen +
Audio + Vision all real. Next: #16 — Persistent Agent Workforce (assignable
task board, persistent per-agent workspaces across runs).**

## Why this order (the corrections vs. the flat P0–P5 table)

- **Stop after `web.fetch` wiring → first real verifier.** Once `web.fetch`
  routes through the resolver, the next unit of work is the first concrete
  verifier. That is the point where FRIDAY stops merely *gaining capabilities*
  and starts gaining **trustworthy execution**.
- **Local verifier before Sonar.** `CodeTestVerifier` proves the verification
  architecture works with zero external services; then Sonar → Security →
  Evidence verifiers slot in behind the same frozen `Verifier` seam
  (`CodeTestVerifier → SonarVerifier → SecurityVerifier → EvidenceVerifier`).
- **Security before autonomy.** Egress policy + secrets move up: "can this
  secret leave the machine?" must be answerable before FRIDAY acts on its own,
  even if the implementation lands incrementally.
- **Agent Director before durable parallelism.** You cannot make durable what
  you have not shaped — topology first, durability second.
- **Skill compounding after verification.** Otherwise the closed loop trains on
  unverified trajectories and FRIDAY learns garbage.
- **Autonomous Tick last.** A heartbeat that blindly calls the LLM is a fancy
  CPU heater, not autonomy. It needs perception + world model + intent +
  planning + director + execution + verification + security first.
- **Walrus down / DrawDB last.** `MemoryLayer` already has multiple real
  implementations — Walrus is an optional fifth backend, not a missing
  capability. DrawDB is sugar until the causal world model exists.

## Update discipline

- When a work item lands (code + tests wired), flip its status to 🟢 and note
  the commit/hash in the Definition of done column.
- Keep the condensed table in the repo [README](../README.md#master-plan--the-corrected-build-order)
  in sync on the same update.
- Before flipping to 🟢, re-verify against the constitution's §2/§3 (Rule 3 —
  classify by evidence): 🟢 means code + tests exist **and are wired**, not that
  a spec or roadmap bullet mentions it.
- Record the change in the [Update log](#update-log).

## Update log

- **2026-08-10** — Plan adopted (corrected master order). Corrections: security
  (egress + secrets) moved ahead of autonomy; local Code/Test verifier before
  Sonar; Agent Director before durable parallelism; skill compounding gated on
  verification; Walrus and DrawDB moved last. Statuses 1–3 🟢 DONE; #4 `web.fetch`
  wiring marked **NEXT**.
- **2026-08-10** — Item #4 `web.fetch` resolver wiring 🟢 DONE (`ResolvingWebTool`
  serves both verbs; fallback + healthy acceptance tests; adapter extracts page
  titles). **Gate 1 (Information Organ) is complete.** Next: #5 the local
  Code/Test Verifier.
- **2026-08-10** — Item #5 real local Code/Test Verifier 🟢 DONE
  (`services/goal-execution/src/verifiers.rs`): first **concrete** (non-mock)
  verifier behind the frozen `Verifier` seam. `run_command_in` captures real
  stdout/stderr/exit code; `CodeTestVerifier::verify_command` runs the plan's
  flow, `to_verification` interprets observed `ToolResult`s; `checks[]` are
  inspectable and a failed check is `Failed`, never `Verified` (incl. the
  FAILED-marker-blocks-false-success acceptance). Constitution §2/§3
  Verification → 🟢. Next: #6 the Evidence pipeline.
- **2026-08-10** — Item #6 Evidence pipeline hardening 🟢 DONE
  (`services/goal-execution/src/evidence_pipeline.rs`): the trustworthy
  research path is real — AnyDoc + Web → unified channel-agnostic `Evidence[]`
  (`EvidenceSet` blends with dedupe + cross-channel corroboration) →
  `EvidenceVerifier` (well-formed/confidence/substance, behind the frozen
  `Verifier` seam) → `evidence_episode` rides the **verified** body into
  episodic memory through the existing admission gate. The executive publishes
  `report.evidence` for evidence-gated plans (acceptance:
  `evidence_pipeline_blends_web_and_docs_into_verified_memory`,
  `executive.rs`). Constitution P1 evidence row → DONE. Next: #7 the Agent
  Director.
- **2026-08-11** — Item #7 Agent Director / bounded subagents 🟢 DONE
  (`services/agent-runtime/src/director.rs`): the supervisor topology above
  `AgentRuntime` — role-typed bounded children (Researcher/Analyst/Verifier,
  `DirectorConfig` pool/deadline/steps), `spawn`/`assign`/`supervise`/`cancel`/
  `collect`/`terminate`/`verify`; every result runs the parent's `ResultGate`
  (`default_gate`) and the goal completes only when
  `DirectorReport::all_verified`. Real-driver acceptance proves the capability
  (`services/action-runtime/tests/agent_director.rs`): Researcher (docs) +
  Verifier (git) through `SkillAgentDriver` + real kernel are both verified,
  and an unapproved verifier blocks the run. `MergedReport::all_succeeded` now
  treats TimedOut/BudgetExhausted as failure (`merge.rs`). Constitution §2/§3
  Recursive agents → 🟢; P1 Agent Director row → DONE. Next: #8 parallel
  durable workflows + agent journaling.
- **2026-08-11** — Parallel durable workflows + agent journaling (#8): the
  `Workflow` trait gains `WorkflowStep::stage` (`workflow.rs:160`) — the Agent
  Director topology made durable, bounded-concurrency children each journaling
  + verifying independently, folded exactly once by `on_stage_completed`;
  write-ahead in-flight journaling with stable FNV-1a idempotency keys
  (`JournalEntry`/`journal_key`, `workflow.rs:253,312`): InFlight persisted
  before dispatch, terminal after; verified steps never re-dispatched
  (idempotent replay), mid-agent recovery observable via `checkpoint.recovered`;
  engine-owned persistence through `with_store`. Reviewer-hardened: the Verified
  entry + state fold + cursor advance persist as ONE atomic save (no fold-loss
  window), and a fully-verified stage persists its fold eagerly (exactly-once
  even across the crash-between-terminal-and-fold window) — locked in by
  `durable_verified_entry_always_carries_its_fold_and_cursor` and
  `stage_fold_is_applied_exactly_once_after_crash_before_eager_persist`.
  `GoalEngine::execute` is now `&self` (knowledge/reflection engines
  Mutex-wrapped) enabling true concurrent stage children. 340/340 goal-execution
  tests green, clippy-clean. Constitution §3.2 Durable execution → 🟢 §3.1;
  P2 row → DONE. Next: #9 causal World Model.
- **2026-08-11** — Causal World Model (#9): `services/goal-execution/src/
  causal.rs` adds the causal seam the constitution called the largest cognitive
  gap. **Entity graph** (`Entity` id/kind/properties, upsert-by-id;
  `EntityRelation`s; `causal.rs:43`) is the grounded instance layer;
  **CausalEdge** (action → effects, gated by `CausalCondition`
  EntityExists/EntityInState/All/Any/Always; `causal.rs:103,224`) is the
  cause→effect rule; **typed effects** (EntityCreated/Removed/StateChanged,
  VariableSet, FileCreated; `causal.rs:169`) touch both the entity graph and
  the world state. **Simulation is pure + deterministic**
  (`CausalModel::simulate`, `causal.rs:379`): the first applicable edge fires,
  its effects project S' (world + entities) on clones — the acceptance "if
  action A runs, state S changes to S'" with counterfactual purity.
  `WorldModel::simulate`/`apply_simulation` (`world.rs:423,433`) close the
  predict→commit loop and refuse **stale projections** (source_world guard).
  `find_missing_capability_prerequisites` now reflects the entity graph — an
  observed `GitRepository` entity satisfies `git.commit` without a `.git` path
  (`world.rs:494`). 11 causal unit tests + 3 integration acceptance tests
  (`causal_edge_answers_if_action_runs_state_changes`,
  `apply_simulation_commits_the_projected_state` incl. stale-refusal,
  `missing_prerequisites_reflect_the_entity_graph`); 351 lib + 15 integration
  green, clippy-clean. `WorldState`/components now derive `PartialEq` (staleness
  guard). Constitution World model → 🟢 §3.1; P2 row → DONE. Next: #10 LifeOS
  current→ideal/gap layer.
- **2026-08-11** — LifeOS intent layer (#10): `services/goal-execution/src/
  intent.rs` makes current→ideal/gap real. **DesiredState** (`intent.rs:44`) —
  a declarative ideal (entity exists / entity in state / entity property /
  file exists / tool available, with All/Any algebra; empty `Any` is never
  satisfied, empty `All` vacuously true) — is evaluated against the **world
  state + causal entity graph** (pure `evaluate`, `intent.rs`). **Gap** =
  `desired_state − current_state` with per-condition unmet reasons
  (`intent.rs:76,187`). **Objective** (`intent.rs:221`) carries the **closing
  actions** — every modeled causal edge is simulated (`CausalModel::simulate`)
  and an action is proposed only when its **projection** satisfies the ideal:
  the counterfactual, never execution. **IntentLayer** (`intent.rs:247`):
  `analyze` → `objective` → `plan`; `plan` feeds the objective into
  `PlanningContext` and injects `PlanSource::GapDriven` candidates
  (`planning.rs:67`) that compete under `PlanEvaluator` — a plan is generated
  *from* a desired-state gap, not only from a one-shot goal string. Honesty
  rule: a closed gap yields no objective and no plan; an open gap with no
  modeled closing action says so and leaves the planner to explore. 14 intent
  unit tests + 3 integration acceptance tests
  (`intent_gap_closes_after_committing_the_proposed_action`,
  `plan_is_generated_from_a_desired_state_gap`,
  `closed_desired_state_yields_no_objective_or_plan`); 364 lib + 18
  integration green, clippy-clean. Constitution LifeOS → 🟢 §3.1; P2 row →
  DONE. Next: #11 skill compounding.
- **2026-08-11** — Skill compounding (#11): the learning loop is closed.
  `services/skills/src/compound.rs` adds **SkillCompiler** — the missing
  link between verification and reuse. **Neutral input** (`TrajectorySkill`,
  `compound.rs:76`) keeps the skills crate decoupled: goal, steps
  (instruction + capability), confidence, support, and
  **`OutcomeSummary::Verified { evidence_ids }`** (`compound.rs:49`) — the
  audit trail. **The refusal gate is the verification step** (`compound.rs`
  compile): an `Unverified` outcome is refused with its reason, and
  below-bar confidence/support, empty steps, no usable capability, or no
  natural-language actions each refuse with a structured `RefusalCode`
  (`compound.rs:98`) — a failure is never learned. Success distills a
  pure-data manifest (slug id, goal description, actions = goal + step
  instructions deduped, capabilities deduped) and registers it with the new
  **`SkillSource::Learned`** (`registry.rs:26`) and every used capability
  Resolved — they all ran. **Collision-safe registration**: register with a
  retry loop on `Duplicate` (the registry is Send + Sync), so a concurrent
  compile never mislabels a dedup collision as a verdict on the trajectory;
  genuine registry errors refuse as `RegistrationFailed`. The
  **`skill.compounded`** event (`events.rs:31`) carries the full evidence
  ids, not a count. **Reuse**: a later goal's `SkillRegistry::select_for`
  ranks and chooses the learned skill exactly like a bundled one — the
  constitution acceptance, proven end-to-end in
  `services/goal-execution/tests/integration.rs`
  (`verified_trajectory_compounds_into_a_reusable_skill` — the trajectory is
  verified by the REAL `CodeTestVerifier`; `unverified_trajectory_is_refused_
  and_never_reused`). 11 compound unit tests (refusals, id uniqueness,
  dedup, reuse, event trail, unrankable-action guard); 55 skills lib + 6
  skills integration + 364 goal-execution lib + 20 integration green,
  clippy-clean, dependents clean. Constitution Skill compounding → 🟢 §3.1;
  Reflection §3.2 missing-capability → automatic trajectory mining; P2 row →
  DONE. Next: #12 Egress policy + Secrets vault.
- **2026-08-11** — Egress policy + Secrets vault (#12): the autonomy
  prerequisites. `services/intelligence/src/secrets.rs` — **SecretsVault**
  (scope → name → `Secret`): a `Secret`'s value is **structurally
  unobservable** (`Debug`/`Display`/`Serialize` all render a redaction;
  `secrets.rs:30`); only the deliberately-named `reveal()` exposes it, so a
  vault handle can never leak into an audit log, trace, or memory dump.
  `scoped(scope)` returns a **capability-scoped handle** owning only its
  scope's entries (`secrets.rs:136`); `seed_from_env()`/`seed_from_vars()`
  register the standard provider credential vars (`PROVIDER_ENV_VARS`).
  `scan()`/`redact()` are longest-first deterministic (a shorter registered
  value inside a longer one is handled correctly; `MIN_SCAN_LENGTH = 8` so a
  local placeholder like `ollama` never false-positives).
  `services/intelligence/src/egress.rs` — **EgressGate**: scans a payload
  about to leave (registered values, name+count only) and applies a
  **per-provider** `Block`/`Redact`/`LogOnly` policy; the safe default is
  `Block`, and the gate is off until attached. Wired into `InferenceFabric`
  (`providers.rs`): `fabric.egress = Some(gate)` enables scanning at BOTH
  dispatch sites (`complete_tier` + `complete_candidates`) before any
  provider call; a Block is recorded on the routing decision as a
  `CandidateFailure` (auditable "which provider was receiving it?") before
  the error returns. 11 secrets + 8 gate + 4 fabric-wiring unit tests
  (incl. `egress_policy_is_per_provider_at_the_fabric`); 66 intelligence lib
  + 6 integration green, clippy-clean, dependents clean. Constitution Egress
  + Secrets 🔴 → 🟢 §3.1; §3.3 rows + P4 row → DONE. Next: #13 Screen
  perception.
- **2026-08-11** — Vision perception (#15): the vision organ is real,
  completing the perception triad. `services/sensors/src/vision.rs` —
  **capture**: Media Foundation (`MFStartup` → `MFCreateAttributes` +
  `MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE`=VIDCAP → `MFEnumDeviceSources` →
  `IMFActivate::ActivateObject` → `MFCreateSourceReaderFromMediaSource`),
  RGB32 negotiated so frames arrive as raw BGRA; `ReadSample` loops past
  media-type changes (re-checking the flag each pass); packets are
  Lock/copy/Unlock'd; dimensions via `GetUINT64(MF_MT_FRAME_SIZE)` — never
  guessed (`pixels_to_frame` refuses a frame without a known size).
  Reviewer-hardened: the `MFEnumDeviceSources` activate array is
  `CoTaskMemFree`'d (it was leaking per construction), `MFShutdown` pairs
  `MFStartup` in a `Drop` impl, and an attached ML seam replaces the diff
  path entirely (no mixed detector/score provenance). **Pure vision**:
  `frame_diff` (BT.601 luma from BGRA), `motion_score`, `changed_regions` — a
  deterministic 4-connected BFS blob detector → `DetectedObject` bounding
  boxes with confidence = changed/box-area; boundary wrapping guarded by
  `nx < w`/`ny < h`. **Honesty**: first frame is a baseline and detects
  nothing (one frame sees nothing — motion is between frames), static scenes
  never emit, `detector: "diff"` names the built-in (no fabricated labels),
  no camera → silence. `Topic::ObjectDetected` (`sensor.vision.objects`)
  added to the workspace enum + `as_str`/`parse`/Deserialize visitor. 10
  vision unit tests (diff/motion/blob correctness incl. disconnected blobs,
  first-frame baseline, static-scene silence, honest event payload, ML seam,
  sensor-loop integration) + 32 sensors lib + 2 integration green,
  clippy-clean, dependents clean. Constitution Vision organ 🔴 → 🟢 §3.1 (15
  implemented entries); §3.3 row fully DONE; P3 row → DONE. **Perception
  triad complete: Screen + Audio + Vision.** Next: #16 Persistent Agent
  Workforce.
- **2026-08-11** — Audio perception (#14): the audio organ is real.
  `services/sensors/src/audio.rs` — **capture**: WASAPI shared-mode
  (`CoCreateInstance` MMDeviceEnumerator → `GetDefaultAudioEndpoint`
  (eCapture/eConsole) → `IAudioClient` → `IAudioCaptureClient`), preferred
  16 kHz mono 16-bit PCM, with a **`GetMixFormat` fallback** (PCM/IEEE-float at
  16/32 bits; EXTENSIBLE resolved via the SubFormat GUID — never guessed) so
  the mic path works on stacks that reject the preferred format; every packet
  is converted to i16 mono (`read_available`) so the VAD sees one shape. Pure
  + platform-neutral core, fully tested everywhere: **`rms`** (energy),
  **`Vad`** (RMS threshold + hangover that **extends but never creates**
  speech — the reviewer-caught bug where leading silence opened a turn is
  fixed), **`TurnDetector`** (speech run closed after `end_silence_frames` of
  silence → `Turn` with frames/duration_ms/samples), **`SpeechToText`** seam
  (`MockTranscriber` for tests; no dead `NoTranscriber` — the `Option` IS the
  honest null), **`build_turn_event`** (payload honest: `transcript: null` /
  `asr: null` without a backend). `AudioSensor` exposes `process_pcm` for the
  pure path + `without_capture` for headless; `unsafe impl Send + Sync` for the
  WASAPI COM interfaces is documented and sound (MTA, only touched from poll).
  `Topic::TurnDetected` (`sensor.audio.turn`) added to the workspace enum +
  `as_str`/`parse`/Deserialize visitor. Reviewer-hardened: portability gate on
  `WasapiCapture` (cfg windows, platform-neutral empty shape elsewhere),
  `GetMixFormat` format fallback, dead `NoTranscriber` removed. 12 audio unit
  tests (VAD hangover, turn closure, silence-never-opens, honest payload with/
  without ASR, end-to-end synthetic turns, sensor-loop integration,
  tolerant real-mic poll) + 22 sensors lib + 2 integration green,
  clippy-clean, dependents clean. Constitution Audio organ 🔴 → 🟢 §3.1
  (14 implemented entries); §3.3 row now scoped to vision; P3 row → DONE.
  Next: #15 Vision perception.
- **2026-08-11** — Screen perception (#13): the first **real** sensor —
  `WindowsScreenSensor` (`services/sensors/src/screen.rs`). Each poll captures
  the real foreground window: identity (`GetForegroundWindow`/`GetWindowTextW`),
  process (`QueryFullProcessImageNameW`), geometry (`GetWindowRect`), state
  (`IsIconic`/`IsZoomed`), and the **continuous screen diff** — an FNV-1a
  fingerprint of the client-area pixels via `BitBlt`+`GetDIBits`
  (`capture_client_hash`). Pure change detection with correct precedence
  (`changed_from`: identity/geometry → `Focus`, same-window pixel flip →
  `Content`; a missing hash can never invent content motion) and **honest
  degradation**: a composited/GPU window that refuses BitBlt yields
  `content_hash: None`, and identity changes still fire. Reviewer-hardened:
  the **first poll emits the initial observation** (FRIDAY perceives its
  screen on tick 1 — the docs promised it, the code now does it), later polls
  deduplicate. Events flow `SensorLoop::poll_all_with_topics`
  (`services/sensors/src/lib.rs:47`) → `GlobalWorkspace` (`Topic::Screen`,
  `services/cognitive/src/workspace.rs:110`); the perception engine gains
  `with_sensors` — Observe polls the real loop and reports the exact channels
  that fired, with the synthetic RNG fallback preserved for headless/dev.
  `Topic` has no serde derive, so `as_str`/`parse` stay the canonical mapping
  (Screen follows the existing pattern exactly). 10 screen unit tests
  (hash determinism, change precedence, None-hash honesty, event shape) + a
  tolerant real-desktop poll test + 2 sensor-loop/perception integration
  tests; sensors 10 lib + 2 integration, perception 5 lib + 2 integration,
  cognitive 24+3+2 green, clippy-clean, dependents clean. Constitution
  Real perception → **Screen perception 🟢 §3.1** (audio/vision remain 🔴 §3.3);
  P3 row → DONE for screen. Next: #14 Audio perception.
- **2026-08-11** — Web fetch gateway enhancement (continues #4): a real
  Jina-style gateway provider ships behind the resolver — `jina` →
  `https://r.jina.ai/` (`JINA_READER_PREFIX`), with **percent-encoded URL
  composition** (`compose_gateway_url`, `services/goal-execution/src/
  web_evidence.rs:141`): direct (empty prefix) fetches verbatim; any gateway
  embeds the target as one RFC 3986 component, so `?`/`&`/`#`/spaces/unicode
  can never be misread by the gateway. Proven over a real gateway transport
  (echo-server acceptance: the encoded target arrives as the gateway's path
  unmodified) and at dispatch level (recording adapter). The gateway shape
  never leaks into evidence — the canonical target URL survives, keeping the
  pipeline's `(channel, url)` dedupe/corroboration intact. Jina is the
  production fetch primary with `direct` as the health-gated fallback
  (`services/mobile/src/goal_runner.rs:567`).
