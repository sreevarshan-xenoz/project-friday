# Knowledge Graph Specification (M5.6)

**Status:** ✅ **Implemented (2026-08-09) — synthesis Phase 4 shipped.** The engine
lives in `friday-knowledge` (`services/knowledge/src/graph.rs`): `KnowledgeGraph`
with the full contract below **plus** the Cognee-style memory API
(`remember / recall / improve / forget`) and a `JsonKnowledgeGraphStore`
(atomic temp+rename). The graph is wired into goal-execution's `WorldModel`
as its relationship layer (`knowledge()` / `knowledge_mut()` /
`find_missing_capability_prerequisites`). **Not yet wired:** the planner's
`Planning::think()` does not yet *append* the prerequisite steps returned by
`creating_capabilities` into the task graph — that is the next brick
(see §Integration with Planner).

**Spec-first:** authoritative spec for FRIDAY's semantic knowledge model.

Rather than relying entirely on vector embedding similarities or LLM intuition, the planner and context builder query a structured Semantic Knowledge Graph. This graph represents rules, preconditions, and relationships between concepts, directories, files, and capabilities.

```
[Git Repo] ──(hasCommand)──> [git.commit] ──(requires)──> [Modified Files]
```

## Graph Schema

The Knowledge Graph will reside in `friday-core::memory::knowledge` (or a dedicated crate `friday-knowledge`) and represent relationships as triple edges (`Subject - Predicate - Object`):

### 1. Concepts / Nodes
- **Entity**: e.g., `GitRepository`, `BrowserSession`, `LocalPath`, `Workspace`.
- **Capability**: e.g., `git.commit`, `terminal.run`, `filesystem.write_file`.
- **Resource**: e.g., `DiskSpace`, `MemoryBuffer`.

### 2. Relations / Predicates
- `Creates`: Node A creates Entity B.
- `Requires`: Capability A requires Entity B or Capability B.
- `Alters`: Action A alters Entity B.
- `ChildOf`: Path A is a child of Path B.

## API Contract

Implemented in `friday-knowledge` (`graph.rs`). `KnowledgeTriple` additionally
carries `id` (uuid), `confidence` (`[0,1]` — belief strength, derived
*deterministically*, never LLM-computed), optional `source` provenance, and
created/updated timestamps. `assert_fact` returns the triple id and is
idempotent (asserting an existing fact returns its id without duplicating).

```rust
pub struct KnowledgeTriple {
    pub id: String,
    pub subject: String,
    pub predicate: String,
    pub object: String,
    pub confidence: f32,          // 0.5 on first assert; strengthened by remember/improve
    pub source: Option<String>,   // e.g. "ontology:seed"
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
}

pub struct KnowledgeGraph {
    /// In-memory graph representation (custom adjacency: Vec<KnowledgeTriple>,
    /// dedup on subject/predicate/object)
    triples: Vec<KnowledgeTriple>,
}

impl KnowledgeGraph {
    pub fn new() -> Self;               // seeded with the ontology below
    pub fn empty() -> Self;             // no seeds (tests / custom graphs)

    /// Declare a semantic fact in the graph (idempotent). Returns the id.
    pub fn assert_fact(&mut self, subject: &str, predicate: &str, object: &str) -> String;

    /// Query the graph for relations (any `None` field is a wildcard);
    /// results sorted by confidence descending.
    pub fn query(&self, subject: Option<&str>, predicate: Option<&str>, object: Option<&str>) -> Vec<KnowledgeTriple>;

    /// Find execution dependencies (e.g. what preconditions are missing to run git.commit?)
    pub fn find_missing_preconditions(&self, target_capability: &str, active_entities: &[&str]) -> Vec<String>;

    /// Whether the ontology models any Requires edge for `capability` — lets
    /// callers distinguish "checked and satisfied" from "not modeled at all".
    pub fn has_preconditions(&self, capability: &str) -> bool;

    /// Capabilities that *create* `entity` (the planner's candidate steps to
    /// prepend when a prerequisite is missing).
    pub fn creating_capabilities(&self, entity: &str) -> Vec<String>;

    // ── Cognee-style memory API (synthesis Phase 4) ──────────────
    /// Remember: assert and strengthen on repetition (confidence → 1.0).
    pub fn remember(&mut self, subject: &str, predicate: &str, object: &str) -> String;
    /// Recall: the query under the Cognee vocabulary.
    pub fn recall(&self, subject: Option<&str>, predicate: Option<&str>, object: Option<&str>) -> Vec<KnowledgeTriple>;
    /// Improve: feedback-driven confidence adjustment by id, clamped to [0,1].
    pub fn improve(&mut self, id: &str, delta: f32) -> Option<f32>;
    /// Forget: remove matching triples (wildcards allowed); returns count removed.
    pub fn forget(&mut self, subject: Option<&str>, predicate: Option<&str>, object: Option<&str>) -> usize;
}

// Persistence
pub struct JsonKnowledgeGraphStore { /* dir */ }
impl JsonKnowledgeGraphStore {
    pub fn new(dir: impl Into<PathBuf>) -> Self;
    pub fn save(&self, graph: &KnowledgeGraph) -> Result<(), KnowledgeError>;  // atomic temp+rename
    pub fn load(&self) -> Result<Option<KnowledgeGraph>, KnowledgeError>;
}
```

## Ontology Rules (Seed Facts)
When initialized, the Knowledge Graph is pre-loaded with seed facts about system tools:
- `git.commit` -> `Requires` -> `GitRepository`
- `git.init` -> `Creates` -> `GitRepository`
- `browser.click` -> `Requires` -> `BrowserSession`
- `browser.open` -> `Creates` -> `BrowserSession`

## Integration with Planner (M3)
The planner uses this graph to automatically append prerequisite steps. If a user asks to `commit changes`, the planner queries `Requires(git.commit)` -> finds `GitRepository` -> checks if present in `WorldState` -> if missing, appends `git.init` or `git.clone` to the task graph before the commit node.

The **query half is shipped**: `WorldModel::find_missing_capability_prerequisites("git.commit")` returns `["GitRepository"]` when no `.git` is in the state, and `creating_capabilities("GitRepository")` returns `["git.clone", "git.init"]` (tested end-to-end in goal-execution). **The append half is not yet wired**: `Planning::think()` does not consume these to build the plan. **Id-space note:** subjects are capability-registry dot-ids (`git.commit`), not goal-execution's `Capability` enum snake_case (`execute_command`) — `has_preconditions` exists so callers can detect a wrong id-space instead of silently treating an unmodeled capability as satisfied.
