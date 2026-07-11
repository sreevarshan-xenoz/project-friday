# Release 0.1 Vertical Slice Specification (Phase A)

**Status:** 🚧 Planned. **Spec-first:** authoritative spec for the Release 0.1 Vertical Slice.

To demonstrate that the entire stack (from Context Builder and Cognitive Engine to the Simulation Engine, Kernel, and Experience Engine) cooperates seamlessly, we implement a Vertical Slice End-to-End (E2E) Harness and a Developer Console.

```
Goal ──> Context ──> Router/Compiler ──> Model ──> Parser ──> Simulation ──> Kernel ──> Experience
```

## Subsystem Architecture

The Vertical Slice will reside in the `friday-e2e` crate (`e2e/`) and contain three main components:

### 1. End-to-End Harness (`runner.rs`)
Executes JSON/YAML-defined scenarios containing:
- User goal
- Expected intent/actions
- Expected pre-flight checks
- Final filesystem/state changes

Harness verifies:
1. **Pipeline Continuity**: Every component receives inputs and produces outputs without panic.
2. **Deterministic Output**: The planner outputs the expected task sequence.
3. **Execution Success**: The kernel successfully dispatches actions to adapters.

### 2. Developer Console (`console.rs`)
A CLI-based debugging tool (`friday-dev-console`) that displays live execution stages:
```
FRIDAY Developer Console
────────────────────────
Goal: "Create Research folder, init git, add README"
Context: Loaded (15 Capabilities, 1 Memory, 0 Experiences)
Planner: Generated 4 tasks
Simulation: Passed (96% Confidence, 0 Warnings, 1 Optimizer Collapse)
Execution: 
  - Task 1: filesystem.create_dir ✔ (12ms)
  - Task 2: git.init            ✔ (280ms)
  - Task 3: filesystem.create_file ✔ (8ms)
  - Task 4: git.commit          ✔ (190ms)
Experience: Telemetry recorded to redb table.
```

### 3. Trace Replay Engine (`trace.rs`)
Captures a detailed execution log (`ExecutionTrace`) containing the complete serializable output of each stage:
- Input Goal
- Package `AgentContext`
- Compiled prompt string
- Parser output (`LlmResponse`)
- Optimized `TaskGraph`
- Step-by-step audit logs & adapter metric durations

Allows engineers to save this trace file to disk (`trace_xxx.json`) and replay it offline (re-feeding the context and prompt to the parser) to isolate regressions.

## Crate Layout (`e2e/`)
```
e2e/
├── Cargo.toml
├── src/
│   ├── main.rs         # Developer Console entrypoint
│   ├── runner.rs       # E2E Scenario Runner
│   ├── trace.rs        # Trace recorder and Replay Engine
│   └── lib.rs
├── scenarios/
│   └── git_research.json
└── tests/
    └── e2e_tests.rs    # Vertical slice E2E test runs
```
