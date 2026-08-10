# FRIDAY 2.0 Synthesis — Evidence, Memory, Durable Execution

> Status: **living spec** (2026-08-09). Sources were re-verified against their
> real repositories before this document was written; every "we take" is mapped
> to FRIDAY's existing codebase (exists / partial / missing) so the roadmap
> builds on evidence, not vibes.

## 0. The one rule

> **FRIDAY owns the interfaces; the sources are concepts, not dependencies.**

We never "add" these projects to the dependency tree. We extract the strongest
subsystem idea from each, map it onto a FRIDAY-owned interface, and only then —
where genuinely missing — implement a FRIDAY-native version. If an idea maps to
code FRIDAY already has, the spec says **exists** and we move on. Adding a
dependency or a new layer that merely reorganizes existing code violates the
architectural-gravity rule (no new layer unless it *reduces* coupling).

## 1. Verified sources

All repositories were checked (2026-08-09). Where the original brief's link was
garbled, the real URL is given.

| Source | Real repo | Actual contribution (verified) |
| --- | --- | --- |
| OMH "Oh-My-Hermes" | `rlaope/oh-my-hermes` | **Evidence Before Claims** — execution states are explicitly distinguished: `Plan · not run` → `Code · running` → `Code · reported done` → `Test · verified`. An LLM *saying* it did something is not evidence. |
| TencentDB Agent Memory | `TencentCloud/TencentDB-Agent-Memory` | Memory as **reusable specialized assets**: chat memory, skill, LLM-wiki, code-graph — not one vector dump. |
| Mem0 | `mem0ai/mem0` | **Memory extraction + entity linking + token-efficient recall**; ADD-oriented memory algorithm. |
| Cognee / Cognee-RS | `topoteretes/cognee`, `topoteretes/cognee-rs` | **Knowledge-graph memory**; Cognee-RS exposes `remember / recall / improve / forget` in Rust. |
| Letta | `letta-ai/letta` | **Stateful agents, editable memory blocks, persistent identity**, cross-session state. |
| LangGraph | `langchain-ai/langgraph` | **Checkpointed durable execution**: interrupt/resume, human-in-the-loop, state snapshots. |
| Temporal | `temporalio/temporal` | **Durable workflow semantics** (workflow/task/activity/retry/timeout/cancellation/compensation) as a distributed-systems problem. |
| OpenHands | `All-Hands-AI/OpenHands` | **Event-driven agent runtime**: Agent/Controller/State/EventStream/Runtime/Sandbox/Session, events as the backbone. |
| CubeSandbox | `TencentCloud/CubeSandbox` | **RustVMM/KVM MicroVM sandbox** for agents: sub-150ms cold start, <5MB overhead, eBPF network isolation, CoW snapshots. |
| Browser Use | `browser-use/browser-use` (Py), TS port `webllm/browser-use` | **Browser automation organ**: session, controller, action registry, DOM extraction, screenshots. |
| OpenAI Codex | `openai/codex` | **Local terminal coding agent** patterns (agent loop over shell + git). |
| OpenClaw | `openclaw/openclaw` | **Personal-assistant operating model**: identity (`SOUL.md`), user model (`USER.md`), daily + long-term memory, background heartbeats. |
| FRIDAY's own ACOS | `specs/architecture-intelligence-os.md` | Global Workspace, Attention Schema, active inference, multi-tier memory, organs, world model, affect. |

> **Spec correction:** the brief's OMH evidence states were paraphrased; the
> real taxonomy is `Plan · not run / Code · running / Code · reported done /
> Test · verified`. FRIDAY's `ExecutionState` (§5) uses the same epistemic
> principle with FRIDAY's own vocabulary.

## 2. Grounded mapping — what FRIDAY already has

Every idea in the brief, mapped to the actual codebase. **Already built** is the
most common answer — the brief was written without reading FRIDAY's tree.

> **Spec correction (2026-08-09):** two rows originally read "persona engine
> spec'd but not built" and "no DOM/session/screenshot organ". An alignment
> audit against the tree found both **already implemented**: the Persona Engine
> (`services/persona`) exists as a tested-but-unwired crate, and the Browser
> organ (`BrowserAdapter`) exists and is e2e-wired. The rows above now reflect
> reality; their remaining gap is **wiring**, not construction.

