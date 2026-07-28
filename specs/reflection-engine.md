# Reflection Engine Specification

**Status:** 🚧 Spec-first — implement against this file.
**Depends on:** `friday-core` (memory), `friday-planner` (mission history)
**Crate:** `services/reflection` — crate `friday-reflection`

## 1. Purpose

After every mission, FRIDAY should ask: *why did that happen?*

Not memory (which records what happened).
Not learning (which detects patterns across episodes).
**Reflection** — which analyzes why things happened and extracts lessons.

Reflection is the bridge between doing and learning. Without it, FRIDAY
records events and detects patterns but never asks *why*. Reflection
asks why, and stores the answer.

## 2. Memory vs Learning vs Reflection

| Component | When | What | Example |
|-----------|------|------|---------|
| **Memory** | During execution | Records what happened | "Docker build failed at step 3" |
| **Learning** | Periodically | Detects patterns across episodes | "Docker fails more on Mondays" |
| **Reflection** | After each mission | Analyzes why, extracts lessons | "Docker failed because Cargo.lock was corrupted → delete and rebuild" |

## 3. When reflection runs

Reflection triggers on:

| Trigger | What happens |
|---------|-------------|
| `mission.completed` | Full reflection on the completed mission |
| `mission.failed` | Failure analysis: why did it fail? |
| `runtime.checkpoint` (after recovery) | Recovery analysis: what fix worked? |
| `user.correction` | Correction analysis: what did we get wrong? |

Reflection runs **after** the mission, not during. It does not block
the next mission. It's a background process.

## 4. Reflection process

```
Mission finished
    │
    ▼
Load mission context
    ├── Goal and decomposition
    ├── Execution history (steps, results, timings)
    ├── Error logs and recovery records
    ├── Checkpoints (what was saved/restored)
    └── User feedback (if any)
    │
    ▼
Analyze
    ├── Time analysis: where did we spend the most time?
    ├── Failure analysis: where did failures cluster?
    ├── Recovery analysis: what fixes worked?
    ├── Efficiency analysis: could we have done this faster?
    └── Error pattern: is this error new or recurring?
    │
    ▼
Extract lessons
    ├── "This error = this fix" → procedural knowledge
    ├── "This approach works better" → workflow improvement
    ├── "This step is always slow" → optimization hint
    ├── "This dependency is fragile" → risk flag
    └── "We've seen this before" → reinforce existing knowledge
    │
    ▼
Store
    ├── Lessons → Memory (procedural store)
    ├── Patterns → Learning Engine (if pattern is recurring)
    ├── World Model updates (if environment facts changed)
    └── Emit reflection.completed with insights
```

## 5. Lesson types

```rust
pub enum LessonType {
    /// "This error = this fix" — direct mapping
    ErrorFix {
        error_pattern: String,
        fix_description: String,
        confidence: f64,
    },

    /// "This approach works better than that" — workflow improvement
    WorkflowImprovement {
        what_was_done: String,
        what_worked_better: String,
        time_saved_ms: Option<u64>,
    },

    /// "This step is always slow" — optimization hint
    OptimizationHint {
        step_description: String,
        average_time_ms: u64,
        suggested_approach: String,
    },

    /// "This dependency is fragile" — risk flag
    RiskFlag {
        component: String,
        failure_rate: f64,
        impact: String,
    },

    /// "We've seen this before" — reinforce existing knowledge
    Reinforcement {
        existing_lesson_id: String,
        new_occurrence: bool,
        confidence_increase: f64,
    },
}
```

## 6. Reflection output

```rust
pub struct ReflectionResult {
    pub mission_id: String,
    pub completed_at: DateTime<Utc>,
    pub duration_ms: u64,
    pub success: bool,
    pub lessons: Vec<Lesson>,
    pub time_breakdown: HashMap<String, u64>,  // step → time spent
    pub failure_points: Vec<FailurePoint>,
    pub recovery_attempts: Vec<RecoveryAttempt>,
    pub overall_assessment: String,  // one-sentence summary
}

pub struct Lesson {
    pub id: String,
    pub lesson_type: LessonType,
    pub confidence: f64,       // 0..1
    pub source_mission: String,
    pub stored_in: String,     // "memory.procedural" or "knowledge.learned"
}
```

## 7. Integration with other engines

### 7.1 With Memory Engine

Lessons are stored as procedural memories:
```
Memory.procedural.store("Corrupted Cargo.lock → delete and rebuild")
```

This means the next time the Planner sees a Cargo.lock error, it can
recall the procedural memory and apply the fix without re-discovering it.

### 7.2 With Learning Engine

If reflection detects a recurring pattern, it emits to the Learning Engine:
```
Learning.pattern.detected("Cargo.lock corruption happens after force push")
```

The Learning Engine then tracks this pattern across multiple missions.

### 7.3 With World Model

If reflection discovers environment facts changed, it updates the World Model:
```
World.node.updated("Docker", { rootless: true })
```

### 7.4 With Planning Engine

Reflection results feed back into planning:
- Optimization hints → Planner can choose faster approaches
- Risk flags → Planner can add defensive steps
- Error fixes → Planner already knows the fix

## 8. Events

| Event | Payload | Producer |
|-------|---------|----------|
| `reflection.started` | `{ mission_id }` | ReflectionEngine |
| `reflection.completed` | `{ mission_id, lessons, assessment }` | ReflectionEngine |
| `reflection.lesson` | `{ lesson_id, type, confidence }` | ReflectionEngine |
| `reflection.risk.detected` | `{ component, failure_rate }` | ReflectionEngine |
| `reflection.optimization` | `{ step, suggestion }` | ReflectionEngine |

## 9. Metrics

```rust
pub struct ReflectionMetrics {
    pub total_reflections: u64,
    pub lessons_extracted: u64,
    pub lessons_by_type: HashMap<LessonType, u64>,
    pub average_reflection_time_ms: f64,
    pub lessons_reinforced: u64,    // existing lessons confirmed
    pub lessons_overridden: u64,    // old lessons proven wrong
    pub risk_flags_generated: u64,
    pub optimization_hints: u64,
}
```

## 10. Acceptance criteria

- [ ] `friday-reflection` builds clean (clippy/fmt/tests)
- [ ] Reflection runs after mission.completed and mission.failed
- [ ] Time breakdown accurately tracks step durations
- [ ] Failure analysis identifies failure points correctly
- [ ] Error-fix lessons are extracted from recovery attempts
- [ ] Lessons are stored as procedural memories
- [ ] Recurring patterns are forwarded to Learning Engine
- [ ] Environment changes update World Model
- [ ] Reflection does not block the next mission (background)
- [ ] Test: mission with 3 steps, 1 failure, 1 recovery → reflection produces 2+ lessons

## 11. What this unlocks

- **Self-awareness:** FRIDAY knows why things happened, not just what
- **Faster recovery:** error-fix lessons prevent re-discovery
- **Optimization:** time analysis reveals bottlenecks
- **Risk detection:** fragile components are flagged early
- **Continuous improvement:** every mission makes the next one better
- **Explainability:** "I did X because Y" — FRIDAY can explain its choices
