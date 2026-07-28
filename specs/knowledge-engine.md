# Knowledge Engine Specification

**Status:** 🚧 Spec-first — implement against this file.
**Depends on:** `friday-core` (memory, event bus)
**Crate:** `services/knowledge` — crate `friday-knowledge`

## 1. Purpose

Memory is experiences. Knowledge is facts. They are different systems.

Memory remembers:
- "Docker failed yesterday due to permissions"
- "Sree prefers vim over nano"
- "This machine uses rootless Docker"

Knowledge knows:
- "Cargo builds Rust projects"
- "Docker uses OCI images"
- "Rust Analyzer is an LSP for Rust"
- "The FRIDAY mobile protocol version is 1.0"

The Knowledge Engine stores, indexes, and retrieves facts from any source:
documentation, APIs, codebases, PDFs, notes, and conversations.

## 2. Memory vs Knowledge

| Dimension | Memory | Knowledge |
|-----------|--------|-----------|
| Content | Personal experiences | Universal facts |
| Source | User actions, outcomes | Docs, APIs, code, notes |
| Changes | Frequent (new experiences) | Stable (until updated) |
| Query | "What happened when X?" | "What is X?" |
| Use case | Learning, personalization | Reasoning, planning |
| Lifetime | Session → Long-term | Persistent until expired |

Both are needed. A system with only memory forgets facts. A system with
only knowledge forgets experiences.

## 3. Knowledge Model

```rust
pub struct KnowledgeEntry {
    pub id: String,
    pub content: String,           // the fact itself
    pub source: KnowledgeSource,   // where it came from
    pub category: String,          // grouping (e.g., "docker", "rust", "api")
    pub tags: Vec<String>,         // searchable tags
    pub confidence: f64,           // 0..1, how sure are we
    pub version: Option<String>,   // e.g., "Rust 1.80", "Docker 24.0"
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub expires_at: Option<DateTime<Utc>>,  // for time-sensitive facts
    pub embedding: Option<Vec<f32>>,        // for semantic search
}

pub enum KnowledgeSource {
    Documentation,   // official docs
    Codebase,        // extracted from code comments, types, signatures
    Api,             // API references
    Manual,          // user-provided manuals
    Conversation,    // extracted from conversations
    Learned,         // inferred from patterns
    UserProvided,    // explicitly added by user
}
```

## 4. Knowledge Categories

| Category | Examples | Sources |
|----------|---------|---------|
| **System** | OS commands, file paths, env vars | Codebase, docs |
| **Language** | Rust syntax, Python stdlib, Kotlin coroutines | Docs, API refs |
| **Tool** | Docker commands, git workflows, cargo subcommands | Docs, manual |
| **Project** | FRIDAY architecture, module roles, event vocabulary | Project docs |
| **API** | REST endpoints, protocol fields, plugin interfaces | API specs |
| **Concept** | Container = image + runtime, LSP = language server | Docs, learned |
| **User** | User's preferences, project context, team norms | Conversation |

## 5. Knowledge Graph

Knowledge entries are connected via a graph:

```
Docker
├── USES → OCI Images
├── MANAGES → Containers
├── DEFINED_BY → docker-compose.yml
├── RUNS_ON → Linux
├── ALTERNATIVE_TO → Podman
└── LEARNED_BY → FRIDAY (from user fixing permissions)

Cargo
├── BUILDS → Rust Projects
├── USES → Cargo.toml
├── PUBLISHES_TO → crates.io
├── COMMANDS → build, test, clippy, fmt
└── PART_OF → Rust Toolchain
```

Graph edges carry:
- `relationship` type (USES, MANAGES, PART_OF, ALTERNATIVE_TO, etc.)
- `confidence` (how sure we are about this relationship)
- `source` (where this relationship was learned)

The Reasoning Engine traverses this graph for causal analysis:
"Docker failed → Docker runs on Linux → Linux needs cgroups → check cgroups"

## 6. Indexing strategies

### 6.1 Semantic search (embeddings)

Knowledge entries are embedded using a local model (all-MiniLM-L6-v2 or
similar). Query returns entries with highest cosine similarity.

Use case: "How do I build a Rust project?" → semantic match to
"Cargo builds Rust projects using `cargo build`"

### 6.2 Keyword search (BM25)

Fast exact-match search for known terms.

Use case: "docker compose" → exact match to entries containing "docker compose"

### 6.3 Graph traversal

Follow relationships from a known entry.

Use case: "What depends on Docker?" → traverse `DEPENDS_ON → Docker` edges

