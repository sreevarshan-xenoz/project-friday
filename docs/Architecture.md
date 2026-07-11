# Architecture

FRIDAY is **event-driven from day one**. Modules never call each other directly;
they communicate exclusively through the event bus and the protocol transport.
This keeps the system modular and makes adding capabilities a matter of dropping
in a new plugin rather than rewriting the application.

## Final architecture

```
                   USER
                     │
          Wake Word Engine
                     │
          Speech Recognition
                     │
          Conversation Engine
                     │
             Goal Interpreter
                     │
            Planning Engine
                     │
             Event Bus
                     │
      ┌──────────────┼──────────────┐
      │              │              │
 Vision        Windows Skill     Browser
      │              │              │
 Terminal        Filesystem      Coding
      │              │              │
      └──────────────┼──────────────┘
                     │
              Verification
                     │
              Memory Engine
                     │
               Knowledge Graph
                     │
              Plugin Manager
                     │
                 Voice Reply
```

## Language allocation (use each where it wins)

| Component           | Language      | Reason                              |
| ------------------- | ------------- | ----------------------------------- |
| Core Engine         | Rust          | Performance, safety, no GC pauses    |
| AI Orchestrator     | Python        | Best ML/LLM ecosystem                |
| Windows Integration | C#            | First-class Win32 / COM / UWP access |
| Desktop UI          | React + Tauri | Small, fast, native WebView          |
| Plugins             | Python / Rust | Author in whatever fits the job     |
| Memory Engine       | Rust          | Low-latency, durable                 |
| Vision              | Python        | Mature CV / multimodal tooling       |
| Installer           | Rust          | Single small native binary           |

## Communication model

Two distinct channels, both defined in `friday-core`:

1. **Events** (`core/src/event`) — fire-and-forget, broadcast to every
   subscriber. Used for the autonomous pipeline (voice → planner → execution →
   verification → memory → voice). An `Event` is `{ id, kind, source, payload,
   trace_id, priority, hops }` where `kind` is a dot-namespaced topic such as
   `voice.transcript` or `plan.task.created`.
2. **Protocol messages** (`core/src/protocol`) — correlated request/response
   between two named modules via a `Transport`. Used when a module needs a
   specific answer from another (e.g. "transcribe this audio clip").

## Routing

`PluginManager` (`core/src/plugin/manager.rs`) subscribes to the bus once at
construction and dispatches each event only to plugins whose
`PluginMetadata.subscribes` matches the event `kind`. Plugin-produced events are
re-published back onto the bus, forming the pipeline. A `hops` counter
(`MAX_HOPS = 16`) prevents accidental feedback loops.

## Core invariants

- The core contains **no intelligence**. All behaviour is in plugins.
- Nothing is reachable without going through the bus or the transport.
- Dangerous actions pass through the permission layer before execution
  (see `Security.md`).
