# Knowledge Graph Specification (M5.6)

**Status:** 🚧 Planned. **Spec-first:** authoritative spec for FRIDAY's semantic knowledge model.

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

```rust
pub struct KnowledgeTriple {
    pub subject: String,
    pub predicate: String,
    pub object: String,
}

pub struct KnowledgeGraph {
    /// In-memory graph representation (using petgraph or custom adjacency map)
    triples: Vec<KnowledgeTriple>,
}

impl KnowledgeGraph {
    pub fn new() -> Self;

    /// Declare a semantic fact in the graph
    pub fn assert_fact(&mut self, subject: &str, predicate: &str, object: &str);

    /// Query the graph for relations
    pub fn query(&self, subject: Option<&str>, predicate: Option<&str>, object: Option<&str>) -> Vec<KnowledgeTriple>;

    /// Find execution dependencies (e.g. what preconditions are missing to run git.commit?)
    pub fn find_missing_preconditions(&self, target_capability: &str, active_entities: &[&str]) -> Vec<String>;
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
