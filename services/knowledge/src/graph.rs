//! Knowledge Graph — Cognee-style graph memory (M5.6 + synthesis Phase 4).
//!
//! The **relationship layer** over FRIDAY's memory. Where `KnowledgeRecord`
//! (lib.rs) derives *flat* patterns from observations, the graph stores
//! *structured semantic facts* as triples:
//!
//! ```text
//!   subject ──predicate──> object
//!   git.commit ──Requires──> GitRepository
//!   git.init   ──Creates──> GitRepository
//! ```
//!
//! API surface (two contracts, one engine):
//!
//! - **M5.6 (`specs/knowledge_graph.md`)** — `assert_fact` / `query` /
//!   `find_missing_preconditions` — the planner-facing contract used to append
//!   prerequisite steps before execution.
//! - **Cognee-RS (`remember / recall / improve / forget`)** — the memory-facing
//!   contract: `remember` asserts and *strengthens* on repetition, `recall` is
//!   the query, `improve` is feedback-driven confidence adjustment, `forget`
//!   removes. Both map to the same triple store.
//!
//! Facts carry a `confidence` in `[0, 1]` — the belief strength. It is *derived*
//! deterministically (no LLM in the store): 0.5 on first assert, strengthened
//! toward 1.0 by repetition (`remember`) and explicit feedback (`improve`).
//! The graph is serializable and persists via [`JsonKnowledgeGraphStore`]
//! (atomic temp+rename, mirroring the Phase 2 `JsonWorkflowStore` precedent).

use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::PathBuf;

// ── Triple ─────────────────────────────────────────────────────────

/// A semantic fact: `subject — predicate → object`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct KnowledgeTriple {
    pub id: String,
    pub subject: String,
    pub predicate: String,
    pub object: String,
    /// Belief strength `[0.0, 1.0]`. 0.5 = asserted once; repetition
    /// (`remember`) and feedback (`improve`) move it toward 1.0; negative
    /// feedback toward 0.0. Deterministic — never LLM-derived.
    pub confidence: f32,
    /// Provenance — where this fact came from (e.g. `"ontology:seed"`,
    /// `"executive:consolidate"`). Auditable, never trusted for gating.
    #[serde(default)]
    pub source: Option<String>,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
}

impl KnowledgeTriple {
    fn new(
        subject: impl Into<String>,
        predicate: impl Into<String>,
        object: impl Into<String>,
        source: Option<String>,
        confidence: f32,
    ) -> Self {
        let now = chrono::Utc::now().timestamp_millis() as u64;
        Self {
            id: uuid::Uuid::new_v4().simple().to_string(),
            subject: subject.into(),
            predicate: predicate.into(),
            object: object.into(),
            confidence,
            source,
            created_at_ms: now,
            updated_at_ms: now,
        }
    }

    /// Same subject/predicate/object — the dedup key.
    fn same_fact(&self, subject: &str, predicate: &str, object: &str) -> bool {
        self.subject == subject && self.predicate == predicate && self.object == object
    }
}

// ── Graph ──────────────────────────────────────────────────────────

/// A knowledge graph: a deduplicated set of semantic triples.
///
/// ```text
/// KnowledgeGraph
///   triples: Vec<KnowledgeTriple>   (dedup on subject/predicate/object)
/// ```
///
/// [`KnowledgeGraph::new`] seeds the system ontology (M5.6 §"Ontology
/// Rules"): capabilities declare what they `Requires` and what they `Creates`,
/// so [`find_missing_preconditions`](KnowledgeGraph::find_missing_preconditions)
/// can answer "what must exist before `git.commit` runs?".
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct KnowledgeGraph {
    triples: Vec<KnowledgeTriple>,
}

/// Default construction seeds the ontology (spec: "when initialized, the
/// Knowledge Graph is pre-loaded with seed facts").
impl Default for KnowledgeGraph {
    fn default() -> Self {
        let mut g = Self::empty();
        g.seed_ontology();
        g
    }
}

impl KnowledgeGraph {
    /// An empty graph (tests and callers that want no seeds).
    pub fn empty() -> Self {
        Self {
            triples: Vec::new(),
        }
    }

    /// A graph pre-loaded with the system ontology (M5.6 seed facts).
    pub fn new() -> Self {
        Self::default()
    }

