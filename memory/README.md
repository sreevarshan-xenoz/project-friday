# memory/

Backends for the layered memory system (working, session, long-term, knowledge
graph, vector). Each implements `MemoryLayer` from `core/src/memory/traits.rs`
and is registered with `Friday::register_memory_layer`.

Status: interfaces defined (M1); backends in M2.
