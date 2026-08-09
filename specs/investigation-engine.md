# Investigation Engine Specification

**Status:** 🚧 Spec-first — implement against this file (v1 shipped 2026-08-08).
**Depends on:** none (pure data; `serde` only). Consumed by the planner's
`PlanningContext` seam (§8).
**Crate:** `services/investigation` — crate `friday-investigation`

## 1. Purpose

The planner decides *how to execute*. FRIDAY also needs a component that
decides **what information is required before execution** — the difference
between an assistant (reacts to the goal) and a cognitive architecture (checks
what it does and does not know first).

```text
Goal
  │
  ▼
InvestigationEngine   (what do I need to know?)
  │
  ▼
InvestigationPlan     (channels + rationale + confidence)
  │
  ▼
Execution graph       (investigate → plan → execute)
  │
  ▼
Planner → Runtime
```

### 1.1 Core-abstraction rule

The engine owns **epistemic strategy** and nothing else. It produces data
([`InvestigationPlan`]) and a traceable view ([`InvestigationGraph`]); it does
not execute, route, or hold state. The architectural-gravity question —
"new core abstraction, or just another feature?" — answers *core abstraction*:
it is the first owner of *what FRIDAY needs to know*, complementing the
planner (what to do), skills (how), and personas (expression).

## 2. Channels

| Channel | Meaning | Suggested capabilities |
|---|---|---|
| `Web` | External knowledge | `web.search`, `web.fetch` |
| `Docs` | Project/technical docs | `docs.search`, `docs.read` |
| `CodeIntel` | Understand the current repo | `codeintel.understand` |
| `Memory` | Long-term recall | `memory.recall` (may be unregistered until the memory API lands — advisory) |
| `Git` | Repo history / working tree | `git.status`, `git.log` |
| `Filesystem` | Local files/dirs | `filesystem.list_directory`, `filesystem.read_file` |

## 3. Types

```rust
pub struct InformationRequirement {
    pub channel: InvestigationChannel,
    pub rationale: String,   // which keyword(s) triggered it — auditable "why"
    pub confidence: f32,     // 0.0..=1.0
}

pub struct InvestigationPlan {
    pub requirements: Vec<InformationRequirement>, // highest confidence first
}

// The always-present outcome of investigation — never an `Option` on the
// planning path.
pub enum InvestigationResult {
    Complete(InvestigationPlan), // empty plan = goal is self-contained
    Skipped,                     // investigation deliberately not run
    Failed { reason: String },   // strategy could not produce a plan (planner folds to direct)
}

pub struct InvestigationGraph {
    pub goal: String,
    pub requirements: Vec<InformationRequirement>,
    pub phases: Vec<String>, // ["plan","execute"] or ["investigate","plan","execute"]
}
```

## 4. Strategy

`InvestigationStrategy` is a pluggable, **async** trait (a model call is
genuinely async; a sync trait would force a blocking hack in the strategy
layer):

```rust
#[async_trait]
pub trait InvestigationStrategy: Send + Sync {
    async fn investigate(&self, goal: &str) -> InvestigationPlan;
}
```

### 4.1 Default: heuristic (deterministic)

`HeuristicInvestigationStrategy`: keyword tables per channel (longer phrases
weigh more), score capped at 1.0, threshold `min_confidence` (default 0.25),
deterministic ordering (confidence desc, channel-order ties). The pure core
stays a sync `compute()` so deterministic baselines never need a runtime.

### 4.2 Drop-in: LLM-backed with heuristic fallback

`LlmInvestigationStrategy` — the model derives the requirements; the
heuristic is the fallback whenever the model call fails, returns unparseable
output, or produces no valid requirements. **A failed model is never a failed
investigation.**

- The strategy depends only on `LlmStrategyClient` (one async `complete`),
  defined in this crate so the strategy layer stays model-agnostic — the
  Inference Fabric implements it at the wiring point.
- The prompt is a strict JSON contract (channel vocabulary, `confidence`
  0..1, empty `[]` = self-contained goal). Parsing is deliberately tolerant:
  code fences and surrounding prose are stripped; both
  `{"requirements":[…]}` and a bare `[…]` are accepted; hallucinated
  channels are dropped; `confidence` clamps to 0..1 (missing → 0.5,
  non-finite → entry dropped); duplicate channels keep the strongest entry;
  the list is capped (default 8) and ordered like the heuristic.
