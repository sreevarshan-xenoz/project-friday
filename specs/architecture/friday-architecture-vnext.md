# FRIDAY Architecture vNext — The Architectural Constitution

**Status:** Living constitution (2026-08-10). Consolidates the ACOS design with
~15 external reference projects reviewed across batches. Every subsystem is
classified against the **actual codebase** (`file:line` evidence), not against
the roadmap's target architecture. This document is the **filter** every future
GitHub project is measured against.

**Rule 1 — Freeze the interfaces.** `friday-core`, `friday-planner` APIs, event
bus, and the `ActionAdapter` / `Verifier` / `MemoryLayer` / `WorkflowStore`
contracts are frozen. No new `MemoryBackend`, `InformationSource`,
`DocumentParser`, `Verifier`, or sibling seam unless the existing seam
*genuinely fails* (trait rule: a new trait needs ≥2 implementations to unify —
`Workflow` is the accepted exception, user-directed).

**Rule 2 — Steal patterns, not repos.** FRIDAY owns the interfaces; external
projects are reference implementations or optional adapters. No Frankenstein
wrapper, no "install Multica inside FRIDAY", no dependency on another project's
runtime.

**Rule 3 — Classify by evidence.** `[BUILT]`/🟢 means code + tests exist and are
wired. `[PARTIAL]`/🟡 means the framework exists but key paths are mock/simulated/
not wired. `[MISSING]`/🔴 means no code. A spec page or a roadmap bullet is
**not** a status.

---

## 1. The ACOS control loop

The full loop the system must run continuously:

```text
PERCEIVE → UNDERSTAND → DEFINE DESIRED STATE → PLAN → DELEGATE → EXECUTE
     → VERIFY → LEARN → UPDATE STATE → PERCEIVE AGAIN
```

| Loop stage       | FRIDAY subsystem                       | Source of the pattern |
|------------------|----------------------------------------|-----------------------|
| PERCEIVE         | Perception engines, sensors            | Vision Agents, Meetily, AnyDoc, Agent-Reach |
| UNDERSTAND       | Cognitive core, context builder        | FRIDAY cognitive-runtime |
| DESIRED STATE    | Goal system, user model, gap analysis  | **LifeOS** (current → ideal) |
| PLAN             | friday-planner (intent → task graph)   | FRIDAY + Google ADK |
| DELEGATE         | Agent runtime, task board              | **Multica**, Prime Agent |
| EXECUTE          | Action runtime, workflow engine        | DeepZero pipelines, OpenMontage |
| VERIFY           | Verification + planner verify          | SonarQube CLI, PR-Agent |
| LEARN            | Reflection, experience, knowledge      | LifeOS, Prime Agent, Multica skills |
| UPDATE STATE     | World model, memory                    | FRIDAY + Walrus |

---

## 2. Status map (locked) — the honest picture

> **This is the answer to "does FRIDAY have it?", not "is it in the roadmap?"**
> The roadmap defines the *target* (world model, durable execution, HITL,
> multi-agent topologies, proactive agency). The table below records what
> *exists and runs* today.

