# FRIDAY — Specification

`specs/` is the **source of truth** for FRIDAY's architecture. From M4.7 onward,
every subsystem gets a stable specification here **before** implementation. The
`docs/` directory remains as design history and implementation notes, but for new
work this directory supersedes it.

## Discipline

- **Spec first, implement after.** No subsystem code without a locked spec.
- **Contracts are frozen.** The event vocabulary (`events.md`) and the
  `friday-core` / `friday-planner` / `ActionAdapter` APIs are locked. New features
  must answer: *which event do they consume, which do they produce, which subsystem
  owns them?*
- **The LLM sits above the kernel, not inside it.** Intelligence (M5) is fed by the
  Context Builder and reasons over Capability Intelligence; it never sees plugin
  internals and never assembles its own context.

## Architecture

```
                USER

                  │
          Context Builder          (M4.10)

                  │
          Intelligence Layer        (M5, LLM above kernel)

                  │
          Planner Engine            (M3, four reasoning engines)

                  │
        Simulation Engine           (M4.7, virtual execution)

                  │
          Agent Kernel              (M4, execution/lifecycle/resources)

                  │
      Capability Intelligence        (M4.6, what can be done + by whom)

                  │
        Service Orchestrator         (M4.6.5, lifecycle/health)

                  │
              Services               (filesystem, git, terminal, windows, …)

                  │
      Windows / Browser / Git
```

## Subsystem → spec → crate

| Subsystem            | Spec                | Crate / home                         | Status   |
|---------------------|---------------------|--------------------------------------|----------|
| Events              | `events.md`         | `friday-planner/src/events.rs`       | locked   |
| Kernel (AgentKernel)| `kernel.md`         | `services/action-runtime`            | ✅ M4    |
| Memory              | `memory.md`         | `friday-core`                        | ✅ M2    |
| Planner             | `planner.md`        | `friday-planner`                     | ✅ M3    |
| Capabilities        | `capabilities.md`   | `services/capabilities`              | ✅ M4.6  |
| Services            | `services.md`       | `services/action-runtime/adapters`   | ✅ M4.5  |
| IPC (Windows bridge)| `ipc.md`            | `plugins/windows` + `windows-host`   | ✅ M4.5.2|
| Simulation          | `simulation.md`     | `services/simulation`                | ✅ M4.7  |
| Experience (Engine) | `experience.md`     | `friday-core`                        | ✅ M4.8  |
| Context Builder     | `context.md`        | `services/context`                   | ✅ M4.10 |
| Cognitive Engine    | `intelligence.md`   | `services/intelligence`              | ✅ M5    |
| Evaluation          | `evaluation.md`     | `services/evaluation` (new)          | 🚧 M5.5  |
| Knowledge Graph     | `knowledge_graph.md` | `friday-core`                        | 🚧 M5.6  |
| Reasoning Session   | `reasoning_session.md`| `services/intelligence`             | 🚧 M5.7  |
| Benchmark (QA)      | `benchmark.md`        | `services/benchmark`                 | ✅ QA    |
