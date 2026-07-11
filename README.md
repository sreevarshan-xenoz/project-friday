# PROJECT FRIDAY

> **Mission:** Build the world's best open-source desktop AI agent that can use
> Windows like a human.
>
> Not "an AI assistant." A genuine **Desktop Intelligence Platform** — the
> operating system's AI layer.

FRIDAY follows one loop:

> **Understand. Plan. Execute. Verify. Learn.**

The core contains **no intelligence of its own**. All behaviour lives in
swappable **plugins** that communicate only through an **event bus** and a
**protocol transport**. This makes the system modular, testable, and able to grow
from a simple desktop assistant into an autonomous operating companion without a
rewrite.

## Status

| Milestone | State |
| --------- | ----- |
| M0 Foundation (structure, docs, tooling) | ✅ Done |
| M1 FRIDAY Core (event bus, plugin SDK, memory, protocol, runtime) | ✅ Done |
| M2 Memory backends | ⏳ Next |
| M3 Permission & safety layer | Planned |
| M4 AI Orchestrator (Python) | Planned |
| M5 Voice & Vision | Planned |
| M6 Windows integration (C#) | Planned |
| M7 Desktop UI (React + Tauri) | Planned |
| M8 Packaging & installer (Rust) | Planned |

See [`docs/Roadmap.md`](docs/Roadmap.md) for detail.

## Repository layout

```
FRIDAY/
├── docs/            Design docs (Vision, Architecture, Modules, API, Security, …)
├── architecture/    Diagrams and ADRs
├── prototypes/     Throwaway spikes
├── services/       Long-lived capability services
├── models/         Model specialist registry & configs
├── sdk/            Cross-language SDK + protocol schema
├── frontend/       React + Tauri desktop UI
├── desktop/        Native Windows integration (C#)
├── plugins/        Individual capability plugins
├── memory/         Memory layer backends
├── tests/          Integration and system tests
├── scripts/        Build / dev automation
├── assets/         Icons, sounds, branding
└── core/           friday-core Rust crate (the engine)
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

## Documentation

Start with [`docs/Vision.md`](docs/Vision.md) and
[`docs/Architecture.md`](docs/Architecture.md). The plugin contract lives in
[`docs/PluginSDK.md`](docs/PluginSDK.md).

### Locked architecture documents

These define the frozen foundation and require explicit review + a version bump
to change:

`Architecture.md`, `Planner.md`, `Execution.md`, `Memory.md`, `Security.md`,
`PluginSDK.md`, `API.md`, `docs/research/` (original research).

Every new feature must answer: **which event does it consume? which event does
it produce? which plugin owns it?**

## Contributing

- Branch from `dev` into `feature/*`; never commit to `main`.
- Rust: `cargo fmt`, `cargo clippy -- -D warnings`, `cargo test`.
- Python (coming in M4): `black`, `ruff`, `mypy`, `pytest` via `pre-commit`.
- Every module is documented **before** its code.

Full rules in [`docs/CodingStandards.md`](docs/CodingStandards.md).

## License

MIT OR Apache-2.0.