| Subsystem                      | Status | Reality |
|--------------------------------|--------|---------|
| **Memory**                     | 🟢 Implemented | Multiple real `MemoryLayer` implementations |
| **Model routing**              | 🟢 Implemented | Capability/provider routing + expert reasoning router |
| **Security / permissions**     | 🟢 Implemented | Permission, audit, MCP gating, HITL |
| **HITL**                       | 🟢 Implemented | Approval/interrupt paths exist |
| **Skills**                     | 🟢/🟡 Implemented–Partial | Real manifests/runtime; learning/compounding incomplete |
| **Web resolver**                | 🟢 Wired | BindingResolver → health-gated provider fallback (`services/capabilities/src/resolver.rs:31`); `web.search` **and** `web.fetch` routed end-to-end through the evidence seam — resolver → Action Runtime → normalized `Evidence[]` (`services/goal-execution/src/web_evidence.rs:129`; acceptance: search fallback at `web_evidence.rs:736`, fetch fallback at `web_evidence.rs:806`). Fetch now ships a **real gateway provider** — `jina` → `https://r.jina.ai/` with percent-encoded URL composition (`compose_gateway_url`, `web_evidence.rs:141`; gateway wins never leak the composed URL into evidence — the canonical target survives). Jina is the production fetch primary with `direct` as the health-gated fallback (`services/mobile/src/goal_runner.rs:567`). The whole Web organ is wired |
| **Durable execution**          | 🟡 Partial | Checkpoint/resume exists; no full crash-safe agent journaling |
| **Verification**               | 🟢 Implemented | Real framework **+ first concrete verifier**: `CodeTestVerifier` — workspace + command → real exit code/stdout/stderr → inspectable `checks[]` → `VerifiedResult` (`services/goal-execution/src/verifiers.rs:61,226,274`); FAILED-marker-blocks-false-success acceptance (`verifiers.rs:416`). Sonar/security/evidence verifiers remain additive behind the same seam |
| **World model**                | 🟢 Implemented | Entity graph + causal edges + pure simulation ("if A runs, S→S'") — `services/goal-execution/src/causal.rs:43,224,311` |
| **Cognitive planner**          | 🟡 Partial | Real planner; not yet the world-model-driven cognitive planner |
| **Reflection**                 | 🟡 Partial | Real machinery; no closed self-improvement loop |
| **Context builder**            | 🟡 Partial | Infrastructure exists; integration depth needs verification |
| **Recursive agents / Agent Director** | 🟢 Implemented | Supervisor topology over `AgentRuntime`: role-typed bounded children (Researcher/Analyst/Verifier), results gated — a run completes only when `DirectorReport::all_verified` (`services/agent-runtime/src/director.rs:54,197,342`); wired through the real `SkillAgentDriver` (`services/action-runtime/tests/agent_director.rs:119`) |
| **Autonomous drive**           | 🔴 Missing | No genuine self-directed continuous loop |
| **LifeOS intent layer**        | 🔴 Missing | Goals exist; current→ideal / gap reasoning doesn't |
| **Persistent agent workforce** | 🔴 Missing | Agents are ephemeral |
| **Real perception**            | 🔴 Missing | Perception is simulation, not actual sensing |
| **Audio organ**                | 🔴 Missing | No real audio sensor |
| **Vision organ**               | 🔴 Missing | No real vision sensor |
| **World-model visualization**  | 🔴 Missing | No visual world-model layer |
| **Egress policy**              | 🔴 Missing | No generalized outbound-data policy |
| **Secrets vault**              | 🔴 Missing | No proper secrets subsystem |
| **OpenTelemetry/Prometheus**   | 🔴 Missing | No mature observability plane |
| **Cost telemetry**             | 🔴 Missing | No model-cost accounting layer |

---

## 3. Subsystem registry — evidence per subsystem

Each entry: **what exists** (evidence), **missing capability**, **inspired by**,
**plugs into** (frozen FRIDAY seam), **acceptance criteria**. Grouped by status.

### 3.1 Implemented (🟢)

#### Memory — 🟢
- **What exists:** 4-tier engine over `MemoryLayer` trait (`Working`/`Episodic`/
  `Semantic`/`Procedural`); `MemoryEngine` (`core/src/memory/traits.rs:86`),
  `RedbStore` durable impl (`backends.rs:30`), vector store with
  `search_similar` (`backends.rs:260`), `MemoryAdmissionController` gating
  episodic intake (`services/goal-execution/src/admission.rs`).
- **Missing capability:** specialization into queryable chat/skill/wiki/code
  assets (TencentDB idea) — deferred; single store satisfies current needs.
- **Inspired by:** Mem0, TencentDB, Walrus.
- **Plugs into:** frozen `MemoryLayer` + `MemoryEngine`.
- **Acceptance:** episodic→semantic→procedural promotion is tested; admission
  controller verdicts recorded; a Walrus-backed `MemoryLayer` is an optional
  adapter, never a core change.

