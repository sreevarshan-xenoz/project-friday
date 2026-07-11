# Memory

FRIDAY remembers the way people do: information moves between tiers of increasing
durability and abstraction.

## Layers

| Layer           | Kind (`MemoryLayerKind`) | Purpose                                   |
| --------------- | ------------------------ | ----------------------------------------- |
| Working         | `Working`                | Scratch space for the current turn         |
| Session         | `Session`                | Everything that happened this session      |
| Long-term       | `LongTerm`               | Durable facts, preferences, history        |
| Knowledge Graph | `KnowledgeGraph`         | Entities and relationships                 |
| Vector          | `Vector`                 | Embeddings for semantic search             |

## Data model

```rust
pub struct MemoryEntry {
    pub id: Uuid,
    pub layer: MemoryLayerKind,
    pub key: String,
    pub content: serde_json::Value,
    pub embedding: Option<Vec<f32>>,
    pub created_at: DateTime<Utc>,
    pub tags: Vec<String>,
}
```

## Interface

Every backend implements `MemoryLayer` (`core/src/memory/traits.rs`):

```rust
#[async_trait]
pub trait MemoryLayer: Send + Sync {
    fn kind(&self) -> MemoryLayerKind;
    async fn store(&self, entry: &MemoryEntry) -> Result<()>;
    async fn retrieve(&self, key: &str) -> Result<Option<MemoryEntry>>;
    async fn search(&self, query: &str, limit: usize) -> Result<Vec<MemoryEntry>>;
}
```

`MemoryEngine` aggregates registered layers and routes by `kind`. To add a
backend (e.g. SQLite session store, vector index), implement the trait and call
`Friday::register_memory_layer`.

## Flow

1. A plugin stores an observation: `memory.store(entry)`.
2. Long-term facts are promoted from session memory after verification.
3. Knowledge-graph edges are extracted from verified facts.
4. Vector embeddings enable "remember when I asked about X" style recall.

This is the **Learn** stage of the core loop.
