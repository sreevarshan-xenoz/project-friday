# Modules

FRIDAY itself knows almost nothing. Every capability is a **plugin** or
**service** (often an `ActionAdapter` behind the AgentKernel). If a better
implementation appears, you swap the module — you do not rewrite the application.

## Plugin lifecycle

Defined by the `Plugin` trait (`core/src/plugin/sdk.rs`):

1. `metadata()` — static declaration of identity and interests.
2. `init(&ctx)` — acquire resources, called once before routing starts.
3. `handle_event(&event) -> Vec<Event>` — do the work, emit follow-up events.
4. `shutdown()` — release resources and flush state.

Action-capable services additionally implement `ActionAdapter` and are dispatched
by the **AgentKernel** (`services/action-runtime`, crate `friday-actions`) after
permission and audit.

## Capability map (current)

| Module / Plugin / Service | Kind(s) / tool | Language | Status |
| ------------------------- | -------------- | -------- | ------ |
| Intent Engine | `goal.raw` → `intent.created` | Rust (`friday-planner`) | ✅ |
| Planning Engine | `intent.created` → `plan.created` | Rust (`friday-planner`) | ✅ |
| Execution Engine | `plan.created` → `exec.action` / tasks | Rust (`friday-planner`) | ✅ |
| Verification Engine | task results → `verification.*` | Rust (`friday-planner`) | ✅ |
| AgentKernel | `exec.action` → `exec.result` | Rust (`friday-actions`) | ✅ Stable |
| Filesystem | `filesystem.*` | Rust | ✅ |
| Terminal | `terminal.*` | Rust | ✅ |
| Git | `git.*` | Rust | ✅ |
| Windows bridge | `windows.*` | Rust + C# host | ✅ |
| MCP tools | dynamic MCP tools | Rust (`friday-mcp` + `McpAdapter`) | ✅ |
| Capability Intelligence | registry / rank / health | Rust (`friday-capabilities`) | ✅ |
| Plan Simulation | dry-run / validate / estimate | Rust (`friday-simulation`) | ✅ |
| Context Builder | `AgentContext` assembly | Rust (`friday-context`) | ✅ |
| Intelligence layer | providers / router / compiler | Rust (`friday-intelligence`) | ✅ |
| Experience Engine | procedural memory | Rust (`friday-core`) | ✅ |
| Recovery | failure policies / confidence | Rust (`friday-recovery`) | ✅ (shadow) |
| Benchmark harness | resilience metrics | Rust (`friday-benchmark`) | ✅ |
| Mobile daemon | WebSocket protocol v1.0 | Rust (`friday-mobile`) | ✅ |
| Android console | Mobile Protocol client | Kotlin | ✅ Build-verified |
| Browser | `browser.*` | TBD | ⏳ Planned |
| Vision | `vision.frame`, `vision.query` | Python (planned) | ⏳ Planned |
| Wake Word | `audio.wake` | Python (planned) | ⏳ Planned |
| Speech Recognition | `audio.speech` | Python (planned) | ⏳ Planned |
| Conversation Engine | `conv.turn` | Python (planned) | ⏳ Planned |
| Voice Reply (TTS) | `voice.speak` | Python (planned) | ⏳ Planned |
| Coding | `code.*` | Python (planned) | ⏳ Planned |
| Discord / Spotify / Steam / OBS / VSCode | product integrations | various | ⏳ Planned |
| Desktop UI | status / permissions UI | React + Tauri | ⏳ Planned |
| Knowledge Graph depth | entities / relations | Rust | 🚧 Interface only |
| Evaluation framework | offline eval harness | Rust | 🚧 Specced |

## Core reasoning engines (frozen design)

The planner is split into four independent engines so each stage is debuggable
and replaceable. They are ordinary bus plugins, not core-specialised code.

| Engine | Consumes | Produces | Design doc |
| ------ | -------- | -------- | ---------- |
| Intent Engine | `goal.raw` | `intent.created` | `docs/Planner.md` |
| Planning Engine | `intent.created` | `plan.created` | `docs/Planner.md` |
| Execution Engine | `plan.created` | `exec.action`, task lifecycle | `docs/Execution.md` |
| Verification Engine | task results | `verification.*` | `docs/Planner.md` |

> Note: early design docs used `intent.resolved` / `verify.passed`. The
> implemented vocabulary is in `planner/src/events.rs` and `specs/events.md` —
> prefer those for new work.

## Reference plugins shipped with the core

`core/examples/echo_plugin.rs` is a working reference implementation that
subscribes to `demo.ping` and emits `demo.pong`. Use it as the template for new
bus plugins.

## Adding a module

1. Prefer a **service + `ActionAdapter`** for executable capabilities (kernel path).
2. Or create a crate under `plugins/<name>/` for bus plugins / bridges.
3. Implement the relevant trait(s); declare `subscribes` / `emits` or a capability
   manifest.
4. Register with `Friday::register_plugin` and/or `AgentKernel`.
5. Document it here, in `PluginSDK.md` / `specs/services.md`, and add tests.

## Subsystem map

| Subsystem | Owns |
| --------- | ---- |
| **Kernel** | AgentKernel — execution, lifecycle, permissions, audit, sessions |
| **Knowledge** | Memory, experience, capability registry |
| **Services** | filesystem, git, terminal, windows, (future) browser |
| **Cognition** | Planner + intelligence layer |
| **Perception** | Vision, voice (planned) |
| **Autonomy** | Learning, workflows, background agents (planned) |
