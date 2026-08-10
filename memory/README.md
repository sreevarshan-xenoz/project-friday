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
| Knowledge Graph | `KnowledgeGraph` | `friday-knowledge` (`services/knowledge`) — graph + World Model wiring |
| Admission control | — | `friday-goal-execution` `admission.rs` — importance / novelty / reliability / sensitivity gates before an episode enters memory |

Register layers with `Friday::register_memory_layer`. Convenience:
`MemoryEngine::default_layers`.

## Status

✅ M2 memory backends + M4.8 experience engine shipped.  
✅ Knowledge Graph + World Model shipped (`friday-knowledge`, synthesis Phase 4).  
✅ Memory admission controller shipped (synthesis Phase 1b).  
🚧 Consolidation depth, decay, and retrieval scoring remain open
(`specs/knowledge_graph.md`, `specs/friday-2-0-synthesis.md`).

See `docs/Memory.md` and `specs/memory.md` / `specs/experience.md`.
