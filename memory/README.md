# memory/

Notes and ownership for FRIDAY's layered memory system.

**Implementation lives in `core/`** (`friday-core`), not in this directory:

| Layer | Kind | Implementation |
| ----- | ---- | -------------- |
| Working | `Working` | In-process (`core/src/memory/backends.rs`) |
| Session | `Session` | Embedded **redb** |
| Long-term | `LongTerm` | Embedded **redb** |
| Vector | `Vector` | In-memory cosine search over stored embeddings |
| Experience (procedural) | — | `ExperienceEngine` / `ExecutionExperience` (`core/src/memory/experience.rs`) |
| Knowledge Graph | `KnowledgeGraph` | Interface reserved; full graph engine still planned (M5.6) |

Register layers with `Friday::register_memory_layer`. Convenience:
`MemoryEngine::default_layers`.

## Status

✅ M2 memory backends + M4.8 experience engine shipped.  
🚧 Knowledge-graph depth remains open (`specs/knowledge_graph.md`).

See `docs/Memory.md` and `specs/memory.md` / `specs/experience.md`.