#### Model routing — 🟢
- **What exists:** `IntelligenceRouter::route → CognitiveMode::System1/System2`
  (`services/intelligence/src/router.rs:16`); `ExpertRouter` best-fit expert per
  goal with latency/confidence tiers + shadowing (`adapters/reasoning.rs:449`);
  `ProviderRegistry` (`provider.rs:436`); OpenAI-compatible client covering
  OpenRouter/Groq/Cerebras/Gemini/Ollama + qualification (`intelligence/src/providers.rs:191`).
- **Missing capability:** model-cost ledger (see §3.3).
- **Inspired by:** Archestra, OpenMontage.
- **Plugs into:** `ModelProvider` / `InferenceFabric` seam.
- **Acceptance:** shadowing output recorded; qualification tiers enforced.

#### Security / permissions — 🟢
- **What exists:** `PermissionDecision {Allow, AllowOnce, AllowAlways, Deny}`
  with deny-substring lists + configurable default
  (`services/action-runtime/src/permission.rs:27`); `AuditEngine` bounded ring,
  every action recorded with verdict (`audit.rs:31`); skill-scoped agent gating
  (`agent_driver.rs:111`).
- **Missing capability:** secrets vault, egress secret-scan policy (§3.3).
- **Inspired by:** Archestra.
- **Plugs into:** frozen permission + audit contracts.
- **Acceptance:** every action dispatch is permissioned + audited; deny-substring
  can never be overridden by a skill.

#### HITL — 🟢
- **What exists:** `AllowOnce` + workflow pause-before-dispatch when approval
  required (`workflow.rs`); mobile pairing/auth (`services/mobile/src/auth.rs`).
- **Missing capability:** richer interrupt surfaces (approval request as a
  first-class event to the user) — incremental.
- **Inspired by:** Archestra, OpenMontage, ADK.
- **Plugs into:** `Workflow` declarative interrupts + permission engine.
- **Acceptance:** an approval-gated workflow pauses, resumes only on approve, and
  never re-runs verified steps after resume.

#### Skills — 🟢/🟡
- **What exists:** `skill.json` manifest (id/version/capabilities/actions/
  permissions/readonly/memory writes/examples/min_core_version) —
  `skills/bundled/web/skill.json`; `SkillRegistry`, `SkillPolicy`, `RiskResolver`,
  `SkillApprovals` in `friday-skills`.
- **Missing capability:** closed compounding loop (trajectory → procedure →
  verified skill → registry) — §3.3, skill compounding.
- **Inspired by:** Multica, Prime Agent.
- **Plugs into:** `SkillRegistry` + `AgentDriver` gating.
- **Acceptance:** a skill can only execute within its declared permission scope;
  readonly skills never mutate state.

#### Verification — 🟢 (real framework + first concrete verifier)
- **What exists:** `Verification` registry of `Box<dyn Verifier>`,
  `VerifiedResult { verified, tool_success, checks, confidence }`, execution
  lifecycle `Prepared → Observed → Verified|Failed`
  (`services/goal-execution/src/verification.rs:116,182,265`); planner verifier/
  result/retry; **first concrete (non-mock) verifier shipped**: `CodeTestVerifier`
  — real command run in a workspace with captured stdout/stderr/exit code
  (`services/goal-execution/src/verifiers.rs:61,226`), inspectable `checks[]`
  (`command_run_checks`, `verifiers.rs:94`) over both live runs and observed
  `ToolResult`s (`verify_command`, `to_verification`, `verifiers.rs:274,288`);
  10 tests green incl. "FAILED marker blocks false success" (`verifiers.rs:416`).
- **Remaining (additive, behind the same seam):** Sonar → Security verifiers
  slot into the frozen `Verifier` trait; `NEW VerificationSystem` is forbidden
  by Rule 1. **The Evidence verifier has landed**: `EvidenceVerifier`
  (well-formed/confidence/substance checks) gates the blended research body
  (`services/goal-execution/src/evidence_pipeline.rs:388`), and the executive
  blends Web + Docs evidence into one verified body that rides into episodic
  memory (`evidence_pipeline.rs:138,449`; acceptance at
  `executive.rs:1639`).
