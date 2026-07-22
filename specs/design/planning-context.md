# Planning Context Enrichment (M13)

**Status:** Design note
**Date:** 2026-07-22
**Depends on:** ADR-0009, M12.1–M12.5, dogfooding results

## Purpose

`PlanningContext` is the single value object the planner receives. Everything the
planner needs to make a decision lives here. Nothing the planner doesn't need
lives here.

This note answers three questions:

1. What evidence belongs in `PlanningContext`?
2. What deliberately does **not** belong there?
3. Which component owns each field?

## 1. What belongs in PlanningContext

```rust
/// Immutable snapshot of everything the planner needs.
/// No setters. No mutation after construction.
pub struct PlanningContext {
    /// The goal the planner must produce a plan for.
    pub goal: Goal,

    /// Available capabilities (from registry). Immutable snapshot.
    pub capabilities: Vec<Capability>,

    /// Policy, risk, budget constraints from the user or system.
    pub constraints: Vec<String>,

    /// All experience-derived evidence, grouped together.
    pub evidence: ExperienceEvidence,
}

/// Experience-derived evidence. Separated from goal/constraints to make
/// obvious which parts originate from the current request vs. history.
pub struct ExperienceEvidence {
    /// Similar past goals with outcomes, trust, and reflection.
    /// From: ExperienceService::similar(&goal.title, N)
    pub similar_experiences: Vec<ExperienceMatch>,

    /// What happened the last time this exact goal was attempted.
    /// From: ExperienceService::last_attempt(goal.id)
    pub previous_outcome: Option<ExperienceMatch>,

    /// Workflows that succeeded for similar goals.
    /// From: ExperienceService::successful_workflows(&goal.title)
    pub successful_workflows: Vec<GoalRecord>,

    /// Suggested workflows from pattern mining.
    /// From: ExperienceService::suggestions()
    pub workflow_suggestions: Vec<WorkflowSuggestion>,

    /// Workflows eligible for replay, with applicability checks.
    /// From: ReplayEngine::check_pattern() for each suggestion
    pub replay_candidates: Vec<ReplayCandidate>,

    /// Reflections from similar past goals.
    /// From: ExperienceService::similar() → load reflections for each match
    pub reflections: Vec<ReflectionRecord>,
}

/// A replay candidate with its applicability result.
pub struct ReplayCandidate {
    pub pattern: Pattern,
    pub applicability: ApplicabilityResult,
    pub reuse_ratio: f64,  // pre-computed for ranking
}
```

### Why each field

| Field | Why it's here | Planner use case |
|-------|---------------|------------------|
| `goal` | The task to plan for | Always needed |
| `capabilities` | What tools/services exist | Feasibility check |
| `constraints` | Policy/risk/budget limits | Safety boundaries |
| `evidence.similar_experiences` | "What happened before?" | Learn from history |
| `evidence.previous_outcome` | "Did this exact goal work?" | Decide to reuse or retry |
| `evidence.successful_workflows` | "What workflow worked?" | Adopt proven workflow |
| `evidence.workflow_suggestions` | "What patterns exist?" | Discover new workflows |
| `evidence.replay_candidates` | "Can I replay this?" | Reuse with adaptation |
| `evidence.reflections` | "What was learned?" | Avoid known mistakes |

## 2. What deliberately does NOT belong

| Excluded | Why |
|----------|-----|
| TF-IDF vectors | Implementation detail behind ExperienceService |
| Embedding models | Implementation detail, will change |
| BM25 scores | Implementation detail, will change |
| Index internals | ExperienceService owns these |
| Runtime state | Planner doesn't execute, runtime does |
| Permission decisions | Runtime owns these (ADR-0007) |
| Telemetry/metrics | Observational, not planning-relevant |
| "You should do X" | Evidence, not decisions (ADR-0009 Rule 2) |

The planner never sees **how** evidence was obtained. It sees **what** evidence
was obtained. This protects the retrieval engine to evolve (TF-IDF → embeddings
→ Tantivy) without touching planner code.

## 3. Which component owns each field

### The trait boundary

```rust
/// Abstraction over evidence retrieval. Builder depends on this, not
/// the concrete ExperienceService. Enables unit testing with mocks and
/// alternative evidence sources without changing the builder.
pub trait ExperienceProvider {
    fn similar(&self, goal_title: &str, max_results: usize) -> Vec<ExperienceMatch>;
    fn last_attempt(&self, goal_id: Uuid) -> Option<ExperienceMatch>;
    fn successful_workflows(&self, goal_title: &str) -> Vec<GoalRecord>;
    fn suggestions(&self) -> Vec<WorkflowSuggestion>;
    fn load_reflections(&self, goal_ids: &[Uuid]) -> Vec<ReflectionRecord>;
}
```

