# Memory Specification

Durable memory for the agent. Long-term store for memories, user preferences, and
execution history that the Context Builder (M4.10) and Learning (M9) consume.

## Crate

`friday-core` — memory module.

## Storage engine: `redb`

**Constraint:** the build toolchain is `x86_64-pc-windows-gnu` with **no C
compiler**, so `rusqlite` (needs `cc`) cannot build. Therefore durable memory uses
**`redb`** (pure-Rust, embedded, ACID) instead of SQLite. This is a hard toolchain
constraint, not a preference — do not reintroduce `rusqlite`.

## What is stored

- **Memories** — episodic/semantic facts (`Memory { id, kind, content, embedding?,
  created_at }`).
- **User preferences** — durable settings the Context Builder reads.
- **Execution history** — past plans/actions and their outcomes (feeds Simulation
  `report` accuracy and Learning).
- **Recent failures** — error signatures for the Context Builder to surface.

## Access

Synchronous embedded API; the bus and engines read/write through `friday-core`.
Memory is a **subsystem** (Knowledge) and is queried, never directly mutated by the
LLM.

## See also

- `Memory.md` (locked).
- Consumed by `specs/context.md` (M4.10) and `specs/intelligence.md` (M5).