    /// Declare a semantic fact. Idempotent: asserting an existing fact returns
    /// its id without duplicating it (and without touching its confidence —
    /// strengthening is `remember`'s job). Returns the triple id.
    pub fn assert_fact(&mut self, subject: &str, predicate: &str, object: &str) -> String {
        self.assert_fact_with_source(subject, predicate, object, None, 0.5)
    }

    /// Internal assert with full control (used by the seeds and `remember`).
    fn assert_fact_with_source(
        &mut self,
        subject: &str,
        predicate: &str,
        object: &str,
        source: Option<String>,
        confidence: f32,
    ) -> String {
        if let Some(existing) = self
            .triples
            .iter_mut()
            .find(|t| t.same_fact(subject, predicate, object))
        {
            return existing.id.clone();
        }
        let triple = KnowledgeTriple::new(subject, predicate, object, source, confidence);
        let id = triple.id.clone();
        self.triples.push(triple);
        id
    }

    /// Cognee-style `remember`: assert a fact, and if it already exists
    /// **strengthen** it (confidence moves toward 1.0) and bump its timestamp.
    /// Returns the triple id.
    pub fn remember(&mut self, subject: &str, predicate: &str, object: &str) -> String {
        if let Some(existing) = self
            .triples
            .iter_mut()
            .find(|t| t.same_fact(subject, predicate, object))
        {
            existing.confidence += (1.0 - existing.confidence) * 0.25;
            existing.updated_at_ms = chrono::Utc::now().timestamp_millis() as u64;
            return existing.id.clone();
        }
        self.assert_fact_with_source(subject, predicate, object, None, 0.5)
    }

