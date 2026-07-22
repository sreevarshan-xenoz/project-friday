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
pub struct PlanningContext {
    /// The goal the planner must produce a plan for.
    pub goal: Goal,

    /// Available capabilities (from registry). Immutable snapshot.
    pub capabilities: Vec<Capability>,

    /// Policy, risk, budget constraints from the user or system.
    pub constraints: Vec<String>,

    // ── Experience evidence ──────────────────────────────

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
| `similar_experiences` | "What happened before?" | Learn from history |
| `previous_outcome` | "Did this exact goal work?" | Decide to reuse or retry |
| `successful_workflows` | "What workflow worked?" | Adopt proven workflow |
| `workflow_suggestions` | "What patterns exist?" | Discover new workflows |
| `replay_candidates` | "Can I replay this?" | Reuse with adaptation |
| `reflections` | "What was learned?" | Avoid known mistakes |

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

```
ExperienceService          PlanningContextBuilder        Planner
─────────────────          ─────────────────────        ───────
similar()          ──▸     similar_experiences    ──▸   decides
last_attempt()     ──▸     previous_outcome       ──▸   decides
successful_workflows() ─▸  successful_workflows   ──▸   decides
suggestions()      ──▸     workflow_suggestions   ──▸   decides
                           replay_candidates      ──▸   decides
                           reflections            ──▸   decides

CapabilityRegistry         PlanningContextBuilder        Planner
──────────────────         ─────────────────────        ───────
list()              ──▸    capabilities           ──▸   decides

User/System                PlanningContextBuilder        Planner
──────────────             ─────────────────────        ───────
constraints          ──▸   constraints            ──▸   decides
```

### Ownership rules

1. **ExperienceService** owns evidence retrieval. It never builds `PlanningContext`.
2. **PlanningContextBuilder** owns assembly. It calls ExperienceService, CapabilityRegistry,
   and collects constraints. It produces an immutable `PlanningContext`.
3. **Planner** owns decisions. It receives `PlanningContext` and produces a `Plan`.
4. **No one else** touches `PlanningContext`. It's a read-only DTO.

```rust
pub struct PlanningContextBuilder {
    experience: ExperienceService,
    capability_registry: CapabilityRegistry,
}

impl PlanningContextBuilder {
    pub fn build(&self, goal: Goal, constraints: Vec<String>) -> PlanningContext {
        let similar = self.experience.similar(&goal.title, 5);
        let previous = self.experience.last_attempt(goal.id);
        let workflows = self.experience.successful_workflows(&goal.title);
        let suggestions = self.experience.suggestions();

        let replay_candidates = suggestions.iter()
            .filter_map(|s| {
                // find pattern, check applicability, compute reuse ratio
            })
            .collect();

        let reflections = similar.iter()
            .filter_map(|m| m.reflection.clone())
            .collect();

        PlanningContext {
            goal,
            capabilities: self.capability_registry.list(),
            constraints,
            similar_experiences: similar,
            previous_outcome: previous,
            successful_workflows: workflows,
            workflow_suggestions: suggestions,
            replay_candidates,
            reflections,
        }
    }
}
```

## Architectural boundaries (protected)

```
ExperienceService  ──▸  PlanningContextBuilder  ──▸  PlanningContext  ──▸  Planner
      │                         │                         │
      │ owns retrieval          │ owns assembly           │ owns decisions
      │ never builds context    │ never decides           │ never queries indexes
      │                         │                         │
      ▼                         ▼                         ▼
  Indexes/DB              Immutable DTO              Plan (output)
```

Two boundaries must never break:

1. **Planner never queries ExperienceService directly.** Always through PlanningContext.
   If the planner needs more evidence, the builder collects it. This keeps the
   planner stateless and the experience engine replaceable.

2. **ExperienceService never builds PlanningContext.** It answers questions.
   The builder assembles. This keeps retrieval concerns separate from assembly
   concerns.

## Implementation order

1. Define `PlanningContext` and `ReplayCandidate` types
2. Implement `PlanningContextBuilder::build()`
3. Update planner to accept `PlanningContext` instead of raw goal
4. Verify ADR-0007, ADR-0008, ADR-0009 invariants still hold
5. Dogfood: run scenarios with enriched context

## Success criteria

- Planner receives `PlanningContext` (not raw goal + manual queries)
- ExperienceService API unchanged (builder consumes it)
- All 116+ tests still pass
- Planner can ignore any field in `PlanningContext` without error
- Retrieval engine can be swapped without planner changes