- **Inspired by:** SonarQube CLI, PR-Agent, OMH evidence-before-claims.
- **Plugs into:** frozen `Verifier` trait + `ExecutionState` lifecycle.
- **Acceptance:** a real verifier runs on observed output and produces a
  `VerifiedResult` whose `checks` are inspectable; a failed check is recorded
  `Failed`, never `Verified`.

#### Agent Director — 🟢 (supervisor topology over AgentRuntime)
- **What exists:** `AgentDirector` supervisor above `AgentRuntime` — role-typed
  children (`AgentRole` Researcher/Analyst/Verifier,
  `services/agent-runtime/src/director.rs:54`), bounded by `DirectorConfig`
  (pool cap / deadline / step budget, `director.rs:93`), with the method set
  `spawn`/`assign`/`supervise`/`cancel`/`collect`/`terminate`/`verify`
  (`director.rs:249,274,300,309`). Results are gated by a `ResultGate`
  (`default_gate`: state + non-empty summary + verifier-ran-a-step,
  `director.rs:342`); a run completes only when `DirectorReport::all_verified`
  (`director.rs:147`). `MergedReport::all_succeeded` treats TimedOut /
  BudgetExhausted as failure (`merge.rs:26`). **Wired with the real driver:**
  acceptance runs `SkillAgentDriver` + the real kernel through the director —
  Researcher (docs) + Verifier (git) both verified, and an unapproved verifier
  blocks the run (`services/action-runtime/tests/agent_director.rs:119`).
- **Missing capability:** persistent workforce (suspend/resume, assignable task
  board, per-agent workspaces across runs) — deferred to the P3 step by design.
- **Inspired by:** Prime Agent, Multica, Google ADK.
- **Plugs into:** `AgentRuntime` + `AgentDriver` seam.
- **Acceptance:** a goal can be decomposed into a director spawning bounded
  workers whose results are collected and verified before the goal completes
  (`services/agent-runtime/tests/director.rs:28`).

#### Durable execution — 🟢 (parallel stages + write-ahead journaling + mid-agent recovery)
- **What exists:** `WorkflowEngine` + `WorkflowCheckpoint` (serializable
  snapshot: next_step cursor, state, approvals, attempts, full evidence trail,
  append-only journal) — `services/goal-execution/src/workflow.rs:348`;
  `WorkflowStore` trait with InMemory + Json (atomic temp+rename) stores and
  engine-owned persistence via `WorkflowEngine::with_store`.
  **Parallel stages (map/reduce):** the `Workflow` trait gains a stage step
  (`WorkflowStep::stage`, `workflow.rs:160`) — bounded-concurrency fan-out
  (the Agent Director topology made durable), per-child journaling + evidence,
  folded exactly once by the `on_stage_completed` hook.
  **In-flight journaling (write-ahead):** every dispatch records a stable
  idempotency key (`JournalEntry` + FNV-1a `journal_key`, `workflow.rs:253,312`)
  — InFlight persisted before the run, terminal outcome after; a crash resumes
  with explicit, observable recovery (`checkpoint.recovered`) instead of a
  silent re-run, verified steps are **never re-dispatched** (idempotent
  replay), and a Verified entry + state fold + cursor advance land in **one
  atomic save** (no fold-loss window). Approval-gated steps cannot be stage
  children (rejected loudly).
- **Missing capability:** multi-process durable workflows (cross-node
  recovery) + finer-grained journaling of the agent's *reasoning* state —
  additive behind the same frozen seam.
- **Inspired by:** DeepZero, LangGraph, Temporal.
- **Plugs into:** `Workflow` trait + `WorkflowStore` seam (frozen).
- **Acceptance:** a simulated crash mid-run + `resume()` from a stored
  checkpoint reproduces exactly the evidence trail with no re-execution of
  verified steps; a parallel stage runs with bounded concurrency, folds exactly
  once even after a crash between terminal persist and fold persist, replays
  verified children, and recovers in-flight ones (`workflow.rs` —
  `parallel_stage_runs_children_with_bounded_concurrency_and_folds`,
  `stage_fold_is_applied_exactly_once_after_crash_before_eager_persist`,
  `engine_with_store_persists_write_ahead_before_dispatch_and_terminal_after`).

