use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::Path;
use uuid::Uuid;

use super::backends::{LongTermMemory, SessionMemory, VectorMemory, WorkingMemory};

/// The distinct memory tiers from the architecture. Each is a pluggable layer
/// that can be backed by a different store.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MemoryLayerKind {
    Working,
    Session,
    LongTerm,
    KnowledgeGraph,
    Vector,
}

impl MemoryLayerKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            MemoryLayerKind::Working => "working",
            MemoryLayerKind::Session => "session",
            MemoryLayerKind::LongTerm => "long_term",
            MemoryLayerKind::KnowledgeGraph => "knowledge_graph",
            MemoryLayerKind::Vector => "vector",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryEntry {
    pub id: Uuid,
    pub layer: MemoryLayerKind,
    pub key: String,
    pub content: serde_json::Value,
    /// Optional dense vector for semantic search (used by the Vector layer).
    #[serde(default)]
    pub embedding: Option<Vec<f32>>,
    pub created_at: DateTime<Utc>,
    #[serde(default)]
    pub tags: Vec<String>,
}

impl MemoryEntry {
    pub fn new(layer: MemoryLayerKind, key: impl Into<String>, content: serde_json::Value) -> Self {
        Self {
            id: Uuid::new_v4(),
            layer,
            key: key.into(),
            content,
            embedding: None,
            created_at: Utc::now(),
            tags: Vec::new(),
        }
    }

    pub fn with_embedding(mut self, embedding: Vec<f32>) -> Self {
        self.embedding = Some(embedding);
        self
    }

    pub fn with_tags(mut self, tags: Vec<String>) -> Self {
        self.tags = tags;
        self
    }
}

/// A single memory tier. Implementations may be in-memory, SQLite, a vector
/// database, or a graph store.
#[async_trait]
pub trait MemoryLayer: Send + Sync {
    fn kind(&self) -> MemoryLayerKind;

    async fn store(&self, entry: &MemoryEntry) -> crate::error::Result<()>;

    async fn retrieve(&self, key: &str) -> crate::error::Result<Option<MemoryEntry>>;

    async fn search(&self, query: &str, limit: usize) -> crate::error::Result<Vec<MemoryEntry>>;
}

/// Aggregates all memory layers and routes operations to the appropriate tier
/// by [`MemoryLayerKind`]. Mirrors how humans move information between
/// short-term and long-term recall.
pub struct MemoryEngine {
    layers: Vec<Box<dyn MemoryLayer>>,
}

impl Default for MemoryEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl MemoryEngine {
    pub fn new() -> Self {
        Self { layers: Vec::new() }
    }

    pub fn register_layer(&mut self, layer: Box<dyn MemoryLayer>) {
        self.layers.push(layer);
    }

    fn layer(&self, kind: MemoryLayerKind) -> Option<&dyn MemoryLayer> {
        self.layers
            .iter()
            .find(|l| l.kind() == kind)
            .map(|b| b.as_ref())
    }

    pub async fn store(&self, entry: &MemoryEntry) -> crate::error::Result<()> {
        match self.layer(entry.layer) {
            Some(l) => l.store(entry).await,
            None => Err(crate::error::FridayError::Memory(
                entry.layer.as_str().to_string(),
                "no layer registered".into(),
            )),
        }
    }

    pub async fn retrieve(
        &self,
        kind: MemoryLayerKind,
        key: &str,
    ) -> crate::error::Result<Option<MemoryEntry>> {
        match self.layer(kind) {
            Some(l) => l.retrieve(key).await,
            None => Ok(None),
        }
    }

    pub async fn search(
        &self,
        kind: MemoryLayerKind,
        query: &str,
        limit: usize,
    ) -> crate::error::Result<Vec<MemoryEntry>> {
        match self.layer(kind) {
            Some(l) => l.search(query, limit).await,
            None => Ok(Vec::new()),
        }
    }

    /// Construct an engine pre-loaded with the four standard tiers, with the
    /// durable tiers persisted under `dir`. Creates `dir` if missing.
    pub fn default_layers(dir: &Path) -> crate::error::Result<Self> {
        std::fs::create_dir_all(dir)
            .map_err(|e| crate::error::FridayError::Memory("init".into(), e.to_string()))?;
        let mut engine = MemoryEngine::new();
        engine.register_layer(Box::new(WorkingMemory::new()));
        engine.register_layer(Box::new(SessionMemory::open(&dir.join("session.redb"))?));
        engine.register_layer(Box::new(LongTermMemory::open(&dir.join("longterm.redb"))?));
        engine.register_layer(Box::new(VectorMemory::new()));
        Ok(engine)
    }
}
