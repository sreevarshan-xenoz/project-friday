# ADR-0009: Experience Informs Planning

- **Status:** Accepted
- **Date:** 2026-07-22
- **Deciders:** FRIDAY architecture (M12.1)
- **Supersedes:** — (no prior planning-experience contract)
- **Superseded by:** —

## Context

ADR-0008 established the Experience Layer: an append-only timeline, recorder,
reflection, and (M12.1) retrieval via `ExperienceService`. ADR-0007 established
the execution runtime: `AgentKernel`, adapters, unified action execution.

The missing contract is **how these two meet**. Specifically:

1. Who may query experience?
2. What does experience return?
3. Who decides?

Without a formal contract, future code could introduce shortcuts — runtime
directly consulting experience, experience influencing execution order, or
experience emitting events. Each would violate the one-way architectural flow
established by ADR-0008.

## Decision

**Formalize the planning-experience boundary with three rules:**

### Rule 1: Who may query experience

```
Planner (and diagnostics) may query ExperienceService.
Runtime (AgentKernel, adapters, supervisors) may NOT.
```

The planner is the decision-maker. The runtime is the executor. Mixing these
roles creates circular dependencies and makes both harder to reason about.

```
Planner ──query──▸ ExperienceService
Runtime ──(never)──▸ ExperienceService
```

### Rule 2: What does experience return

```
ExperienceService returns Evidence, never Decisions.
```

`ExperienceMatch` contains:
- `similarity_score` — how alike is this?
- `trust_score` — how much should I trust this?
- `match_reason` — why did this match?
- `outcome` — what happened?
- `reflection` — what was learned?
- `artifacts` / `commit_hash` — what was produced?

It does **not** contain:
- "You should do X"
- "Skip planning"
- "Use this exact plan"
- "Execute this workflow"

Evidence is descriptive. Decisions are prescriptive. The planner consumes
evidence and makes its own decisions.

### Rule 3: Who decides

```
Always the planner.
```

The planner receives `PlanningContext` (which may include experience evidence)
and produces a `Plan`. Experience never produces plans. Experience never
modifies plans. Experience never bypasses planning.

```
PlanningRequest
    │
    ▼
ExperienceService  ──▸  Evidence (similar, outcome, trust, reflection)
    │
    ▼
PlanningContext (evidence + goals + constraints)
    │
    ▼
Planner (rule-based or LLM)
    │
    ▼
Plan
```

## Architecture

```
┌─────────────────────────────────────────────────────┐
│                    Planner                           │
│                                                      │
│  PlanningRequest ──▸ PlanningContext ──▸ Plan        │
│                         │                            │
│                         │  evidence from             │
│                         ▼                            │
│                  ExperienceService                   │
│                         │                            │
│              ┌──────────┴──────────┐                 │
│              │                     │                 │
│          Chronological         Semantic              │
│           Index                  Index               │
│              │                     │                 │
│              └──────────┬──────────┘                 │
│                         │                            │
│                  Timeline (truth)                    │
└─────────────────────────────────────────────────────┘
         ▲
         │  facts from
         │
┌─────────────────────────────────────────────────────┐
│                  Runtime (ADR-0007)                  │
│                                                      │
│  AgentKernel → Adapters → Events → Timeline          │
│  (never reads experience)                            │
└─────────────────────────────────────────────────────┘
```

### Data flow

```
Runtime produces facts
        │
        ▼
Timeline stores facts (append-only)
        │
        ▼
Indexer derives search structures
        │
        ▼
ExperienceService answers questions
        │
        ▼
Planner receives evidence
        │
        ▼
Planner produces plan
        │
        ▼
Runtime executes plan
```

This is a **one-way loop**: runtime → experience → planner → runtime.
The loop never short-circuits.

### What the planner receives

The planner receives an immutable `PlanningContext` assembled by a stateless
`PlanningContextBuilder` from an `ExperienceProvider` trait:

```rust
/// Immutable snapshot. No setters. No mutation after construction.
pub struct PlanningContext {
    pub goal: PlanningGoal,
    pub capabilities: Vec<String>,
    pub constraints: Vec<String>,
    pub evidence: ExperienceEvidence,
}

pub struct ExperienceEvidence {
    pub similar_experiences: Vec<ExperienceMatch>,
    pub previous_outcome: Option<ExperienceMatch>,
    pub successful_workflows: Vec<SuccessfulWorkflow>,
    pub workflow_suggestions: Vec<WorkflowSuggestion>,
    pub replay_candidates: Vec<ReplayCandidate>,
    pub reflections: Vec<ReflectionRecord>,
    pub provenance: Vec<EvidenceProvenance>,  // reserved for diagnostics
}
```

The builder depends on a trait, not the concrete service:

```rust
pub trait ExperienceProvider {
    fn similar(&self, goal_title: &str, max_results: usize) -> Vec<ExperienceMatch>;
    fn last_attempt(&self, goal_id: Uuid) -> Option<ExperienceMatch>;
    fn successful_workflows(&self, goal_title: &str) -> Vec<GoalRecord>;
    fn suggestions(&self) -> Vec<WorkflowSuggestion>;
    fn load_reflections(&self, goal_ids: &[Uuid]) -> Vec<ReflectionRecord>;
}
```

This enables unit testing with mocks and alternative evidence sources without
changing the builder or planner.

The planner accesses evidence through a `PlanningEvidenceView` — a read-only
advice layer with convenience methods:

```rust
pub fn evidence(&self) -> PlanningEvidenceView<'_>;

// PlanningEvidenceView provides:
//   has_previous_attempt()  → bool
//   had_previous_success()  → bool
//   had_previous_failure()  → bool
//   previous_outcome_summary()  → Option<String>
//   similar_count()  → usize
//   suggestion_count()  → usize
//   replay_candidate_count()  → usize
//   reflection_count()  → usize
```

The planner is free to ignore evidence. It may use `similar()` results,
or it may not. The contract is that evidence is **available**, not that
it is **used**.

## What this ADR is NOT

- Not a requirement to use experience evidence.
- Not a guarantee that experience will improve planning.
- Not a mandate to change the planner's internal logic.
- Not a contract about how experience is indexed or scored.

It is a **boundary contract**: who may talk to whom, and what may flow
across the boundary.

## Consequences

**Positive**

- Clear separation of concerns: runtime executes, planner decides, experience
  informs.
- Experience retrieval can evolve (TF-IDF → embeddings → Tantivy) without
  touching planner or runtime code.
- The planner can be replaced (rule-based → LLM → hybrid) without changing
  how experience is stored or queried.
- Debugging is straightforward: the data flow is one-way and every component
  has a single responsibility.

**Negative / trade-offs**

- The planner must explicitly query experience (it does not receive push
  notifications of relevant new experiences). This is intentional — push
  would create timing dependencies and coupling.
- Experience evidence may be stale or irrelevant. The planner must decide
  how to weight or ignore it. This is by design — the planner is the
  decision-maker.

## Alternatives considered

- **Runtime queries experience directly.** Rejected: creates circular
  dependency, violates "runtime executes, planner decides."
- **Experience pushes evidence to planner.** Rejected: creates timing
  coupling, makes the planner dependent on experience availability.
- **Experience emits planning events.** Rejected: violates ADR-0008
  invariant 2 ("experience never changes execution").
- **Experience produces plans directly.** Rejected: turns experience into
  a second planner, violating "who decides: always the planner."

## Follow-ups

- M12.2: Pattern mining (capability sequences → frequency/success/last-seen). ✅
- M12.3: Workflow suggestions (mined patterns → eligible suggestions). ✅
- M12.4: Replay-through-planner (applicability check → planner → execution). ✅
- M13: Planning Context Enrichment (ExperienceProvider trait, immutable
  PlanningContext, PlanningEvidenceView). ✅
