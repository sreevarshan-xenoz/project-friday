use std::path::Path;
use std::sync::Mutex;

use redb::{Database, ReadableTable, TableDefinition};

use crate::error::{FridayError, Result};
use crate::memory::{MemoryEntry, MemoryLayer, MemoryLayerKind};

const TABLE: TableDefinition<&str, &str> = TableDefinition::new("memory");

/// Cosine similarity between two equal-length vectors. Returns 0.0 when either
/// vector is empty or lengths differ.
pub fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }
    let dot: f32 = a.iter().zip(b).map(|(x, y)| x * y).sum();
    let na: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let nb: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();
    if na == 0.0 || nb == 0.0 {
        0.0
    } else {
        dot / (na * nb)
    }
}

/// A durable, ACID key-value store backed by **redb**. Used for the Session and
/// Long-Term memory tiers. Storage is opaque JSON so any `MemoryEntry` can be
/// persisted; the layer kind is enforced on write.
struct RedbStore {
    db: Database,
    kind: MemoryLayerKind,
}

impl RedbStore {
    fn open(path: &Path, kind: MemoryLayerKind) -> Result<Self> {
        let db = Database::create(path)
            .map_err(|e| FridayError::Memory(kind.as_str().to_string(), e.to_string()))?;
        Ok(Self { db, kind })
    }

    fn put(&self, entry: &MemoryEntry) -> Result<()> {
        let mut entry = entry.clone();
        entry.layer = self.kind; // normalise to this tier
        let json = serde_json::to_string(&entry)?;
        let txn = self
            .db
            .begin_write()
            .map_err(|e| FridayError::Memory(self.kind.as_str().to_string(), e.to_string()))?;
        {
            let mut table = txn
                .open_table(TABLE)
                .map_err(|e| FridayError::Memory(self.kind.as_str().to_string(), e.to_string()))?;
            table
                .insert(entry.key.as_str(), json.as_str())
                .map_err(|e| FridayError::Memory(self.kind.as_str().to_string(), e.to_string()))?;
        }
        txn.commit()
            .map_err(|e| FridayError::Memory(self.kind.as_str().to_string(), e.to_string()))?;
        Ok(())
    }

    fn get(&self, key: &str) -> Result<Option<MemoryEntry>> {
        let txn = self
            .db
            .begin_read()
            .map_err(|e| FridayError::Memory(self.kind.as_str().to_string(), e.to_string()))?;
        let table = txn
            .open_table(TABLE)
            .map_err(|e| FridayError::Memory(self.kind.as_str().to_string(), e.to_string()))?;
        match table
            .get(key)
            .map_err(|e| FridayError::Memory(self.kind.as_str().to_string(), e.to_string()))?
        {
            Some(v) => {
                let entry: MemoryEntry =
                    serde_json::from_str(v.value()).map_err(FridayError::from)?;
                Ok(Some(entry))
            }
            None => Ok(None),
        }
    }

    fn scan(&self, query: &str, limit: usize) -> Result<Vec<MemoryEntry>> {
        let q = query.to_lowercase();
        let txn = self
            .db
            .begin_read()
            .map_err(|e| FridayError::Memory(self.kind.as_str().to_string(), e.to_string()))?;
        let table = txn
            .open_table(TABLE)
            .map_err(|e| FridayError::Memory(self.kind.as_str().to_string(), e.to_string()))?;
        let mut out = Vec::new();
        for kv in table
            .iter()
            .map_err(|e| FridayError::Memory(self.kind.as_str().to_string(), e.to_string()))?
        {
            let (_k, v) =
                kv.map_err(|e| FridayError::Memory(self.kind.as_str().to_string(), e.to_string()))?;
            let entry: MemoryEntry = serde_json::from_str(v.value()).map_err(FridayError::from)?;
            if entry.layer != self.kind {
                continue;
            }
            let text = serde_json::to_string(&entry.content)
                .unwrap_or_default()
                .to_lowercase();
            if text.contains(&q) {
                out.push(entry);
                if out.len() >= limit {
                    break;
                }
            }
        }
        Ok(out)
    }
}

/// Ephemeral, in-process scratch space for the current turn. Fastest tier;
/// nothing is persisted.
pub struct WorkingMemory {
    store: Mutex<std::collections::HashMap<String, MemoryEntry>>,
}

impl Default for WorkingMemory {
    fn default() -> Self {
        Self::new()
    }
}

impl WorkingMemory {
    pub fn new() -> Self {
        Self {
            store: Mutex::new(std::collections::HashMap::new()),
        }
    }
}

#[async_trait::async_trait]
impl MemoryLayer for WorkingMemory {
    fn kind(&self) -> MemoryLayerKind {
        MemoryLayerKind::Working
    }

    async fn store(&self, entry: &MemoryEntry) -> Result<()> {
        let mut entry = entry.clone();
        entry.layer = MemoryLayerKind::Working;
        self.store
            .lock()
            .expect("working memory lock poisoned")
            .insert(entry.key.clone(), entry);
        Ok(())
    }