### Dependency graph

```text
Planner
    ▲
    │
PlanningContextBuilder
    │
    ▼
ExperienceProvider (trait)
    ▲
    │
ExperienceService (impl)
```

Builder depends on trait. Service implements trait. Planner sees only the
finished `PlanningContext`.

### Builder responsibilities (intentionally narrow)

The builder should:

* query the provider
* normalize results
* filter obvious duplicates
* package evidence

The builder should **not**:

* rank plans
* choose workflows
* discard valid evidence because it "looks bad"

Those are planner decisions.

### Ownership rules

1. **ExperienceService** owns evidence retrieval. It implements `ExperienceProvider`.
   It never builds `PlanningContext`.
2. **PlanningContextBuilder** owns assembly. It depends on `ExperienceProvider` (trait),
   not the concrete service. It produces an immutable `PlanningContext`.
3. **Planner** owns decisions. It receives `PlanningContext` and produces a `Plan`.
4. **No one else** touches `PlanningContext`. It's a read-only snapshot.

```rust
pub struct PlanningContextBuilder<P: ExperienceProvider> {
    provider: P,
    capability_registry: CapabilityRegistry,
}

impl<P: ExperienceProvider> PlanningContextBuilder<P> {
    pub fn build(&self, goal: Goal, constraints: Vec<String>) -> PlanningContext {
        let similar = self.provider.similar(&goal.title, 5);
        let previous = self.provider.last_attempt(goal.id);
        let workflows = self.provider.successful_workflows(&goal.title);
        let suggestions = self.provider.suggestions();

        let replay_candidates = suggestions.iter()
            .filter_map(|s| {
                // find pattern, check applicability, compute reuse ratio
            })
            .collect();

        let reflection_goals: Vec<Uuid> = similar.iter()
            .filter_map(|m| m.outcome.as_ref().map(|o| o.goal_id))
            .collect();
        let reflections = self.provider.load_reflections(&reflection_goals);

        PlanningContext {
            goal,
            capabilities: self.capability_registry.list(),
            constraints,
            evidence: ExperienceEvidence {
                similar_experiences: similar,
                previous_outcome: previous,
                successful_workflows: workflows,
                workflow_suggestions: suggestions,
                replay_candidates,
                reflections,
            },
        }
    }
}
```

## Architectural boundaries (protected)

```
ExperienceService  ──▸  PlanningContextBuilder  ──▸  PlanningContext  ──▸  Planner
      │                         │                         │
      │ implements              │ depends on              │ owns decisions
      │ ExperienceProvider      │ ExperienceProvider      │ never queries indexes
      │ never builds context    │ never decides           │
      │                         │                         │
      ▼                         ▼                         ▼
  Indexes/DB              Immutable snapshot          Plan (output)
```

Three boundaries must never break:

1. **Planner never queries ExperienceProvider directly.** Always through PlanningContext.
   If the planner needs more evidence, the builder collects it. This keeps the
   planner stateless and the experience engine replaceable.

2. **ExperienceService never builds PlanningContext.** It answers questions.
   The builder assembles. This keeps retrieval concerns separate from assembly
   concerns.

3. **Builder depends on trait, not concrete.** `PlanningContextBuilder<P: ExperienceProvider>`
   enables unit testing with mocks and alternative evidence sources without
   changing the builder or planner.

## Implementation order

1. Define `ExperienceProvider` trait (mirrors existing `ExperienceService` API)
2. Implement `ExperienceProvider` for `ExperienceService`
3. Define `PlanningContext`, `ExperienceEvidence`, `ReplayCandidate` types
4. Implement `PlanningContextBuilder<P: ExperienceProvider>::build()`
5. Update planner to accept `PlanningContext` instead of raw goal
6. Verify ADR-0007, ADR-0008, ADR-0009 invariants still hold
7. Dogfood: run scenarios with enriched context

## Success criteria

- Planner receives `PlanningContext` (not raw goal + manual queries)
- Builder depends on `ExperienceProvider` trait (mockable, swappable)
- `PlanningContext` is immutable (no setters, snapshot after construction)
- ExperienceService API unchanged (builder consumes it through trait)
- All 116+ tests still pass
- Planner can ignore any field in `PlanningContext` without error
- Retrieval engine can be swapped without planner or builder changes