### 6.4 Hybrid

Combine all three: semantic for discovery, keyword for precision,
graph for context.

## 7. Knowledge Sources

### 7.1 Local documentation

Scan project directories for:
- README.md, ARCHITECTURE.md, CONTRIBUTING.md
- Code comments with `///` or `//!`
- Type signatures (extracted by language server)
- Configuration files (Cargo.toml, package.json, docker-compose.yml)

### 7.2 API references

Index API documentation:
- REST endpoints from OpenAPI specs
- Protocol fields from schema definitions
- Plugin interfaces from trait definitions

### 7.3 Codebase indexing

Extract knowledge from code:
- Function signatures → "what does this function do?"
- Type definitions → "what is this struct?"
- Module structure → "where is X implemented?"
- Dependencies → "what does this crate depend on?"

### 7.4 User-provided knowledge

Users can add knowledge explicitly:
- Notes (`~/.friday/knowledge/notes/`)
- Bookmarks (`~/.friday/knowledge/bookmarks.json`)
- PDFs (`~/.friday/knowledge/pdfs/`)

### 7.5 Learned knowledge

Extracted from conversations and outcomes:
- "Docker failed due permissions" → Knowledge: "this machine uses rootless Docker"
- "User always uses vim" → Knowledge: "user prefers vim"
- "This error = this fix" → Knowledge: "error X means Y, fix with Z"

## 8. Integration points

### 8.1 With Memory Engine

Knowledge and Memory share the `MemoryEngine` storage layer but have
separate namespaces. Memory stores experiences; Knowledge stores facts.
The Context Builder queries both.

### 8.2 With Reasoning Engine

The Reasoning Engine queries Knowledge for facts during planning:
- "What is Docker?" → Knowledge graph traversal
- "How do I build Rust?" → Semantic search
- "What's the FRIDAY protocol version?" → Keyword lookup

### 8.3 With Context Builder

The Context Builder includes relevant Knowledge in `AgentContext`:
- Knowledge entries matching the goal
- Graph neighbors of matched entries
- Recent knowledge updates

### 8.4 With Learning Engine

The Learning Engine extracts knowledge from conversations:
- User explains something → extract as Knowledge
- Outcome reveals a fact → store as Knowledge
- Pattern detected → store as Knowledge

## 9. Events

| Event | Payload | Producer |
|-------|---------|----------|
| `knowledge.indexed` | `{ entry_id, source, category }` | KnowledgeEngine |
| `knowledge.found` | `{ query, entry_ids, latency_ms }` | KnowledgeEngine |
| `knowledge.graph.edge.added` | `{ from, to, relationship }` | KnowledgeEngine |
| `knowledge.graph.edge.removed` | `{ from, to }` | KnowledgeEngine |
| `knowledge.expired` | `{ entry_id }` | KnowledgeEngine |
| `knowledge.source.updated` | `{ source, count }` | KnowledgeEngine |

## 10. Metrics

```rust
pub struct KnowledgeMetrics {
    pub total_entries: u64,
    pub total_edges: u64,
    pub entries_by_source: HashMap<KnowledgeSource, u64>,
    pub entries_by_category: HashMap<String, u64>,
    pub average_query_latency_ms: f64,
    pub semantic_search_accuracy: f64,
    pub index_size_bytes: u64,
    pub last_index_update: DateTime<Utc>,
}
```

## 11. Acceptance criteria

- [ ] `friday-knowledge` builds clean (clippy/fmt/tests)
- [ ] Knowledge entries can be added, queried, and removed
- [ ] Semantic search returns relevant results for natural language queries
- [ ] Keyword search returns exact matches
- [ ] Graph traversal follows relationship edges
- [ ] Hybrid search combines semantic + keyword + graph
- [ ] Local documentation is indexed automatically
- [ ] Codebase indexing extracts function signatures and types
- [ ] User-provided knowledge is indexed from `~/.friday/knowledge/`
- [ ] Learned knowledge is extracted from conversations
- [ ] Context Builder includes relevant Knowledge in AgentContext
- [ ] Test: index 100 entries, query "Docker", return relevant results < 50ms

## 12. What this unlocks

- **Fact retrieval:** FRIDAY knows facts, not just experiences
- **Codebase awareness:** knows your project's structure and APIs
- **Documentation search:** finds relevant docs without internet
- **Concept graphs:** understands relationships between concepts
- **Learning:** extracts new knowledge from conversations
- **Better planning:** reasons over facts, not just memories
