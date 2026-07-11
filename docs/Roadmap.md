# Roadmap

Milestones are intentionally small and each leaves the project in a runnable
state.

## M0 — Foundation ✅

- Folder structure, git branches, tooling (cargo, uv, node).
- Documentation suite (`docs/*.md`).
- Coding standards + pre-commit + CI skeleton.

## M1 — FRIDAY Core ✅ (this milestone)

- `friday-core` Rust crate:
  - Async **event bus** (`core/src/event`).
  - **Plugin SDK** + `PluginManager` router (`core/src/plugin`).
  - Layered **memory** interfaces (`core/src/memory`).
  - **Protocol** transport for request/response (`core/src/protocol`).
  - **Runtime** that wires it together (`core/src/runtime`).
- Reference `echo_plugin` example + integration test (passing).

## M2 — Memory backends ✅

- Implement `WorkingMemory` (in-process, ephemeral).
- Implement `SessionMemory` and `LongTermMemory` over an embedded **redb** store
  (pure-Rust ACID; chosen over SQLite because this dev machine's Rust toolchain
  is GNU with no C compiler, so `rusqlite` cannot build here — see `Memory.md`).
- Implement `VectorMemory` with in-memory cosine-similarity search over stored
  embeddings.
- Wire memory events: `memory.store`, `memory.recall`, `memory.search`.

## M3 — Permission & safety layer

- Define a `Permission` enum and an approval gate on the execution path.
- Block the forbidden actions from `Security.md` by default.
- Audit log for every privileged action.

## M4 — AI Orchestrator (Python)

- Planner + reasoning models routing through the event bus.
- Model specialist registry (conversation, planning, vision, coding, speech,
  embedding).
- Local-first with Ollama, cloud fallback.

## M5 — Voice & Vision

- Wake word → STT → conversation → TTS pipeline as plugins.
- Screen capture + OCR + scene understanding plugin.

## M6 — Windows integration (C#)

- Window management, clipboard, file operations, app launching via a C# service
  exposed over the protocol transport.

## M7 — Desktop UI (React + Tauri)

- Status surface, permission prompts, memory viewer, plugin manager UI.

## M8 — Packaging & installer (Rust)

- Single native installer, auto-update, signed binary.