#### World model — 🟢 (entity graph + causal edges + simulation)
- **What exists:** `WorldModel` with `WorldState` snapshot + immutable
  `StateTransition` history + embedded `KnowledgeGraph` (Requires/Creates edges)
  + the **causal layer** (`causal.rs`): a grounded **entity graph** (`Entity`
  id/kind/properties + `EntityRelation`s, `causal.rs:43`), typed **causal
  edges** (`CausalEdge`: action → effects gated by `CausalCondition`
  EntityExists/EntityInState/All/Any/Always, `causal.rs:103,224`) and typed
  **effects** (EntityCreated/Removed/StateChanged, VariableSet, FileCreated,
  `causal.rs:169`). **Simulation is pure and deterministic**
  (`CausalModel::simulate`, `causal.rs:379`): the first applicable edge fires
  and its effects project S' (world + entities) without touching the model —
  exactly "if action A runs, state S changes to S'". `WorldModel::simulate` /
  `apply_simulation` (`world.rs:423,433`) close the predict→commit loop with a
  **stale-projection guard** (a sim derived from an older world is refused), and
  `find_missing_capability_prerequisites` now reflects the **entity graph** — an
  observed `GitRepository` entity satisfies `git.commit` without any `.git` path
  (`world.rs:494`).
- **Missing capability (still NOT visualization):** self-observed causal
  learning (edges mined from executed transitions — that's #11 skill
  compounding), multi-entity counterfactual chains, and the LifeOS
  current→ideal/gap layer (#10) that consumes these edges for planning.
  DrawDB-style visualization stays sugar, deferred.
- **Inspired by:** Cognee, DrawDB (visualization only, later), ACOS world model.
- **Plugs into:** `WorldModel` struct + `KnowledgeGraph` (M5.6 contract).
- **Acceptance:** a causal edge answers "if action A runs, state S changes to
  S'" (proven with counterfactual purity in
  `services/goal-execution/tests/integration.rs` — `causal_edge_answers_if_
  action_runs_state_changes`); `find_missing_capability_prerequisites` reflects
  real state, not just static graph (`missing_prerequisites_reflect_the_
  entity_graph`); `apply_simulation` commits S' + an audit transition and
  refuses stale projections.

### 3.2 Partial (🟡)

#### Cognitive planner — 🟡 (real, not yet world-model-driven)
- **What exists:** full intent → plan → execute → verify pipeline: intent
  parser/classifier, `PlannerRouter` (llm/rule/backend), `TaskGraph`, dispatcher/
  executor/scheduler/supervisor, capability registry, persistence, experience
  replay — `planner/src`. Most complete subsystem.
- **Missing capability:** planning driven by the world model's causal edges +
  gap analysis (current state → desired state), i.e. the LifeOS intent layer
  feeding the planner.
- **Inspired by:** LifeOS, Google ADK.
- **Plugs into:** friday-planner public API (frozen).
- **Acceptance:** a plan can be generated *from* a desired-state gap, not only
  from a one-shot goal string.

#### Reflection — 🟡 (machinery real, loop unclosed)
- **What exists:** `ReflectionEngine::reflect(knowledge, outcome) → Option<Insight>`
  (`services/reflection/src/lib.rs:90`); `KnowledgeEngine::analyze(episodes) →
  KnowledgeRecord` + `PatternDetector` (`services/knowledge/src/lib.rs:57`);
  experience layer (recorder/miner/replay/suggestion/timeline/metrics).
- **Missing capability:** the closed loop — trajectory → outcome → reflection →
  pattern → candidate procedure → **verification** → skill → registry → reuse.
  Nothing mutates FRIDAY's own prompts/skills/registry from evidence.
