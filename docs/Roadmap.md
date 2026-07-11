# Roadmap

Milestones are intentionally small and each leaves the project in a runnable
state. Status follows the agreed phase order.

## M0 — Foundation ✅

- Folder structure, git branches, tooling (cargo, uv, node).
- Documentation suite (`docs/*.md`).
- Coding standards + pre-commit + CI skeleton.

## M1 — Core Runtime ✅

- `friday-core` Rust crate: async **event bus**, **Plugin SDK** +
  `PluginManager` router, layered **memory** interfaces, **protocol**
  transport, **runtime** wiring, **logging** (tracing).
- Reference `echo_plugin` example + integration test (passing).

## M2 — Memory Engine ✅

- `WorkingMemory` (in-process, ephemeral).
- `SessionMemory` and `LongTermMemory` over an embedded **redb** store
  (pure-Rust ACID; chosen over SQLite because this dev machine's Rust toolchain
  is GNU with no C compiler, so `rusqlite` cannot build here — see `Memory.md`).
- `VectorMemory` with in-memory cosine-similarity search over stored embeddings.
- `MemoryEngine::default_layers` convenience.

## M3 — Planner Engine ✅

- `friday-planner` crate with four independent engine plugins:
  - **Intent Engine** — `goal.raw` → `intent.created` (rule-based parser/classifier).
  - **Planning Engine** — `intent.created` → `plan.created` (builds a DAG `TaskGraph`).
  - **Execution Engine** — `plan.created` → `task.started` / `exec.action` → `task.completed`.
  - **Verification Engine** — `task.completed`/`task.failed` → `verification.*`, driving retry/replan.
- Plans are a **DAG**; every task has an explicit lifecycle (`TaskState`).
- End-to-end pipeline test against a `MockExecutor` (happy path + retry path).

## M4 — Execution Plugins ⏳

- Real capability plugins consuming `exec.action`: Terminal, Filesystem,
  Browser, Windows (C#), Git.
- **Permission & safety layer**: `Permission` enum + approval gate on the
  execution path (see `Security.md`); forbidden actions blocked by default;
  audit log for every privileged action.

## M5 — AI Layer ⏳

- LLM provider abstraction (local via Ollama + cloud fallback).
- Model specialist registry (conversation, planning, vision, coding, speech,
  reasoning, embedding) and model routing.
- Replaces the rule-based Intent classifier and Planning heuristics with
  model-driven ones behind the same event contract.

## M6 — Vision ⏳

- Screen capture, OCR, scene understanding, UI element tree plugins.

## M7 — Voice ⏳

- Wake word → STT → conversation → TTS pipeline as plugins.

## M8 — GUI ⏳

- Desktop UI (React + Tauri): status surface, permission prompts, memory
  viewer, plugin manager.

> The four reasoning engines and the standard event vocabulary are **locked**
> (see `Architecture.md`, `Planner.md`, `Execution.md`). New features must answer:
> which event do they consume, which do they produce, which plugin owns them.
