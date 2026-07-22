# Dogfooding Observations — FRIDAY on FRIDAY

- **Started:** 2026-07-22
- **Repository:** `E:\project-friday` (129 commits)
- **Purpose:** Exercise the experience layer against real development patterns

## Template

For each task:

```
Task: [description]

↓

Observation: [what happened]

↓

Root Cause: [why it happened]

↓

Possible Improvement: [what could change]

↓

Priority: [high/medium/low]
```

---

## Observation 1: Add a test to the experience layer

**Task:** Add a new test to `experience_retrieval_tests.rs` verifying that `similar()` returns results for a query matching a recorded goal.

**Observation:**
- The experience layer has no memory of this specific task.
- `similar("add test experience retrieval")` returns low-similarity matches because no past goal has that exact title.
- Pattern mining finds no relevant patterns (test-writing isn't a repeated workflow).
- Suggestions: none (no patterns meet frequency threshold).
- Replay: not applicable (no prior test-writing workflow to replay).

**Root Cause:**
- The experience layer only knows about goals it has observed.
- Test-writing is a new task type with no history.
- The TF-IDF index has no vocabulary overlap between "add test" and existing goal titles.

**Possible Improvement:**
- For M13: consider "cold start" behavior — when no experience exists, the planner should fall back to rule-based planning without degradation.
- The current system handles this correctly (returns empty results, planner proceeds with fresh plan).

**Priority:** Low (system works correctly; this is expected behavior)

---

## Observation 2: Fix a clippy warning

**Task:** Fix the `collapsible_if` clippy warning in `planner/src/planning/planners/validation.rs:80`.

**Observation:**
- `similar("fix clippy warning")` returns no relevant matches.
- No past goal has a title containing "clippy" or "warning".
- The experience layer has no concept of "code quality" tasks.
- After recording the fix as a timeline entry, the next similar query would find it.

**Root Cause:**
- Clippy fixes are one-off tasks, not repeated workflows.
- The experience layer correctly identifies this as a novel task.

**Possible Improvement:**
- None needed. One-off tasks shouldn't generate patterns or suggestions.

**Priority:** Low

---

## Observation 3: Update documentation (ADR)

**Task:** Write a new ADR documenting a decision.

**Observation:**
- `similar("write ADR")` returns no matches (no past ADR-writing goal recorded).
- After recording several ADR-writing goals, pattern mining would find: `["filesystem.write", "git.add", "git.commit"]`.
- This pattern already exists from README updates — the capabilities overlap.
- Suggestions would recommend the write→add→commit workflow.

**Root Cause:**
- Documentation tasks share capability sequences with other file-editing tasks.
- The experience layer correctly identifies the workflow pattern even across different task types.

**Possible Improvement:**
- For M13: consider task-type tagging in the experience layer. "Documentation" vs "code" vs "test" could improve suggestion relevance.

**Priority:** Medium (would improve suggestion quality)

---

## Observation 4: Refactor a module (rename)

**Task:** Rename `foo` to `bar` across multiple files.

**Observation:**
- `similar("rename module")` returns no matches.
- This is a novel task with no history.
- The experience layer correctly returns empty results.
- The planner would need to generate a fresh plan: find files, edit each, run tests, commit.

**Root Cause:**
- Refactoring tasks are diverse and don't repeat in the same way.
- The experience layer correctly treats this as a new task.

**Possible Improvement:**
- None needed. Refactoring is inherently variable.

**Priority:** Low

---

## Observation 5: Add a feature (new capability)

**Task:** Add a new MCP server integration (e.g., a database MCP).

**Observation:**
- `similar("add MCP server")` would find past MCP integrations (M9, M10).
- The pattern `["transport.setup", "protocol.handshake", "adapter.register", "test.write"]` would be mined.
- Suggestions would recommend the MCP integration workflow.
- Replay would classify steps as reused (transport, protocol) and adapted (adapter, test).

**Root Cause:**
- MCP integrations are repeated workflows with similar capability sequences.
- The experience layer correctly identifies the pattern.

**Possible Improvement:**
- For M13: consider "workflow templates" — when a pattern is reliable (frequency ≥ 5, success ≥ 90%), the planner could receive it as a pre-structured plan rather than evidence.

**Priority:** Medium (would accelerate repeated integrations)

---

## Summary of Findings

| Finding | Priority | M13 Relevance |
|---------|----------|---------------|
| Cold start behavior works correctly | Low | No action needed |
| One-off tasks don't generate noise | Low | No action needed |
| Cross-task workflow patterns exist | Medium | Task-type tagging |
| Refactoring is inherently variable | Low | No action needed |
| Repeated integrations benefit from patterns | Medium | Workflow templates |

## Key Insight

The experience layer's biggest value is **not** for novel tasks. It's for **repeated workflows** — the kind of tasks you do over and over with slight variations:

- Write → add → commit (every feature)
- Read → write → add → commit (every file update)
- Write → add → commit → push (every PR)

These are the patterns that accumulate frequency and become reliable suggestions. The experience layer is designed for exactly this use case.

## M13 Planning Implications

Based on dogfooding observations:

1. **Planning Context Enrichment** should focus on:
   - Presenting similar past goals with their outcomes
   - Suggesting workflows from reliable patterns
   - Offering replay candidates for repeated tasks

2. **Retrieval evolution** should focus on:
   - Better title-to-title matching (not just TF-IDF)
   - Task-type awareness (documentation vs code vs test)

3. **Trust calibration** should wait for:
   - At least 50 real task observations
   - Clear patterns of suggestion acceptance/rejection
   - Measurable replay reuse rates

## Next Steps

- [x] Run 5 realistic dogfood scenarios (see `dogfood_scenarios.rs`)
- [ ] Run 10 more realistic tasks against the experience layer
- [ ] Record suggestion acceptance/rejection for each
- [ ] Measure replay reuse ratios
- [ ] Compile findings into M13 planning document

---

## Dogfood Results (2026-07-22)

### Scenario 1: Simulate 10 realistic development tasks

**Setup:** 3 MCP integrations, 5 clippy fixes, 2 ADR updates.

**Results:**
- Goals indexed: 10
- Patterns mined: Multiple (read-write-add-commit and transport-handshake-adapter-write-add-commit)
- Suggestions generated: Yes, with explainable reasons
- Key finding: Cross-task workflow patterns exist (clippy fixes and ADR updates share the same capability sequence)

**Observation:** The experience layer correctly identifies workflow patterns across different task types. A "clippy fix" and an "ADR update" use the same capabilities, so they produce the same pattern. This is correct behavior — the planner should see both as instances of the same workflow.

### Scenario 2: Query for new task

**Queries tested:**
- "Add a new feature" → found MCP integration patterns
- "Fix a warning" → found clippy fix patterns
- "Write documentation" → found ADR patterns (when they exist)

**Key finding:** TF-IDF similarity is diluted by capability terms, but the planner still receives evidence. The planner decides whether to use it.

**Observation:** The retrieval works correctly. The planner receives multiple pieces of evidence with varying similarity and trust scores. ADR-0009 Rule 2 holds: evidence is presented, not decisions.

### Scenario 3: Replay value for repeated workflows

**Exact reuse:** reuse_ratio = 1.0 (all 6 steps reused)
**Adapted (skip handshake, add push):** reuse_ratio = 0.83 (5 of 6 original steps kept)

**Key finding:** Replay correctly classifies steps as reused/adapted/skipped/new. Adapted workflows still have high reuse ratios.

**Observation:** The replay engine provides meaningful metrics. A planner can use `reuse_ratio()` to decide whether to adopt a stored workflow.

### Scenario 4: Applicability check for MCP integration

**All capabilities registered:** applicable = true
**Missing MCP transport:** applicable = false, 3 capabilities missing

**Key finding:** Applicability checks correctly identify missing capabilities before replay.

**Observation:** The planner receives structured information about why a workflow can't be replayed. This enables intelligent adaptation.

### Scenario 5: Trust for repeated success

**Setup:** 10 successful clippy fixes.

**Results:**
- Trust score: > 0.5 for repeated successes
- Repetition component: > 0.3 (significant)
- Outcome quality: > 0.8 (success = high quality)

**Key finding:** Trust components correctly reflect repeated success. The repetition factor grows with observation count.

**Observation:** The trust system is working as designed. Repeated success builds confidence. The planner can use `trust_components()` to understand why a match is trustworthy.

---

## Summary of Dogfood Findings

| Finding | Evidence | M13 Relevance |
|---------|----------|---------------|
| Cross-task workflow patterns work | Scenario 1: clippy + ADR share patterns | Task-type tagging could improve suggestions |
| Retrieval returns evidence, not decisions | Scenario 2: multiple matches with varying scores | ADR-0009 holds under realistic conditions |
| Replay classification is accurate | Scenario 3: reuse_ratio = 1.0 for exact, 0.83 for adapted | Replay metrics are meaningful |
| Applicability checks work | Scenario 4: missing capabilities correctly identified | Planner can adapt based on applicability |
| Trust reflects repeated success | Scenario 5: trust > 0.5, repetition > 0.3 | Trust calibration is ready for real data |

## Key Insight from Dogfooding

The experience layer's biggest value is **accumulated evidence**. After 10 tasks:
- Patterns emerge from repeated workflows
- Suggestions are generated from reliable patterns
- Trust scores reflect outcome history
- Replay provides meaningful reuse metrics

This is exactly the use case the architecture was designed for. The system benefits from history without depending on it.

## M13 Planning Implications (Updated)

Based on dogfooding results:

1. **Planning Context Enrichment** should:
   - Present similar past goals with outcomes and trust scores
   - Suggest workflows from reliable patterns (frequency ≥ 5, success ≥ 90%)
   - Offer replay candidates with reuse ratios
   - Include applicability checks before suggesting replay

2. **Retrieval evolution** should:
   - Consider separate title-only and capability-only indexes
   - Explore embedding-based retrieval for better semantic matching
   - Keep `ExperienceService` interface stable while evolving internals

3. **Trust calibration** should:
   - Use real dogfooding data to validate current weights
   - Consider adaptive weights only after 50+ real observations
   - Keep current static weights as baseline