- Fallback is observable: `[Investigation Strategy] strategy=llm → heuristic
  / reason=model_error|unparseable|no_valid_requirements / goal="…"`.

```rust
let engine = InvestigationEngine::llm(client);   // LLM, heuristic fallback
let engine = InvestigationEngine::heuristic();   // deterministic default
```

## 5. Engine

```rust
let engine = InvestigationEngine::heuristic();   // deterministic default
let engine = InvestigationEngine::llm(client);   // model-backed, heuristic fallback
let plan = engine.investigate(goal).await;       // what is required
let graph = engine.graph(goal).await;            // watch-FRIDAY-think view
```

## 6. Invariants

1. The engine executes nothing, routes nothing, holds no state.
2. The plan is deterministic for a given **deterministic** strategy + goal
   (the heuristic); the LLM strategy validates and orders its output
   deterministically, but the model itself is stochastic.
3. A self-contained goal ("open notepad") produces an empty plan.
4. Requirements are ordered by confidence (ties break on stable channel order).
5. All suggested capabilities are advisory; the planner resolves registration.
6. A model failure falls back to the heuristic — never a failed investigation.
7. The planning path always carries `InvestigationResult` — never an
   `Option<InvestigationPlan>`; `Failed` folds to direct planning (a failed
   investigation is never a failed goal).

## 7. Acceptance criteria (v1 shipped)

- [x] `friday-investigation` builds clean (fmt/clippy/tests) — 18 unit + 1 doc test.
- [x] Web/Docs/CodeIntel/Memory/Git/Filesystem detection proven.
- [x] Multi-channel goals detected (git + codeintel).
- [x] Self-contained goals produce empty plans.
- [x] `min_confidence` filtering proven.
- [x] Strategy pluggability proven (custom strategy test).
- [x] `InvestigationGraph` phase insertion proven.
- [x] JSON round-trip proven.
- [x] `describe()` narration proven (`web (0.90), git (0.80)` / `none`).
- [x] LLM strategy is a drop-in for the heuristic (`InvestigationEngine::llm`)
      with heuristic fallback — 34 unit + 1 doc test in `friday-investigation`
      (parsing tolerance, validation, fallback triggers, dedup, cap).
- [x] The Inference Fabric implements `LlmStrategyClient` (free-first
      requirement routing); executive drop-in proven end-to-end.
- [x] **Real evidence adapters** (§10): `AdapterTool` runs evidence actions
      against the action-runtime adapters — real `git status` (temp repo)
      and real `web.search` (hermetic local endpoint) proven, plus honest
      refusal of non-evidence actions; evidence-gated plans execute the real
      adapter through `GoalEngine` (`friday-goal-execution` 206 unit + 12
      integration + 1 doc tests).

## 8. Wiring seam — M7 shipped (planner integration)

`PlanningContext` (in `services/goal-execution`) now carries
`investigation: InvestigationResult` — **always present, never an `Option`**
(`Complete(InvestigationPlan)` / `Skipped` / `Failed { reason }`; a plain
plan converts via `From`, and `with_investigation()` accepts either). The
runtime (`Executive::cycle`) invokes `InvestigationEngine::investigate` on
every focused goal **before** planning, wraps the plan in `Complete`, and
hands the result to the planner. `Skipped` is the default for directly-built
contexts; `Failed` folds into the direct plan (a failed investigation is
never a failed goal) and the executive narrates it — today no strategy can
fail (the heuristic always produces a plan, the LLM strategy falls back), so
`Failed` is the forward seam.
The planner consumes it:

- `Planning::think()` adds an **`PlanSource::Investigation` candidate** when
  the goal requires evidence: one evidence-gathering action per required
  channel (instruction carries the real skill capability id, e.g.
  `gather evidence: git (git.status)`, carried as `Capability::Custom`
  (see §10) — never a built-in capability, so evidence actions can't hijack
  the terminal/web tools in the first-match registry), followed by the
  goal's own action. Confidence = **mean evidence strength**.
- The `PlanEvaluator` ranks it against the direct plan: **strong evidence
  wins** (a web+git goal gets an evidence-gated plan; the evidence action
  executes first through the tool registry), **weak evidence folds** into the
  direct plan — the planner's judgment, not an unconditional phase.
