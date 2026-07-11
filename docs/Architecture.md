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
            Intent Engine
                     │   goal.raw / intent.resolved
            Planning Engine
                     │   plan.created
            Execution Engine
                     │   exec.action / exec.result
          Verification Engine
                     │   verify.passed / verify.failed
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
              Memory Engine
                     │
               Knowledge Graph
                     │
              Plugin Manager
                     │
                 Voice Reply
```

## The four reasoning engines

"Planner" is intentionally **not** a single component. It is split into four
independent engines, each with exactly one responsibility. This separation makes
debugging, recovery, and evaluation dramatically easier because every stage has
a clear input event and output event.

| Engine              | Consumes            | Produces             | Responsibility                                  |
| ------------------- | ------------------- | -------------------- | ------------------------------------------------ |
| **Intent Engine**   | `goal.raw`          | `intent.resolved`    | Understand what the user actually wants → structured goal. |
| **Planning Engine** | `intent.resolved`   | `plan.created`       | Decompose the goal into ordered, executable steps. |
| **Execution Engine**| `plan.created`      | `exec.action`, `exec.result` | Choose the correct plugin and perform the action. |
| **Verification Engine** | `exec.result`   | `verify.passed` / `verify.failed` | Confirm success; retry or trigger a replan on failure. |

The engines are ordinary plugins on the bus — they are not special-cased in the
core. Swapping or upgrading one (e.g. a better planner) never touches another.
Full design in [`docs/Planner.md`](Planner.md) and [`docs/Execution.md`](Execution.md).

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
   subscriber. Used for the autonomous pipeline (intent → planning → execution →
   verification → memory → voice). An `Event` is `{ id, kind, source, payload,
   trace_id, priority, hops }` where `kind` is a dot-namespaced topic such as
   `voice.transcript` or `plan.created`.
2. **Protocol messages** (`core/src/protocol`) — correlated request/response
   between two named modules via a `Transport`. Used when a module needs a
   specific answer from another (e.g. "transcribe this audio clip").

## Routing

`PluginManager` (`core/src/plugin/manager.rs`) subscribes to the bus once at
construction and dispatches each event only to plugins whose
`PluginMetadata.subscribes` matches the event `kind`. Plugin-produced events are
re-published back onto the bus, forming the pipeline. A `hops` counter
(`MAX_HOPS = 16`) prevents accidental feedback loops.

## Memory

The memory system is layered (working → session → long-term → knowledge graph →
vector). Durable tiers use an embedded **redb** store (pure-Rust, ACID, no C
toolchain required); SQLite can be dropped in later behind the same interface.
See [`docs/Memory.md`](Memory.md).

## Core invariants

- The core contains **no intelligence**. All behaviour is in plugins.
- Nothing is reachable without going through the bus or the transport.
- Dangerous actions pass through the permission layer before execution
  (see `Security.md`).
- Every new feature answers three questions: **which event does it consume?
  which event does it produce? which plugin owns it?**

---

## Addendum — Services & Subsystems (M4.6, authorized extension)

The M4.6 Capability Intelligence work reorganizes how we *name* things without
changing the locked contracts below. This addendum is an authorized extension of
the frozen architecture.

- **Service, not plugin, is the organizing term above `ActionAdapter`.** A
  *service* (filesystem, git, terminal, windows, future browser) may internally
  use multiple adapters/bridges (e.g. the Windows service = Rust adapter + C#
  host over IPC). The `ActionAdapter` trait is unchanged.
- **Subsystems** group the work: **Kernel** (execution/lifecycle/context/
  resources in `friday-actions`), **Knowledge** (memory + `friday-capabilities`
  registry/ranking/health/graph), **Services**, **Perception**, **Cognition**
  (planner + M5 LLM), **Autonomy**.
- The planner now reasons over **capabilities + ranked providers** held in the
  Capability Intelligence registry, never over plugin names. See
  `docs/CapabilityIntelligence.md`.
- Services advertise a rich `Capability` (id, group, actions, platform, latency,
  reliability, health, cost, confidence, …) via `manifest.json` discovery or an
  in-code `capabilities()` function. The registry scores providers and filters
  by availability, enabling autonomous routing and self-healing.

The event vocabulary, `friday-core` API, and `friday-planner` API remain frozen;
Capability Intelligence is an additive layer the planner consults.
