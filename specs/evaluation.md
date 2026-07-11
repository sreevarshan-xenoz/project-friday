# Evaluation & Benchmarking Specification (M5.5)

**Status:** 🚧 Planned. **Spec-first:** authoritative spec for FRIDAY's cognitive benchmarking.

To ensure that changes to prompts, routers, and parser rules improve FRIDAY rather than introducing regressions, we require a deterministic, offline evaluation engine.

```
Task Corpus ──> Scenario Injector ──> Cognitive Engine ──> Metrics Evaluator ──> Quality Score
```

## Subsystem Architecture

The evaluation framework will reside in a new package `friday-evaluation` (`tests/benchmarks/` or `services/evaluation/`) and consist of:

### 1. Golden Test Corpus
An offline database of benchmark tasks spanning desktop operations, filesystem actions, windows control, and complex coding workflows.

```rust
pub struct BenchmarkTask {
    pub task_id: Uuid,
    pub category: String, // "coding", "windows", "filesystem"
    pub goal: String,
    pub expected_intent: String,
    /// Expected topological nodes and dependency relations
    pub expected_plan: ExpectedTaskGraph,
}
```

### 2. Scenario Injector
Simulates system degradations by mutating the mock `WorldState` before running the planner and simulator:
- **Offline Mode**: Set `service_health.git = Offline`, `service_health.network = Offline`.
- **Service Missing**: Empty the registry's providers for `browser.open`.
- **Disk Full / Read-Only**: Set `filesystem.writable = false` or `resources.disk_bytes = 0`.

Assesses whether the planner selects correct fallback paths (e.g. browser -> windows) and raises the expected user approval warning.

### 3. Replay Engine
Reads historical experiences from the `ExperienceEngine` (`friday-core` procedural memory) and replays the original user goals against modified prompts or model routers to measure:
- Timing drift regressions (e.g. does a new model compile slower?).
- Inferences success improvements.

### 4. Metrics Evaluator
Aggregates quality statistics across benchmark runs:

| Metric | Calculation | Goal |
| :--- | :--- | :--- |
| **Intent Accuracy** | `Matches / Total` | > 98% |
| **Planning Accuracy** | Graph edit distance / task dependency match | > 95% |
| **Simulation Timing Drift** | `|(Actual - Predicted)| / Predicted` | < 10% |
| **Recovery Rate** | `Healed Subgoals / Failed Subgoals` | > 85% |
| **Hallucination Rate** | `Invalid Capabilities / Total Tasks` | 0% |

## Acceptance Criteria
- Running `cargo bench` or `cargo test --bench cognition` runs the Golden Test Corpus.
- Output reports a detailed cognitive accuracy scorecard.
- Regressions in planning accuracy or intent classification fail the build.