| Source idea | FRIDAY today | Where |
| --- | --- | --- |
| **Evidence states / verification gates** | ⚠️ **Partial → the gap we close now** | `Verification` + `VerifiedResult { verified, tool_success, checks, confidence }`, `TraceNodeKind::Verification`, `TraceEvent::VerificationFinished` (goal-execution). **Missing:** an explicit, auditable *epistemic lifecycle* (`Prepared → Observed → Verified`) on the execution path. `LifecycleState` in action-runtime (`services/action-runtime/src/kernel/lifecycle.rs`) is the *process* lifecycle (Created/Running/Cancelled/Completed/Archived) — a different axis. |
| **Episodic memory** | ✅ Exists | `Episode` (engines/memory) with `importance` + `consolidated`; `EpisodeStore` + `CognitiveEpisode` (goal-execution); episodes from `GoalCreated / ToolExecuted / OutcomeObserved` workspace events. |
| **Semantic memory** | ✅ Exists | `SemanticMemory`: episodes → `ExperiencePattern` → promoted `Knowledge`/`Policy` (goal-execution). |
| **Procedural memory** | ✅ Exists | `LearningEngine` → `Policy` (retry/sequence/prefer-tool) (goal-execution `policy.rs`/`learning.rs`). |
| **Memory specialization (TencentDB)** | ⚠️ Partial | Working/episodic/semantic/procedural tiers exist. The "specialized assets" (chat/skill/wiki/code-graph) are not realized as queryable stores. |
| **Memory admission controller (Mem0)** | ✅ **Exists (Phase 1b shipped)** | `MemoryAdmissionController` (goal-execution `admission.rs`): `admit(episode) → Store/Reject/Pending` gated on sensitivity / evidence / reliability / importance / novelty; deterministic signals; sensitive episodes held `Pending` for human `approve`/`reject`; verdict + signal recorded on `ExecutiveReport`. **Scope boundary:** gates the durable `EpisodeStore` (SemanticMemory's input); the `MemoryWatcher` raw-observation path stays unfiltered (faithful-observer contract, feeds working context). |
| **Entity-linking / graph memory (Cognee)** | ✅ **Exists (Phase 4 shipped)** | `KnowledgeGraph` (friday-knowledge `graph.rs`): triple store with M5.6 contract (`assert_fact`/`query`/`find_missing_preconditions`/`has_preconditions`/`creating_capabilities`) **+** Cognee API (`remember` strengthens on repetition, `recall`, `improve` by id with `[0,1]` clamp, `forget`), seeded M5.6 ontology (Requires/Creates), deterministic confidence (no LLM in the store), serde + `JsonKnowledgeGraphStore` (atomic temp+rename). Wired into goal-execution `WorldModel` as its relationship layer (`knowledge()`/`knowledge_mut()`/`find_missing_capability_prerequisites` — entities derived from state: files, `.git`, network). **Not yet wired:** `Planning::think()` appending prerequisite steps into the task graph (`creating_capabilities` exists and is tested; the append half is the next brick). |
| **Persistent identity + editable memory (Letta/OpenClaw)** | ⚠️ **Built, not yet wired** | `services/project-cognition` persists project state/decisions. **Persona Engine IS built** (`services/persona`, crate `friday-persona`, spec `specs/persona-engine.md`): `PersonaEngine` (one `Identity`, many data-only `Persona`s, blended `StyleSpec`, PAD `Mood` driven by OCC `Appraisal`, per-persona cooldown/affinity, `render_instruction` → deterministic behavioral spec the planner hands to whichever model is active) — but **nothing outside the crate imports `friday_persona` yet**: it is tested and standalone, not wired into context/planner/bridge. No `SOUL.md`-style identity store (project-cognition is the closest). |
| **Checkpoint / interrupt / resume (LangGraph)** | ✅ **Exists (Phase 2 shipped)** | `WorkflowEngine` + `WorkflowCheckpoint` (goal-execution `workflow.rs`): durable state snapshots, approval + checkpoint-boundary interrupts, resume semantics on top of the evidence lifecycle. |
| **Durable workflow semantics (Temporal)** | ✅ **Exists (Phase 2 shipped)** | `Workflow` trait (durable step sequences), bounded per-step retries (attempts in checkpoint), `InMemoryWorkflowStore` + `JsonWorkflowStore` (atomic temp+rename). Cancellation/compensation remain future bricks. |
| **Event-driven architecture (OpenHands)** | ✅ **Exists** | `friday-core::event::Event` bus with `trace_id`; `events::EXEC_ACTION / EXEC_RESULT / ARTIFACT_CREATED / GIT_COMMIT_CREATED / …`; cognitive topics `GoalCreated / ToolExecuted / OutcomeObserved`. FRIDAY's vocabulary: `user.intent → plan.created → tool.requested → tool.output → tool.verified → memory.created → goal.completed`. |
| **Sandboxed execution (CubeSandbox)** | ⚠️ Partial | `PermissionEngine` + `ExecutionContext` (timeout/deadline/cancellation) + `ResourceManager` (kernel) gate and bound execution. A MicroVM/container sandbox is not wired. |
| **Browser organ (Browser Use)** | ✅ **Exists (built, e2e-wired)** | `BrowserAdapter` (action-runtime `adapters/browser.rs`, chromiumoxide/CDP) is a real browser organ: `web.open / click / type / scroll / extract / screenshot`, persistent sessions (`user_data_dir`), risk-classified verbs (`click`/`type` = Medium, others Safe), and `register_browser_capabilities` flips the `web` skill's soft-declared verbs to resolved. `WebAdapter` (`adapters/web.rs`) remains the HTTP search/fetch channel. Wired in e2e (`skill_planning_suite`, `local_model_task`); **not yet in the production daemon boot path**. |
| **Developer organ (Codex/OpenHands)** | ✅ **Exists** | `GitAdapter`, `FilesystemAdapter`, `TerminalAdapter` (action-runtime) + Git skill + `AdapterTool` evidence seam (goal-execution `evidence.rs`). |
| **Investigation Engine** | ✅ **Exists (built before this brief)** | `services/investigation` (heuristic + LLM strategy), wired into the planner prompt (`EVIDENCE REQUIRED`), the executive cycle (`ExecutivePhase::Investigate`), the cognitive trace, and the live console graph. |
| **Active-inference supervisor** | ⚠️ Spec'd | ACOS/`specs/architecture-intelligence-os.md`; no crate yet. Deliberately last. |

## 3. Interface inventory — the brief's nine traits

The brief proposes `trait Memory / Organ / Executor / Planner / EvidenceStore /
WorldModel / ModelProvider / Sandbox / Workflow`. What actually exists:

| Proposed trait | FRIDAY equivalent | Status |
| --- | --- | --- |
| `ModelProvider` | `ModelClient` / `InferenceFabric` (intelligence) + `LlmStrategyClient` (investigation) | ✅ |
| `Executor` | `Tool` (goal-execution), `ActionAdapter` (action-runtime) | ✅ |
| `Planner` | `PlanningEngine` / `InvestigationStrategy` | ✅ |
| `Organ` | `Plugin` (friday-core) + `Skill` (friday-skills) + `ManagedResource` | ✅ (as plugins/skills) |
| `WorldModel` | `WorldModel` struct + `StateTransition` (goal-execution `world.rs`) | ✅ (struct; a *trait* is optional) |
| `Memory` | Tiers exist as concrete types (`EpisodeStore`, `SemanticMemory`, `LearningEngine`) | ⚠️ no unified `trait Memory` — only worth adding when a second implementation exists |
| `EvidenceStore` | `AuditEngine` (action-runtime) + `CognitiveTrace` | ⚠️ no unified trait |
| `Workflow` | `Workflow` trait + `WorkflowEngine` + checkpoint (goal-execution) | ✅ (Phase 2 shipped) |
| `Sandbox` | `PermissionEngine` + `ExecutionContext` | ❌ no trait/abstraction — Phase 2 |

**Rule:** a trait is only created when there are ≥2 implementations to unify.
`trait Memory`/`trait EvidenceStore`/`trait Sandbox` are deferred until the
second implementation exists. **`trait Workflow` is the exception — shipped in
Phase 2 as the FRIDAY-owned durable-execution interface** (user-directed; its
`WorkflowStore` seam already has two implementations, InMemory + Json).

## 4. Gap ledger (roadmap, grounded)

Phases from the brief, re-sequenced against reality:

- **Phase 1 — Evidence + Memory Fabric.**
  - **1a. Evidence lifecycle (`ExecutionState`) — SHIPPED in this milestone.** The
    epistemic state machine (`Prepared → Observed → Verified/Failed`) on the
    goal-execution path, recorded on `GoalReport` + trace. Closes the "FRIDAY
    believes something happened because FRIDAY intended it" hole.
  - **1b. Memory admission controller** (Mem0). **SHIPPED.** `admit(episode) →
    Store/Reject/Pending` gated on sensitivity → evidence → reliability →
    importance → novelty (epistemic before value, so failures are labeled
    `Unreliable`, never `LowImportance`). Deterministic signals derived from the
    episode + store; word-boundary sensitivity scan (no `pwd`/`secretary`
    false positives); human `approve`/`reject` for `Pending`; verdict + signal
    on `ExecutiveReport`. Defaults preserve prior behavior (verified successes
    store; repeats never rejected by default so SemanticMemory still forms
    patterns). **Follow-ups:** persist `Pending` holds (JSON store precedent),
    surface holds to the console, LLM-assisted sensitivity classification
    (gate stays deterministic).
  - **1c. Memory specialization queries** (TencentDB). Typed accessors over the
    existing tiers (chat facts, skills, wiki, code-graph) rather than a new store.
- **Phase 2 — Durable Execution — SHIPPED in this milestone.** Cognitive-level
  checkpoint → approval interrupt → resume on top of the evidence lifecycle
  (`ExecutionState`). `Workflow` trait + `WorkflowEngine` + serializable
  `WorkflowCheckpoint`; declarative interrupt points (`requires_approval`,
  `checkpoint_after`); bounded retries; `WorkflowStore` trait with two
  implementations (InMemory, Json — atomic temp+rename). **Trait note:** the
  brief's "≥2 implementations" rule is satisfied at the *store* seam; the
  `Workflow` *step-sequence* trait was user-directed (the FRIDAY-owned
  interface is the point of the synthesis).
- **Phase 3 — Investigation Engine.** ✅ Already built; extended: **every
  evidence action in an evidence-gated plan now carries its own
  Prepared → Observed → Verified | Failed trail** (`EvidenceTrail`, shipped in
  this milestone — see §6b), and the real adapters (`web.search`/`git.status`)
  already live via `evidence.rs`.
- **Phase 4 — World Model — SHIPPED in this milestone.** `WorldModel` gained
  its relationship layer: the **Cognee-style graph memory** (`KnowledgeGraph`
  in `friday-knowledge`, see §2 row) with the M5.6 seed ontology and the
  `remember/recall/improve/forget` API, wired via `knowledge()` /
  `find_missing_capability_prerequisites`. Entity relationships are real
  (Requires/Creates triples); **causality + counterfactuals remain future
  bricks** — the graph stores what is, not why it happened. Next brick:
  `Planning::think()` consuming `creating_capabilities` to prepend
  prerequisite steps into the task graph.
- **Phase 5 — Proactive FRIDAY.** Active-inference supervisor from telemetry →
  uncertainty → intrinsic goal → investigate → act → verify → learn. Only after
  Phases 1–2 prove out.

## 5. Phase 1a — `ExecutionState` evidence lifecycle (this milestone)

### Design

```text
Prepared ──► Observed ──► Verified
     │           │
     │           └──► Failed       (tool error or verification failure)
     └──► Cancelled                (never dispatched — action-runtime layer)
```

- `ExecutionState { Prepared, Observed, Verified, Failed, Cancelled }` in
  goal-execution `verification.rs`, with `next_allowed()` / `is_terminal()` /
  `label()` and serde round-trip.
- **Distinct from `LifecycleState`** (action-runtime): that is the *process*
  lifecycle (what the scheduler did); `ExecutionState` is the *evidence*
  lifecycle (what FRIDAY knows). One axis does not imply the other.
- `GoalEngine::execute` drives it: Prepared (action resolved) → Observed (tool
  produced raw output) → Verified (output passed verification) or Failed.
- `GoalReport` carries the final `execution_state`; the cognitive trace records
  the transitions (`prepared` on the Execution node input, `observed` on its
  output, `verified`/`failed` on the Verification node output).
- `ToolResult` stays raw tool output (observed-but-uninterpreted by design);
  `VerifiedResult` stays the interpretation. The state is owned by the
  execution, not the tool.

### Invariants

1. FRIDAY never claims success before `Verified`.
2. Verification only ever runs on `Observed` output.
3. The final state is terminal (`Verified | Failed | Cancelled`).
4. The trace always records the transition, so the audit trail can answer
   "what did FRIDAY *know* at each step?" — not just "what did she do?".

## 6. Phase 2 — `Workflow` durable execution (this milestone)

### Design

```text
WorkflowEngine.start(workflow)
  │
  ├── step 0 ──► GoalEngine ──► Verified ──► checkpoint + evidence[0]
  ├── step 1 ──► requires_approval? ──Pause(ApprovalRequired)──┐
  │              (approve → resume)                             │
  ├── step 2 ──► Verified ──► checkpoint_after? ──Pause(Boundary)┐
  └── …                                                         ▼
                                resume() from the stored checkpoint
```

- **`WorkflowStep`** (pure data): label, action, expectation, declarative
  interrupts (`requires_approval` before dispatch, `checkpoint_after` after
  verification), `max_attempts` (bounded retry).
- **`WorkflowCheckpoint`** — the durable state snapshot: `workflow_id`,
  `next_step` cursor, workflow-owned `state` (JSON), granted `approvals`,
  per-step `attempts`, and the full `evidence` trail (every attempt's
  `ExecutionState`). Serialize/Deserialize, so it survives a crash verbatim.
- **Resume semantics:** `resume()` continues from `next_step`; already-verified
  steps are never re-run (the evidence trail is the truth, not re-derived).
  A fresh engine + stored checkpoint = crash-safe restart.
- **Interrupt points are data, not code** — the same philosophy as skills'
  `readonly`.
- **Stores:** `WorkflowStore` trait (`save/load/delete/list`) with
  `InMemoryWorkflowStore` (tests/short-lived) and `JsonWorkflowStore` (one file
  per workflow, atomic temp+rename, filesystem-safe ids).

### Invariants

1. The checkpoint is the single source of truth for position and evidence.
2. Success is claimed only through the evidence lifecycle: a step is recorded
   `Verified` exactly when the `GoalEngine` returned `ExecutionState::Verified`.
3. Every attempt is recorded — a failed attempt is an honest observation,
   never overwritten by a later success.
4. Retries are bounded per step and counted in the checkpoint (no unbounded
   spin, even across resumes).
5. Already-verified steps are never re-executed on resume.

## 6b. Phase 3 — evidence trail: every evidence action carries its own lifecycle

### Design

```text
Evidence-gated plan (PlanSource::Investigation)
  actions = [gather evidence: web (web.search),      ← evidence action
             gather evidence: git (git.status),      ← evidence action
             <goal's own action>]                    ← main execution

WorkflowEngine/GoalEngine.execute per action
  ├── web.search:  Prepared → Observed → Verified   → EvidenceActionRecord{channel: web,  Verified}
  ├── git.status:  Prepared → Observed → Verified   → EvidenceActionRecord{channel: git,  Verified}
  └── goal action: Prepared → Observed → Verified   → main GoalReport
         │
         ▼
  EvidenceTrail { goal, records } → ExecutiveReport + workspace (bridge live trace)
```

- **`EvidenceTrail`** (`evidence.rs`): one [`EvidenceActionRecord`] per evidence
  action in plan order — step, instruction, capability id (`git.status`),
  channel (adapter tool prefix), final `execution_state`, verified,
  tool_success, duration. `all_verified()` is the gate query: success may be
  claimed only when every evidence action ended `Verified`.
- **`is_evidence_action`** = instruction carries `gather evidence: … (<cap_id>)`
  **and** the capability is `Capability::Custom` — a goal description that
  happens to contain `(word.word)` with a built-in capability can never be
  misclassified.
- **Executive Execute phase** (previously only `actions.first()` ran): every
  evidence action now executes through `GoalEngine` (its own lifecycle), then
  the goal's own action runs as the main report. The merged cognitive trace
  reads investigation → planning → per-evidence-action exec/verify → goal
  action; the trail is published to `ExecutiveReport` and the workspace
  (`evidence_trail` slot, cleared at the start of every Execute so a direct
  cycle never renders a previous goal's trail).

### Invariants

1. One `EvidenceActionRecord` per evidence action — never one record for the
   whole plan.
2. A failed evidence action is recorded `Failed` (never `Verified`), and
   `all_verified()` is false — the gate is per-channel, not per-plan.
3. Every evidence action actually executes (multi-channel plans no longer drop
   everything after the first channel).
4. Direct plans (no evidence requirements) publish an empty trail / cleared
   workspace slot.

## 7. Guarded risks

- **Architectural gravity:** every idea above is mapped to an existing FRIDAY
  surface before any new code. No new crate, no new trait without ≥2
  implementations.
- **Admission controller ≠ surveillance:** memory admission must include a
  sensitivity gate and a `Pending` (human-confirmable) state — Mem0's
  "everything becomes memory" is explicitly rejected.
- **Sandbox depth:** `PermissionEngine` gates *authorization*; a VM/container
  sandbox gates *containment*. They are complementary, not substitutes.