    async fn retrieve(&self, key: &str) -> Result<Option<MemoryEntry>> {
        Ok(self
            .store
            .lock()
            .expect("working memory lock poisoned")
            .get(key)
            .cloned())
    }

    async fn search(&self, query: &str, limit: usize) -> Result<Vec<MemoryEntry>> {
        let q = query.to_lowercase();
        let store = self.store.lock().expect("working memory lock poisoned");
        let mut out: Vec<MemoryEntry> = store
            .values()
            .filter(|e| {
                serde_json::to_string(&e.content)
                    .unwrap_or_default()
                    .to_lowercase()
                    .contains(&q)
            })
            .cloned()
            .collect();
        out.truncate(limit);
        Ok(out)
    }
}

/// Durable session memory (persisted to disk via redb).
pub struct SessionMemory {
    store: RedbStore,
}

impl SessionMemory {
    pub fn open(path: &Path) -> Result<Self> {
        Ok(Self {
            store: RedbStore::open(path, MemoryLayerKind::Session)?,
        })
    }
}

#[async_trait::async_trait]
impl MemoryLayer for SessionMemory {
    fn kind(&self) -> MemoryLayerKind {
        MemoryLayerKind::Session
    }
    async fn store(&self, entry: &MemoryEntry) -> Result<()> {
        self.store.put(entry)
    }
    async fn retrieve(&self, key: &str) -> Result<Option<MemoryEntry>> {
        self.store.get(key)
    }
    async fn search(&self, query: &str, limit: usize) -> Result<Vec<MemoryEntry>> {
        self.store.scan(query, limit)
    }
}

/// Durable long-term memory (persisted to disk via redb).
pub struct LongTermMemory {
    store: RedbStore,
}

impl LongTermMemory {
    pub fn open(path: &Path) -> Result<Self> {
        Ok(Self {
            store: RedbStore::open(path, MemoryLayerKind::LongTerm)?,
        })
    }
}

#[async_trait::async_trait]
impl MemoryLayer for LongTermMemory {
    fn kind(&self) -> MemoryLayerKind {
        MemoryLayerKind::LongTerm
    }
    async fn store(&self, entry: &MemoryEntry) -> Result<()> {
        self.store.put(entry)
    }
    async fn retrieve(&self, key: &str) -> Result<Option<MemoryEntry>> {
        self.store.get(key)
    }
    async fn search(&self, query: &str, limit: usize) -> Result<Vec<MemoryEntry>> {
        self.store.scan(query, limit)
    }
}

/// In-memory vector index for semantic recall. Stores entries that carry an
/// `embedding` and answers `search_similar` by cosine similarity. The trait
/// `search` falls back to lexical matching when no embedding is supplied.
pub struct VectorMemory {
    entries: Mutex<Vec<MemoryEntry>>,
}

impl Default for VectorMemory {
    fn default() -> Self {
        Self::new()
    }
}

impl VectorMemory {
    pub fn new() -> Self {
        Self {
            entries: Mutex::new(Vec::new()),
        }
    }

    /// Top-`limit` entries by cosine similarity to `query`.
    pub fn search_similar(&self, query: &[f32], limit: usize) -> Vec<MemoryEntry> {
        let entries = self.entries.lock().expect("vector memory lock poisoned");
        let mut scored: Vec<(f32, MemoryEntry)> = entries
            .iter()
            .filter_map(|e| {
                e.embedding
                    .as_ref()
                    .map(|v| (cosine_similarity(query, v), e.clone()))
            })
            .collect();
        scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
        scored.truncate(limit);
        scored.into_iter().map(|(_, e)| e).collect()
    }
}

#[async_trait::async_trait]
impl MemoryLayer for VectorMemory {
    fn kind(&self) -> MemoryLayerKind {
        MemoryLayerKind::Vector
    }

    async fn store(&self, entry: &MemoryEntry) -> Result<()> {
        let mut entry = entry.clone();
        entry.layer = MemoryLayerKind::Vector;
        self.entries
            .lock()
            .expect("vector memory lock poisoned")
            .push(entry);
        Ok(())
    }

    async fn retrieve(&self, key: &str) -> Result<Option<MemoryEntry>> {
        Ok(self
            .entries
            .lock()
            .expect("vector memory lock poisoned")
            .iter()
            .find(|e| e.key == key)
            .cloned())
    }

    async fn search(&self, query: &str, limit: usize) -> Result<Vec<MemoryEntry>> {
        let q = query.to_lowercase();
        let entries = self.entries.lock().expect("vector memory lock poisoned");
        let mut out: Vec<MemoryEntry> = entries
            .iter()
            .filter(|e| {
                serde_json::to_string(&e.content)
                    .unwrap_or_default()
                    .to_lowercase()
                    .contains(&q)
            })
            .cloned()
            .collect();
        out.truncate(limit);
        Ok(out)
    }
}
