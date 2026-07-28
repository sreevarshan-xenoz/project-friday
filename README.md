# PROJECT FRIDAY

> **Mission:** Build the world's best open-source desktop AI agent that can use
> Windows like a human.
>
> Not "an AI assistant." A genuine **Desktop Intelligence Platform** — the
> operating system's AI layer.

FRIDAY follows one loop:

> **Understand. Plan. Execute. Verify. Learn.**

The core contains **no intelligence of its own**. All behaviour lives in
swappable **plugins / services** that communicate only through an **event bus**
and a **protocol transport**. This makes the system modular, testable, and able
to grow from a simple desktop assistant into an autonomous operating companion
without a rewrite.

## Status

**Phase: Alpha Qualification** — architecture milestones M0–M5 are complete and
the runtime API is frozen. Work now proves reliability with benchmarks (MTBF,
recovery, Windows-heavy tasks), not greenfield features.

| Area | State |
| ---- | ----- |
| M0 Foundation (structure, docs, tooling) | ✅ Done |
| M1 Core (event bus, plugin SDK, protocol, runtime) | ✅ Done |
| M2 Memory (working / session / long-term / vector + experience) | ✅ Done |
| M3 Planner (intent → plan → execute → verify) | ✅ Done |
| M4 Agent Kernel (permissions, audit, lifecycle, adapters) | ✅ Done |
| M4.5 Services (filesystem, terminal, git, Windows bridge) | ✅ Done (browser still open) |
| M4.6 Capability Intelligence + Service Orchestrator | ✅ Done |
| M4.7 Plan Simulation | ✅ Done |
| M4.8 Experience Engine (procedural memory) | ✅ Done |
| M4.10 Context Builder | ✅ Done |
| M5 Cognitive / Intelligence layer | ✅ Done (providers + router) |
| MCP as first-class `ActionAdapter` | ✅ Done |
| Mobile daemon + Android console client | ✅ Scaffolded / build-verified |
| Benchmark + Recovery + Windows / Provider qual harnesses | ✅ In use |
| Skill Engine (modular capabilities) | 🚧 Spec complete |
| Desktop UI (React + TypeScript) | ✅ Foundation complete |
| Browser Runtime | ⏳ Next major capability |
| Vision / Voice / Learning / Autonomy | Planned |
| Evaluation framework, Knowledge Graph depth, Reasoning Session | 🚧 Specced / partial |

See [`docs/Roadmap.md`](docs/Roadmap.md) and [`specs/README.md`](specs/README.md)
for the authoritative milestone and subsystem map.

## Repository layout

```
FRIDAY/
├── docs/            Design docs (Vision, Architecture, Modules, Security, …)
├── specs/           Source-of-truth subsystem specs + ADRs (prefer for new work)
├── engineering/     Process: qualification, certification, benchmarks, release
├── architecture/    Diagrams / ADR index
├── core/            friday-core — event bus, plugins, memory, protocol, runtime
├── planner/         friday-planner — four reasoning engines
├── services/        Long-lived capability services (kernel, caps, sim, intel, …)
├── mcp/             friday-mcp — MCP transport, adapter, discovery
├── plugins/         Capability plugins (e.g. windows bridge client)
├── windows-host/    C# Windows host (Win32 / UIA / COM / PowerShell)
├── clients/         External clients (Android console)
├── e2e/             End-to-end test harness
├── benchmarks/      Qualification run evidence (Windows, provider, …)
├── artifacts/       Reproducible build outputs (e.g. Android APK)
├── frontend/        React + TypeScript desktop UI (design system + components + screens)
├── skills/          Skill Engine — modular, installable capability bundles
├── desktop/         Legacy path note — live host is windows-host/
├── memory/          Memory layer notes (impl lives in core/)
├── models/          Model registry notes (impl in services/intelligence)
├── sdk/             Cross-language SDK / protocol schema (in progress)
├── tests/           Cross-crate integration notes
├── scripts/         Build / dev automation
├── assets/          Icons, sounds, branding
└── prototypes/      Throwaway spikes
```

## Quick start (Core)

Requires Rust 1.85+.

```powershell
cargo build -p friday-core
cargo test  -p friday-core
cargo run   -p friday-core --example echo_plugin
```

The `echo_plugin` example registers a plugin that answers `demo.ping` with
`demo.pong`, demonstrating the full event-bus pipeline.

### Useful workspace targets

```powershell
# Planner + kernel pipeline tests
cargo test -p friday-planner
cargo test -p friday-actions

# Capability intelligence / simulation / recovery
cargo test -p friday-capabilities
cargo test -p friday-simulation
cargo test -p friday-recovery

# Benchmark harness (optional CHAOS mode)
cargo run -p friday-benchmark -- 600
# $env:CHAOS=1; cargo run -p friday-benchmark -- 600

# Windows qualification (mock-friendly; real host opt-in)
cargo run -p windows-qual -- --help
```

Windows host (C#, .NET 8):

```powershell
dotnet build windows-host/windows-host.csproj -c Release
```

Android console: see [`clients/android/README.md`](clients/android/README.md).

## Documentation

| Start here | Why |
| ---------- | --- |
| [`docs/Vision.md`](docs/Vision.md) | What FRIDAY is / is not |
| [`docs/Architecture.md`](docs/Architecture.md) | Event-driven design + subsystem addendum |
| [`docs/Roadmap.md`](docs/Roadmap.md) | Milestone history and current phase |
| [`specs/README.md`](specs/README.md) | **Source of truth** for subsystem contracts |
| [`docs/PluginSDK.md`](docs/PluginSDK.md) | Plugin contract |
| [`engineering/qualification.md`](engineering/qualification.md) | How we measure readiness |

### Locked architecture documents

These define the frozen foundation and require explicit review + a version bump
to change:

`Architecture.md`, `Planner.md`, `Execution.md`, `Memory.md`, `Security.md`,
`PluginSDK.md`, `API.md`, `docs/research/` (original research).

Event vocabulary and `ActionAdapter` / kernel contracts are also locked under
`specs/`.

Every new feature must answer: **which event does it consume? which event does
it produce? which plugin/service owns it?**

## Contributing

- Branch from `dev` into `feature/*`; never commit to `main`.
- Rust: `cargo fmt`, `cargo clippy -- -D warnings`, `cargo test`.
- Prefer a **benchmark or measurable reliability gain** over feature-only PRs
  during Alpha Qualification (see `docs/Roadmap.md`).
- Every module is documented **before** its code when introducing a new subsystem
  (`specs/` first).

Full rules in [`docs/CodingStandards.md`](docs/CodingStandards.md) and
[`engineering/coding_standard.md`](engineering/coding_standard.md).

## License

MIT OR Apache-2.0.