- **Inspired by:** Multica, Prime Agent, LifeOS.
- **Plugs into:** `ReflectionEngine` + `SkillRegistry` + `skill-forge`.
- **Acceptance:** one verified successful trajectory yields a reusable skill in
  the registry, and a later goal uses it (see §3.3 skill compounding).

#### Context builder — 🟡 (exists, depth to verify)
- **What exists:** `ContextBuilder` assembles `AgentContext` from skills,
  `CoreMemorySource`, registry health, policy — `services/context/src/lib.rs:49`;
  `SkillDecisionCapability::decision_for(goal)` (`context/src/skills.rs:237`).
- **Missing capability:** verified end-to-end depth (does the assembled context
  actually prune to budget and exclude stale/irrelevant memory?); user-level
  preference/context sources are absent (LifeOS).
- **Inspired by:** LifeOS (user context model).
- **Plugs into:** `ContextBuilder` trait + `LLMContext`.
- **Acceptance:** `build(goal)` returns an `LLMContext` under the token budget
  with ranked capabilities + relevant memories, from real registry/memory.

### 3.3 Missing (🔴)

| Missing capability | Why it matters | Inspired by | Plugs into (frozen seam) | Acceptance |
|---|---|---|---|---|

| **Autonomous drive** | No self-directed continuous loop. Current cognitive loop is in-memory phases + proactive project briefing (`services/project-cognition/src/engine.rs:339`), not an unsupervised drive. Start with a Cognitive Tick (observe → evaluate → maybe act → sleep → repeat), then grow the decision function — not full active-inference math first. | ACOS active inference | cognitive-runtime phases + workspace events | A tick loop runs without a user goal, observes the workspace, and fires one action justified by an evidence trace. |
| **LifeOS intent layer** | Goals exist (`goal.rs`) but no persistent user-level current→ideal state, gap/opportunity detection, priorities, constraints, or progress tracking. | **LifeOS** | planner + intent router + context builder | `desired_state − current_state` produces objectives the planner can turn into plans; intent routing consumes user preferences. |
| **Persistent agent workforce** | Agents are ephemeral, in-process, per-goal. No persistent worker registry, assignable task board, isolated per-agent workspaces across runs, progress/blocker reporting. | **Multica** | `AgentRuntime` + `WorkflowStore` | A worker can be assigned a task, picked up across runs, report progress/blockers, and its workspace persists. |
| **Real perception (vision/audio/screen)** | Perception is synthetic RNG events (`services/engines/perception/src/lib.rs:14`), not sensing. No camera/video pipeline, no VAD/ASR/diarization/turn detection, no continuous screen diff. Small-specialist → event → core, never raw 30fps into an LLM. | **Vision Agents**, Meetily | `PerceptionEngine` → workspace events | A real sensor publishes an event to the workspace that a cognitive organ consumes (proof: one camera frame → object event; one mic stream → turn event). |
| **World-model visualization** | Deferred — the causal world model (§3.2) comes first; DrawDB is sugar. | DrawDB | `WorldModel` | None until causal model lands. |
| **Egress policy / secret scan** | No "scan payload before sending to an external model" gate. Should become a global outbound-data policy, not just a coding feature. | **SonarQube CLI**, Archestra | permission engine (outbound hook) | A payload containing a detected secret is blocked/redacted before the provider call; policy is enforceable per-provider. |
| **Secrets vault** | No secret store for tokens/keys. | — | permission + provider config | Secrets never appear in audit logs or memory; only capability-scoped access. |
| **Observability (OTel/Prometheus)** | No metrics/scrape endpoint. | Vision Agents, Archestra | — | A `/metrics` endpoint exposes runtime counters a Prometheus scrape consumes. |
| **Cost telemetry** | No per-model cost accounting ledger. | — | provider engine | Every model call is ledgered with model, tokens, cost; queryable per goal. |
| **Skill compounding** | `skill-forge` exists but no closed loop (§3.2 reflection). | Multica, Prime Agent | `SkillRegistry` + `skill-forge` | A verified successful trajectory becomes a skill used by a later goal. |