- The executive narrates the decision in the thought log (`executive:
  investigation required before planning: web (1.00), docs (0.60)`; or
  `no investigation required — goal is self-contained`).

The strategy seam is **async** (see §4): `Executive::investigate_for` awaits
it, and the bridge's `RuntimeSource` is async so a live daemon can surface
an LLM-backed investigation graph through the same seam. Real evidence
adapters are shipped (§10). The **LLM planner's prompt** in the `planner/`
crate is wired too: `PlanContext.investigation` carries the always-present
`InvestigationResult` and `LlmPlanner` renders an **EVIDENCE REQUIRED**
section (channels with rationale + confidence) joining RELEVANT SKILLS —
see §8a. The engine itself stays decoupled and deterministic.

## 8a. LLM planner prompt — EVIDENCE REQUIRED section (shipped)

The `planner/` crate's LLM planner now reasons from investigation too,
joining the RELEVANT SKILLS section (M5) with an **EVIDENCE REQUIRED**
section. In the cognitive runtime, the evidence gate is also its own
**cycle phase**: `ExecutivePhase::Investigate` sits between Attend and Plan
and is recorded in `phases_completed` for focused goals, and the
`CognitiveTrace` carries a `TraceNodeKind::Investigation` node (channels,
confidence, self-contained flag) so the audit trail shows the gate exactly
when it fires — see §8b for the executive-side wiring.

```text
EVIDENCE REQUIRED (consult these channels before executing; confidence =
evidence strength):
- web (0.90): goal mentions 'search the web'
- git (0.80): goal mentions 'git'
```

Wiring mirrors the skill seam exactly:

- `PlanContext.investigation: Option<InvestigationResult>` — the always-
  present outcome (Complete/Skipped/Failed); `None` when no seam is wired.
- `PlanningEngine::with_investigation_engine(InvestigationEngine)` — the
  engine runs `investigate` for **LLM-routed goals only** (same gate as
  skills: the rule planner ignores the field; a model-backed strategy is
  not free). The result wraps in `Complete`, so `Skipped`/`Failed` are
  exercised by tests, not production. **Cost:** `investigate` runs once per
  LLM-routed goal — free for the heuristic, a model call per LLM-routed
  goal for `LlmInvestigationStrategy` (inherent to the seam; do not skip
  the call based on the rendered section).
- `LlmPlanner::format_investigation` renders the section only when
  `Complete(plan)` with requirements; an empty plan (self-contained goal),
  `Skipped`, `Failed`, or an absent seam render nothing — the prompt stays
  clean.
- Boot: `PolicyBinding` gains `investigation: Option<InvestigationEngine>`,
  wired into the planning engine by `register_engines_with_policy`.

## 8b. Executive cycle — the Investigate phase (shipped)

In the cognitive runtime (`services/goal-execution`), the evidence gate is a
first-class step of the executive cycle, not a hidden sub-step of planning:

- `ExecutivePhase::Investigate` sits between `Attend` and `Plan`. The cycle
  enters it (and records it in `phases_completed`) for **Goal and
  Observation focus targets only** — Idle and continuing states have nothing
  to investigate, so the audit trail never claims a gate that did not fire.
- The investigation result feeds `PlanningContext` exactly as before; the
  rest of the cycle (Plan → Simulate → Execute → Verify → Reflect →
  Consolidate → Learn → Idle) is unchanged.
- The `CognitiveTrace` carries the gate as a `TraceNodeKind::Investigation`
  node (goal, `self_contained`, `requirement_count`, `channels`, mean
  confidence) emitted by the executive's `planning_trace`, preceding the
  `Planning` node — the full trace reads investigation → planning →
  execution → verification → reflection.

The console mirror follows: the bridge's `PHASE_ORDER` and the frontend's
`ExecutivePhaseName`/`EXEC_ORDER` gained `Investigate` (ten-phase cycle →
eleven-phase), and the Execution Graph screen's `investigate` window now
maps to the real phase (index 2) instead of Observe→Attend.

## 9. Execution-graph visualization (shipped)

`InvestigationGraph` now has a live viewer — the Engineering Console's
**Execution Graph** screen (`frontend/src/screens/ExecutionGraphScreen.tsx`):