    /// Pattern query: any `None` field is a wildcard. Results are sorted by
    /// confidence (strongest belief first) so callers can rank candidate facts.
    pub fn query(
        &self,
        subject: Option<&str>,
        predicate: Option<&str>,
        object: Option<&str>,
    ) -> Vec<KnowledgeTriple> {
        let mut out: Vec<KnowledgeTriple> = self
            .triples
            .iter()
            .filter(|t| {
                subject.is_none_or(|s| t.subject == s)
                    && predicate.is_none_or(|p| t.predicate == p)
                    && object.is_none_or(|o| t.object == o)
            })
            .cloned()
            .collect();
        out.sort_by(|a, b| {
            b.confidence
                .partial_cmp(&a.confidence)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        out
    }

    /// Cognee-style `recall` — the query (kept as a named alias so memory
    /// callers read the Cognee vocabulary; identical semantics to [`query`]).
    pub fn recall(
        &self,
        subject: Option<&str>,
        predicate: Option<&str>,
        object: Option<&str>,
    ) -> Vec<KnowledgeTriple> {
        self.query(subject, predicate, object)
    }

    /// Feedback-driven consolidation (Cognee `improve`): adjust a fact's
    /// confidence by `delta`, clamped to `[0, 1]`. Positive delta = the fact
    /// was corroborated; negative = contradicted. Returns the new confidence,
    /// or `None` when no fact has that id. `forget` is the explicit removal —
    /// improvement never silently deletes.
    pub fn improve(&mut self, id: &str, delta: f32) -> Option<f32> {
        let triple = self.triples.iter_mut().find(|t| t.id == id)?;
        triple.confidence = (triple.confidence + delta).clamp(0.0, 1.0);
        triple.updated_at_ms = chrono::Utc::now().timestamp_millis() as u64;
        Some(triple.confidence)
    }

    /// Remove every fact matching the pattern (wildcards = any). Returns the
    /// number of facts removed.
    pub fn forget(
        &mut self,
        subject: Option<&str>,
        predicate: Option<&str>,
        object: Option<&str>,
    ) -> usize {
        let before = self.triples.len();
        self.triples.retain(|t| {
            !(subject.is_none_or(|s| t.subject == s)
                && predicate.is_none_or(|p| t.predicate == p)
                && object.is_none_or(|o| t.object == o))
        });
        before - self.triples.len()
    }

    /// The planner-facing gate (M5.6): given a target capability and the
    /// entities currently present, return every entity a `Requires` edge says
    /// is needed but is **not** present.
    ///
    /// **Id convention:** subjects are **capability-registry dot-ids**
    /// (`git.commit`, `web.search`), *not* goal-execution's `Capability` enum
    /// snake_case (`execute_command`). Passing the wrong id-space returns empty
    /// — check [`has_preconditions`](KnowledgeGraph::has_preconditions) first.
    ///
    /// ```text
    /// graph.find_missing_preconditions("git.commit", &[])
    ///     → ["GitRepository"]
    /// graph.find_missing_preconditions("git.commit", &["GitRepository"])
    ///     → []
    /// ```
    pub fn find_missing_preconditions(
        &self,
        capability: &str,
        active_entities: &[&str],
    ) -> Vec<String> {
        let present: HashSet<&str> = active_entities.iter().copied().collect();
        let mut missing = Vec::new();
        for t in &self.triples {
            if t.subject == capability
                && t.predicate == "Requires"
                && !present.contains(t.object.as_str())
                && !missing.contains(&t.object)
            {
                missing.push(t.object.clone());
            }
        }
        missing
    }

    /// Whether the ontology models any `Requires` edge for `capability`.
    /// Lets callers distinguish "checked and satisfied" (edge present, nothing
    /// missing) from "not modeled at all" (edge absent) — an unmodeled
    /// capability has no claim either way, so it must not be treated as
    /// verified-safe. Same id convention as
    /// [`find_missing_preconditions`](KnowledgeGraph::find_missing_preconditions).
    pub fn has_preconditions(&self, capability: &str) -> bool {
        self.triples
            .iter()
            .any(|t| t.subject == capability && t.predicate == "Requires")
    }

    /// What this capability *creates* (its `Creates` edges) — the planner's
    /// candidate prerequisite steps for a missing entity.
    pub fn creating_capabilities(&self, entity: &str) -> Vec<String> {
        self.triples
            .iter()
            .filter(|t| t.predicate == "Creates" && t.object == entity)
            .map(|t| t.subject.clone())
            .collect()
    }

    /// All facts where `entity` is the subject.
    pub fn neighbors(&self, entity: &str) -> Vec<&KnowledgeTriple> {
        self.triples
            .iter()
            .filter(|t| t.subject == entity)
            .collect()
    }

    /// All facts, in insertion order.
    pub fn triples(&self) -> &[KnowledgeTriple] {
        &self.triples
    }

    pub fn count(&self) -> usize {
        self.triples.len()
    }

    /// Number of distinct entities (subjects + objects).
    pub fn entity_count(&self) -> usize {
        let mut entities = HashSet::new();
        for t in &self.triples {
            entities.insert(t.subject.clone());
            entities.insert(t.object.clone());
        }
        entities.len()
    }

    /// Load the M5.6 ontology seed facts:
    ///
    /// ```text
    /// git.commit    Requires    GitRepository
    /// git.init      Creates     GitRepository
    /// git.clone     Creates     GitRepository
    /// browser.click Requires    BrowserSession
    /// browser.open  Creates     BrowserSession
    /// web.search    Requires    Network
    /// web.fetch     Requires    Network
    /// ```
    ///
    /// (`web.search`/`web.fetch` are FRIDAY's own evidence channels — the seed
    /// set is grounded in the adapters that actually exist.)
    pub fn seed_ontology(&mut self) {
        let seed = |g: &mut Self, s: &str, p: &str, o: &str| {
            g.assert_fact_with_source(s, p, o, Some("ontology:seed".into()), 1.0);
        };
        seed(self, "git.commit", "Requires", "GitRepository");
        seed(self, "git.init", "Creates", "GitRepository");
        seed(self, "git.clone", "Creates", "GitRepository");
        seed(self, "browser.click", "Requires", "BrowserSession");
        seed(self, "browser.open", "Creates", "BrowserSession");
        seed(self, "web.search", "Requires", "Network");
        seed(self, "web.fetch", "Requires", "Network");
    }
}

// ── JSON Store ─────────────────────────────────────────────────────

/// Persistence error for the graph store.
#[derive(Debug, thiserror::Error)]
pub enum KnowledgeError {
    #[error("io error: {0}")]
    Io(String),
    #[error("serialization error: {0}")]
    Serde(String),
}

impl From<serde_json::Error> for KnowledgeError {
    fn from(e: serde_json::Error) -> Self {
        Self::Serde(e.to_string())
    }
}

/// JSON-file store: the whole graph in one file (`knowledge-graph.json`) under
/// `dir`. Writes are atomic (temp + rename) so a crash can never leave a
/// truncated graph — the same durability contract as `JsonWorkflowStore`
/// (Phase 2).
pub struct JsonKnowledgeGraphStore {
    dir: PathBuf,
}

impl JsonKnowledgeGraphStore {
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
    }

    pub fn path(&self) -> PathBuf {
        self.dir.join("knowledge-graph.json")
    }

    fn atomic_write(&self, path: &std::path::Path, bytes: &[u8]) -> Result<(), KnowledgeError> {
        std::fs::create_dir_all(&self.dir).map_err(|e| KnowledgeError::Io(e.to_string()))?;
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, bytes).map_err(|e| KnowledgeError::Io(e.to_string()))?;
        std::fs::rename(&tmp, path).map_err(|e| KnowledgeError::Io(e.to_string()))?;
        Ok(())
    }