---

## 4. What must not enter core

Self-improvement must **never** mutate the FRIDAY core. Separation is absolute:

```text
Immutable Core          Adaptive Layer
 ├─ Identity             ├─ Skills
 ├─ Safety               ├─ Procedures
 ├─ Runtime invariants   ├─ Routing knowledge
 └─ Permission model     └─ Memory
```

- The core's identity, safety, runtime invariants, and permission model are
  frozen (Rule 1). No skill, reflection output, or learned procedure may alter
  them.
- Learning writes only to the Adaptive Layer (skills, procedures, routing
  knowledge, memory).
- External projects may inform the Adaptive Layer's design; they may **never**
  become core dependencies or wrap core execution.

---

## 5. Engineering priority — dependencies and verified gaps (revised 2026-08-10)

> **Engineering priority is determined by verified FRIDAY repository state, not
> by the novelty or importance of an external project. External projects are
> references or implementations behind existing FRIDAY seams.**

The audit invalidated the old project-by-project ranking: several "future
features" are **verified missing architectural capabilities**, while others
should wait because the underlying substrate already exists. This order tracks
the ACOS roadmap's separation — durable runtime, memory/world modeling,
distributed execution, then metacognition/autonomy — instead of treating every
capability as an equal-priority feature.

### Priority table