- **Goal node** → **evidence branches** (channel icon, confidence bar,
  `rationale` chips) → **phase rail** (`investigate → plan → execute`), with
  each phase's live status derived from the executive cycle (`investigate`
  runs during Observe→Attend, `plan` during Plan→Simulate, `execute` during
  Execute→Idle).
- **Live by construction**: the mock source and the bridge's demo source both
  cycle the same four `DEMO_GOALS` (web+docs / git+codeintel+filesystem /
  memory recall / self-contained) one per executive cycle; the graph
  re-renders with staggered reveals, animated confidence bars, and a pulsing
  running-phase ring. The frontend mirror (`investigationGraph()` in
  `frontend/src/console/mock.ts`) re-implements the heuristic exactly
  (word-boundary keywords, min confidence 0.25, capped per-channel sums) so
  mock, bridge, and engine agree.
- **Wire shape**: `DashboardDto` now carries
  `graph: Option<InvestigationGraphDto>` (`InformationRequirementDto` +
  `InvestigationGraphDto` in `services/bridge/src/protocol.rs`, mirrored
  field-for-field in `frontend/src/console/types.ts`). The real bridge
  already emits it via `DemoRuntimeSource` (which runs
  `InvestigationEngine::heuristic` on the current goal) — a real daemon only
  swaps the source.

The core engine remains decoupled: the visualization consumes the same pure
`InvestigationGraph` data the planner consumed in M7. The demo source still
uses the deterministic heuristic (its graphs are seeded demo data mirrored by
the frontend mock); an LLM-backed graph is a drop-in at the source seam.

## 10. Real evidence adapters (shipped)

"Which tools back `web.search` / `git.status`?" — the last M7 seam — is now
answered by `services/goal-execution/src/evidence.rs`, and the answer is
*the action-runtime adapters*.

**Routing model.** Evidence actions carry their **real skill capability id**
as `Capability::Custom("git.status")` / `Custom("web.search")` (the same id
the instruction carries — audit trail and registry key agree), instead of
collapsing into a built-in capability. Rationale: an evidence action is *not*
an arbitrary `execute_command`; routing it through `ExecuteCommand` would let
a git-backed tool hijack every command action in the first-match registry.
The `Capability::Custom` variant is the precise key.

**`AdapterTool`** implements the goal-execution `Tool` trait over one
`friday-actions::ActionAdapter`: it advertises exactly the adapter's verbs as
`Custom("<tool>.<verb>")`, parses the evidence id from the instruction
(`parse_evidence_id`: `gather evidence: git (git.status)` → `git`+`status`),
forwards an `ActionRequest` (with `query` from the goal, the context's
working directory as `repo`, timeout, permissions), and maps the
`ActionResult` back to a `ToolResult`. Non-evidence instructions are refused
with an honest error — the tool never guesses what an arbitrary command
means. `register_evidence_adapters(registry, &[Arc<dyn ActionAdapter>])`
registers one tool per adapter.

**Live by construction.** `gather evidence: git (git.status)` runs the real
`git status --porcelain` process; `gather evidence: web (web.search)` runs a
real search against the DuckDuckGo HTML endpoint (override via `endpoint`,
so tests stay hermetic on a local server). Proven end-to-end: an
evidence-gated plan for a git goal executes its `git.status` evidence action
through `GoalEngine` against a real temp repo, and the porcelain output
reaches the trace.

**Scope note (honest).** The executive cycle executes one action per cycle
(`plan.actions.first()`), so through the executive only the *first* evidence
step of a multi-channel plan runs; the live proof above drives `GoalEngine`
directly. Advancing multi-action plans through the executive is a separate
concern (a plan executor), not this seam.

**Registry note.** The tool registry is first-match. Evidence tools never
advertise built-in capabilities (they cannot hijack command/search tools),
but if a caller registers another tool that also advertises the same
`Custom` id, registration order decides — register evidence adapters
deliberately.

**Ownership stays clean.** The investigation crate still knows nothing about
tools; the planner emits the id; goal-execution owns the bridge (the `Tool`
trait lives there); action-runtime owns the real adapters. One new dependency
edge: `friday-goal-execution → friday-actions` (no cycle — action-runtime
does not depend on goal-execution).