    pub fn save(&self, graph: &KnowledgeGraph) -> Result<(), KnowledgeError> {
        let json = serde_json::to_vec_pretty(graph)?;
        self.atomic_write(&self.path(), &json)
    }

    pub fn load(&self) -> Result<Option<KnowledgeGraph>, KnowledgeError> {
        let path = self.path();
        if !path.exists() {
            return Ok(None);
        }
        let bytes = std::fs::read(&path).map_err(|e| KnowledgeError::Io(e.to_string()))?;
        let graph = serde_json::from_slice(&bytes)?;
        Ok(Some(graph))
    }
}

// ── Tests ──────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seed_ontology_populates_requires_and_creates() {
        let g = KnowledgeGraph::new();
        assert!(g.count() >= 7, "seeded ontology, got {}", g.count());
        assert!(
            g.query(Some("git.commit"), None, None).len() == 1,
            "git.commit has a Requires edge"
        );
        assert!(
            g.query(None, Some("Creates"), Some("GitRepository")).len() == 2,
            "git.init + git.clone create GitRepository"
        );
        // Seeds are marked with provenance.
        assert_eq!(
            g.query(Some("git.commit"), None, None)[0].source.as_deref(),
            Some("ontology:seed")
        );
    }

    #[test]
    fn assert_fact_is_idempotent() {
        let mut g = KnowledgeGraph::empty();
        let a = g.assert_fact("git.commit", "Requires", "GitRepository");
        let b = g.assert_fact("git.commit", "Requires", "GitRepository");
        assert_eq!(a, b, "same fact returns the same id");
        assert_eq!(g.count(), 1);
    }

    #[test]
    fn query_filters_by_any_combination() {
        let mut g = KnowledgeGraph::empty();
        g.assert_fact("git.commit", "Requires", "GitRepository");
        g.assert_fact("git.init", "Creates", "GitRepository");
        g.assert_fact("browser.click", "Requires", "BrowserSession");

        assert_eq!(g.query(None, None, None).len(), 3);
        assert_eq!(g.query(Some("git.commit"), None, None).len(), 1);
        assert_eq!(g.query(None, Some("Creates"), None).len(), 1);
        assert_eq!(g.query(None, None, Some("BrowserSession")).len(), 1);
        assert_eq!(g.query(Some("nope"), None, None).len(), 0);
    }

    #[test]
    fn remember_strengthens_confidence_on_repeat() {
        let mut g = KnowledgeGraph::empty();
        let id = g.remember("git.commit", "Requires", "GitRepository");
        let first = g.query(None, None, None)[0].confidence;
        assert!((first - 0.5).abs() < f32::EPSILON, "first assert = 0.5");
        assert_eq!(g.remember("git.commit", "Requires", "GitRepository"), id);
        let second = g.query(None, None, None)[0].confidence;
        assert!(
            second > first,
            "repetition strengthens: {first} -> {second}"
        );
        assert_eq!(g.count(), 1, "still one fact, deduplicated");
    }

    #[test]
    fn improve_adjusts_and_clamps_confidence() {
        let mut g = KnowledgeGraph::empty();
        let id = g.assert_fact("git.commit", "Requires", "GitRepository");
        // Corroborated.
        let up = g.improve(&id, 0.3).unwrap();
        assert!((up - 0.8).abs() < f32::EPSILON);
        // Contradicted — clamps at zero, never negative.
        let down = g.improve(&id, -5.0).unwrap();
        assert_eq!(down, 0.0);
        // Unknown id.
        assert!(g.improve("missing", 0.1).is_none());
    }

    #[test]
    fn forget_removes_matching_triples() {
        let mut g = KnowledgeGraph::new();
        let before = g.count();
        assert_eq!(g.forget(Some("browser.click"), None, None), 1);
        assert_eq!(g.count(), before - 1);
        assert_eq!(
            g.forget(None, None, None),
            before - 1,
            "wildcard forget clears the rest"
        );
        assert_eq!(g.count(), 0);
    }

    #[test]
    fn find_missing_preconditions_reports_absent_entities() {
        let g = KnowledgeGraph::new();
        let missing = g.find_missing_preconditions("git.commit", &[]);
        assert_eq!(missing, vec!["GitRepository"]);

        let ok = g.find_missing_preconditions("git.commit", &["GitRepository"]);
        assert!(ok.is_empty(), "precondition satisfied: {ok:?}");

        // browser + web channels
        assert_eq!(
            g.find_missing_preconditions("browser.click", &["BrowserSession"]),
            Vec::<String>::new()
        );
        assert_eq!(
            g.find_missing_preconditions("web.search", &["Network"]),
            Vec::<String>::new()
        );
        // A capability with no Requires edges has no preconditions.
        assert!(g.find_missing_preconditions("git.init", &[]).is_empty());
    }

    #[test]
    fn has_preconditions_distinguishes_modeled_from_unmodeled() {
        let g = KnowledgeGraph::new();
        // Modeled: git.commit has a Requires edge.
        assert!(g.has_preconditions("git.commit"));
        // Modeled-and-satisfied vs unmodeled must be distinguishable.
        assert!(
            g.find_missing_preconditions("git.commit", &["GitRepository"])
                .is_empty()
                && g.has_preconditions("git.commit"),
            "satisfied is not the same as unmodeled"
        );
        // Unmodeled: goal-execution's snake_case id is NOT the graph's id-space.
        assert!(!g.has_preconditions("execute_command"));
        assert!(!g.has_preconditions("read_file"));
    }

    #[test]
    fn creating_capabilities_lists_prerequisite_steps() {
        let g = KnowledgeGraph::new();
        let mut creators = g.creating_capabilities("GitRepository");
        creators.sort();
        assert_eq!(creators, vec!["git.clone", "git.init"]);
    }

    #[test]
    fn neighbors_and_entity_count_cover_the_graph() {
        let mut g = KnowledgeGraph::empty();
        g.assert_fact("git.commit", "Requires", "GitRepository");
        g.assert_fact("git.init", "Creates", "GitRepository");

        let n = g.neighbors("git.commit");
        assert_eq!(n.len(), 1);
        assert_eq!(n[0].predicate, "Requires");
        // 3 distinct entities: git.commit, git.init, GitRepository.
        assert_eq!(g.entity_count(), 3);
        // No neighbors for an unknown entity.
        assert!(g.neighbors("nope").is_empty());
    }

    #[test]
    fn serde_round_trips_confidence_and_seeds() {
        let mut g = KnowledgeGraph::new();
        g.remember("git.commit", "Requires", "GitRepository");
        let json = serde_json::to_string(&g).unwrap();
        let back: KnowledgeGraph = serde_json::from_str(&json).unwrap();
        assert_eq!(back.count(), g.count());
        assert_eq!(back, g, "identical after round-trip");
    }

    #[test]
    fn json_store_round_trips_atomically() {
        let dir = std::env::temp_dir().join(format!("friday-kg-{}", uuid::Uuid::new_v4().simple()));
        let store = JsonKnowledgeGraphStore::new(&dir);

        // Missing file → None.
        assert!(store.load().unwrap().is_none());

        let mut g = KnowledgeGraph::new();
        g.remember("git.commit", "Requires", "GitRepository");
        store.save(&g).unwrap();

        let loaded = store.load().unwrap().expect("saved graph loads");
        assert_eq!(loaded, g);
        assert!(store.path().exists());
        assert!(
            !store.path().with_extension("json.tmp").exists(),
            "no temp file left behind"
        );

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn ontology_survives_world_snapshot_style_round_trip() {
        // The graph is embedded in WorldModel snapshots; ensure the serde
        // shape is stable and seeds survive a full JSON round-trip.
        let g = KnowledgeGraph::new();
        let json = serde_json::to_string_pretty(&g).unwrap();
        let back: KnowledgeGraph = serde_json::from_str(&json).unwrap();
        assert_eq!(
            back.find_missing_preconditions("git.commit", &[]),
            vec!["GitRepository"]
        );
    }
}