| Priority | Work                                     | Why now                                                              | Source/project                |
| -------- | ---------------------------------------- | -------------------------------------------------------------------- | ----------------------------- |
| **P0**   | **AnyDoc `docs.parse`**                  | Cleanest existing-seam integration — **DONE (2026-08-10)**           | AnyDoc                        |
| **P0**   | **Rust-native Web Resolver**             | Web resolver wired end-to-end (`web.search` + `web.fetch`) — **DONE (2026-08-10)** | Agent-Reach                   |
| **P1**   | **Concrete Verification Gate**           | **Local code/test verifier DONE (2026-08-10)**; Sonar/security/evidence verifiers remain additive behind the same seam | SonarQube + existing verifier |
| **P1**   | **Evidence/Research pipeline hardening** | **DONE (2026-08-10)** — AnyDoc + Web → unified `Evidence[]` → verification → episodic memory (`services/goal-execution/src/evidence_pipeline.rs`); remaining evidence hardening is additive | Agent-Reach + AnyDoc          |
| **P1**   | **Agent Director / Subagents**           | **DONE (2026-08-11)** — `AgentDirector` supervisor topology (Researcher/Analyst/Verifier) with gated result collection, wired through the real `SkillAgentDriver` | Prime Agent + Multica         |
| **P2**   | **Parallel durable workflows**           | **DONE (2026-08-11)** — parallel stages + write-ahead journaling + idempotent replay + mid-agent recovery (`services/goal-execution/src/workflow.rs`) | DeepZero + OpenMontage        |
| **P2**   | **Real World Model**                     | **DONE (2026-08-11)** — entity graph + causal edges + pure simulation (`services/goal-execution/src/causal.rs`); remaining: observed causal learning + LifeOS gap layer (#10) | ACOS architecture             |
| **P2**   | **LifeOS-style Intent Layer**            | Goals exist, but current→ideal/gap reasoning is missing              | LifeOS                        |
| **P2**   | **Skill Compounding Loop**               | Reflection + Skill Forge exist, but aren't closed-loop               | Multica + Prime Agent         |
| **P3**   | **Real Audio/Voice Sensor**              | Current perception isn't real sensing                                | Meetily                       |
| **P3**   | **Real Vision Sensor**                   | Vision organ is genuinely absent                                     | Vision Agents                 |
| **P3**   | **Persistent Agent Workforce**           | Requires Agent Director first                                        | Multica                       |
| **P3**   | **Autonomous Cognitive Tick**            | Requires perception + world state + goals + execution + safety       | ACOS / Active Inference       |
| **P4**   | **Walrus Memory backend**                | Existing `MemoryLayer` already solves the abstraction — backend #5, not architecture | Walrus |
| **P4**   | **Security/Egress/Secrets plane**        | Important before broad autonomy                                      | Archestra + Sonar             |
| **P4**   | **Observability + cost telemetry**       | Required for long-running autonomous operation                       | Archestra                     |
| **P5**   | **World-model visualization**            | Visualization is downstream of actually having a rich world model    | DrawDB                        |

### Dependency graph

Not a linear build order — a dependency graph. Runtime substrate first, then
the organs that plug into it, then the cognition that makes FRIDAY autonomous:

```text
                         FRIDAY vNext
                              │
                    ┌─────────┴─────────┐
                    │ Existing Runtime │
                    └─────────┬─────────┘
                              │
          ┌───────────────────┼──────────────────┐
          ↓                   ↓                  ↓
       AnyDoc             Web Resolver       Verification
          │                   │                  │
          └───────────────────┼──────────────────┘
                              ↓
                     Evidence / Research
                              │
                              ↓
                       Agent Director
                              │
                 ┌────────────┼────────────┐
                 ↓            ↓            ↓
             Researcher     Coder       Analyst
                 │            │            │
                 └────────────┼────────────┘
                              ↓
                     Durable Workflows
                              │
                  ┌───────────┴───────────┐
                  ↓                       ↓
             World Model             Intent Layer
                  │                       │
                  └───────────┬───────────┘
                              ↓
                       Skill Learning
                              │
                    ┌─────────┴─────────┐
                    ↓                   ↓
                 Audio               Vision
                    │                   │
                    └─────────┬─────────┘
                              ↓
                       Autonomous Tick
                              │
                              ↺
```

### Three categories, not a flat build order

- **Foundation** — things other subsystems depend on (verified substrate work):
  capability routing, verification, durable execution, world model, **Agent
  Director**.
- **Organs** — capabilities plugged into that foundation: AnyDoc, Agent-Reach,
  Vision, Audio, Walrus (backend #5 behind the existing `MemoryLayer`).
- **Cognitive expansion** — what makes FRIDAY progressively more autonomous:
  LifeOS intent, skill compounding, persistent workforce, autonomous tick.

### Two deliberate consolidations

**One Agent Director, not three integrations.** Prime Agent (recursive
spawning), Multica (persistence/workspaces), and Google ADK (workflow topology)
all point at the same missing subsystem — supervisor, peer-to-peer, hierarchical
and blackboard topologies. Build one FRIDAY subsystem:

```text
AgentDirector
    │
    ├── spawn()
    ├── assign()
    ├── supervise()
    ├── suspend()
    ├── resume()
    ├── cancel()
    ├── collect()
    └── terminate()
```

**One sensor abstraction, not three features.** Meetily, Vision Agents, and
screen/telemetry are not unrelated integrations — the sensor interface already
exists:

```text
Sensor
  │
  ├── AudioSensor
  ├── VisionSensor
  ├── ScreenSensor
  └── SystemSensor
       ↓
Perception Events
       ↓
Global Workspace
```

Each step is additive behind a frozen interface. No rewrite.

---

## 6. The filter — every future GitHub project must pass this

When a new project is discovered, answer **in this order**:

1. **What missing dimension does it actually close?** (check §2/§3 first — most
   "new" ideas map to something that exists or is already budgeted).
2. **Which frozen FRIDAY seam does it plug into?** If no seam exists and no
   frozen seam can accept it, it is out unless it *reduces* coupling
   (architectural-gravity rule).
3. **What must NOT enter core?** Extract only the pattern; never the repo, never
   its runtime, never its assumption that it IS the harness.
4. **What is the acceptance criterion?** (must be testable in this repo).
5. **Where does it land in §5?** Every accepted idea ranks into one of the
   P0–P5 tiers; nothing is "adopted" outside the order without an explicit
   re-ranking here.

A project that passes 1–2 with a real seam, a clean 3, a testable 4, and a slot
in 5 is adopted as a **reference pattern**. Anything else stays in the notes —
FRIDAY is not a museum of other people's architectures.
