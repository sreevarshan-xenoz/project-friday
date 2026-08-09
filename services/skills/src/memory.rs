//! Memory-aware skills (M8) — the controlled, deterministic memory surface.
//!
//! Skills stay pure data: they *declare* which memory keys they read and
//! write in `skill.json` (`"memory": { "reads": [...], "writes": [...] }`),
//! and the runtime hands each skill a [`SkillMemory`] handle that only allows
//! exactly those keys. The result is the reviewer's `Memory.search() →
//! Planner → Skill → Execution → Memory.store()` loop without ever asking the
//! model to \"remember\" something:
//!
//! * `search(query, limit)` — free-text recall **scoped to the skill's
//!   declared `memory.reads` keys** (a skill can only ever retrieve content
//!   under keys it declared — no cross-skill exfiltration by searching).
//! * `read(key)` — deterministic keyed recall, allowed only for keys the
//!   skill declared in `memory.reads`.
//! * `store(key, value)` — durable persistence, allowed only for keys the
//!   skill declared in `memory.writes`.
//!
//! The actual store is abstracted behind [`SkillMemorySource`] so this crate
//! stays decoupled from `friday-core`; integration adapters (e.g.
//! `friday-context`'s `CoreMemorySource`) wrap the real `MemoryEngine`.

use async_trait::async_trait;
use std::sync::Arc;

use crate::registry::{Skill, SkillRegistry};

/// One recall result from a skill's memory view.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemoryHit {
    pub key: String,
    pub value: String,
}

/// The controlled memory surface a skill can use. Implemented at integration
/// points over the runtime's real memory engine; this crate only defines the
/// contract so skills and tests are decoupled from the storage backend.
#[async_trait]
pub trait SkillMemorySource: Send + Sync {
    /// Free-text recall across the durable layers (long-term + session).
    async fn search(&self, query: &str, limit: usize) -> Vec<MemoryHit>;

    /// Deterministic keyed recall. `None` when the key was never stored.
    async fn read(&self, key: &str) -> Option<MemoryHit>;

    /// Persist a durable fact under `key`.
    async fn store(&self, key: &str, value: &str) -> Result<(), String>;
}

/// Why a memory operation was denied by the skill gate.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum MemoryError {
    /// The skill is disabled — disabled skills cannot touch memory.
    #[error("skill is disabled")]
    SkillDisabled,
    /// `read` for a key the skill did not declare in `memory.reads`.
    #[error("key '{0}' is not declared in the skill's memory.reads")]
    UndeclaredRead(String),
    /// `store` for a key the skill did not declare in `memory.writes`.
    #[error("key '{0}' is not declared in the skill's memory.writes")]
    UndeclaredWrite(String),
    /// The underlying source rejected the write.
    #[error("memory store failed: {0}")]
    StoreFailed(String),
}

/// The memory handle handed to **one** skill. Every operation is checked
/// against that skill's **live** registry state (never a snapshot — a skill
/// disabled after the handle was created is denied immediately) and its
/// declared memory keys, so a skill can read only keys it declared, write
/// only keys it declared, and search only within its declared read keys.
/// Constructed by the integration point
/// (`SkillEngineWiring::memory_for`).
pub struct SkillMemory {
    source: Arc<dyn SkillMemorySource>,
    registry: Arc<SkillRegistry>,
    skill_id: String,
}

impl SkillMemory {
    /// Wrap `source` for one registered skill id. `registry` is kept so every
    /// operation re-reads the skill's current enabled state and declared
    /// keys instead of trusting a load-time snapshot.
    pub fn new(
        source: Arc<dyn SkillMemorySource>,
        registry: Arc<SkillRegistry>,
        skill_id: &str,
    ) -> Self {
        Self {
            source,
            registry,
            skill_id: skill_id.to_string(),
        }
    }

    /// The skill this handle belongs to.
    pub fn skill_id(&self) -> &str {
        &self.skill_id
    }

    /// Free-text recall, **scoped to the skill's declared `memory.reads`**
    /// keys — a skill can only ever retrieve content under keys it declared,
    /// so a forge-installed skill cannot exfiltrate other skills' or the
    /// system's memory by searching. A disabled skill sees nothing.
    pub async fn search(&self, query: &str, limit: usize) -> Vec<MemoryHit> {
        let Some(skill) = self.live() else {
            return Vec::new();
        };
        if !skill.enabled {
            return Vec::new();
        }
        let allowed: Vec<&str> = skill
            .manifest
            .memory
            .reads
            .iter()
            .map(String::as_str)
            .collect();
        self.source
            .search(query, limit)
            .await
            .into_iter()
            .filter(|h| allowed.contains(&h.key.as_str()))
            .take(limit)
            .collect()
    }

    /// Deterministic keyed recall. `Err` = the skill may not read that key;
    /// `Ok(None)` = allowed but never stored.
    pub async fn read(&self, key: &str) -> Result<Option<MemoryHit>, MemoryError> {
        let skill = self.live().ok_or(MemoryError::SkillDisabled)?;
        if !skill.enabled {
            return Err(MemoryError::SkillDisabled);
        }
        if !skill.manifest.memory.reads.iter().any(|k| k == key) {
            return Err(MemoryError::UndeclaredRead(key.to_string()));
        }
        Ok(self.source.read(key).await)
    }

    /// Persist a durable fact under a key the skill declared in
    /// `memory.writes`. Enforced here, never in the store. Reads the skill's
    /// live state, so disabling the skill mid-session revokes the handle.
    pub async fn store(&self, key: &str, value: &str) -> Result<(), MemoryError> {
        let skill = self.live().ok_or(MemoryError::SkillDisabled)?;
        if !skill.enabled {
            return Err(MemoryError::SkillDisabled);
        }
        if !skill.manifest.memory.writes.iter().any(|k| k == key) {
            return Err(MemoryError::UndeclaredWrite(key.to_string()));
        }
        self.source
            .store(key, value)
            .await
            .map_err(MemoryError::StoreFailed)
    }

    /// The skill's current registry state (registered + live fields).
    fn live(&self) -> Option<Skill> {
        self.registry.get(&self.skill_id).map(|s| (*s).clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::{SkillManifest, SkillMemorySpec};
    use crate::registry::{CapabilityStatus, Skill, SkillRegistry, SkillSource};
    use chrono::Utc;
    use std::collections::HashMap;
    use std::sync::Mutex;

    struct MockMemory {
        store: Mutex<HashMap<String, String>>,
    }

    impl MockMemory {
        fn new() -> Self {
            Self {
                store: Mutex::new(HashMap::new()),
            }
        }
    }

    #[async_trait]
    impl SkillMemorySource for MockMemory {
        async fn search(&self, query: &str, limit: usize) -> Vec<MemoryHit> {
            self.store
                .lock()
                .unwrap()
                .iter()
                .filter(|(k, v)| k.contains(query) || v.contains(query))
                .take(limit)
                .map(|(k, v)| MemoryHit {
                    key: k.clone(),
                    value: v.clone(),
                })
                .collect()
        }

        async fn read(&self, key: &str) -> Option<MemoryHit> {
            self.store.lock().unwrap().get(key).map(|v| MemoryHit {
                key: key.to_string(),
                value: v.clone(),
            })
        }

        async fn store(&self, key: &str, value: &str) -> Result<(), String> {
            self.store
                .lock()
                .unwrap()
                .insert(key.to_string(), value.to_string());
            Ok(())
        }
    }

    fn skill_with_memory(
        registry: &SkillRegistry,
        id: &str,
        reads: &[&str],
        writes: &[&str],
        enabled: bool,
    ) {
        let manifest = SkillManifest {
            id: id.to_string(),
            name: id.to_string(),
            version: "1.0.0".to_string(),
            description: String::new(),
            author: "test".to_string(),
            category: "gaming".to_string(),
            icon: String::new(),
            capabilities: vec!["terminal.exec".to_string()],
            actions: vec!["do it".to_string()],
            permissions: vec![],
            readonly: false,
            dependencies: vec![],
            platform: "Any".to_string(),
            min_core_version: "0.0.0".to_string(),
            enabled: true,
            memory: SkillMemorySpec {
                reads: reads.iter().map(|s| s.to_string()).collect(),
                writes: writes.iter().map(|s| s.to_string()).collect(),
            },
        };
        registry
            .register(Skill {
                manifest,
                source: SkillSource::Builtin,
                loaded_at: Utc::now(),
                enabled,
                capabilities: vec![CapabilityStatus::Resolved("terminal.exec".to_string())],
            })
            .unwrap();
    }

    fn handle(registry: &Arc<SkillRegistry>, source: Arc<MockMemory>, id: &str) -> SkillMemory {
        SkillMemory::new(source, Arc::clone(registry), id)
    }

    #[tokio::test]
    async fn declared_write_key_is_allowed_and_search_finds_it() {
        let registry = Arc::new(SkillRegistry::new());
        skill_with_memory(
            &registry,
            "minecraft",
            &["minecraft.server_state"],
            &["minecraft.server_state"],
            true,
        );
        let source = Arc::new(MockMemory::new());
        let memory = handle(&registry, Arc::clone(&source), "minecraft");

        memory
            .store("minecraft.server_state", "running")
            .await
            .unwrap();
        assert_eq!(
            memory
                .read("minecraft.server_state")
                .await
                .unwrap()
                .unwrap()
                .value,
            "running"
        );
        let hits = memory.search("running", 5).await;
        assert!(hits.iter().any(|h| h.key == "minecraft.server_state"));
    }

    #[tokio::test]
    async fn undeclared_keys_are_rejected() {
        let registry = Arc::new(SkillRegistry::new());
        skill_with_memory(
            &registry,
            "minecraft",
            &["minecraft.server_state"],
            &["minecraft.server_state"],
            true,
        );
        let source = Arc::new(MockMemory::new());
        let memory = handle(&registry, Arc::clone(&source), "minecraft");

        // Write to an undeclared key → denied, nothing persisted.
        assert_eq!(
            memory.store("user.credentials", "hunter2").await,
            Err(MemoryError::UndeclaredWrite("user.credentials".to_string()))
        );
        // Read a key not in memory.reads → denied.
        assert_eq!(
            memory.read("server.logs").await,
            Err(MemoryError::UndeclaredRead("server.logs".to_string()))
        );
        // Allowed-but-never-stored → Ok(None).
        assert_eq!(memory.read("minecraft.server_state").await.unwrap(), None);
    }

    #[tokio::test]
    async fn disabled_skill_cannot_touch_memory() {
        let registry = Arc::new(SkillRegistry::new());
        skill_with_memory(
            &registry,
            "git",
            &["git.last_workspace"],
            &["git.last_workspace"],
            true,
        );
        let source = Arc::new(MockMemory::new());
        registry.disable("git").unwrap();
        let memory = handle(&registry, Arc::clone(&source), "git");

        assert_eq!(
            memory.store("git.last_workspace", "x").await,
            Err(MemoryError::SkillDisabled)
        );
        assert_eq!(
            memory.read("git.last_workspace").await,
            Err(MemoryError::SkillDisabled)
        );
        assert!(memory.search("git", 5).await.is_empty());
    }

    #[tokio::test]
    async fn disabling_after_handle_creation_revokes_memory_immediately() {
        // The gate must read LIVE registry state — a handle created while the
        // skill was enabled keeps nothing once the skill is disabled.
        let registry = Arc::new(SkillRegistry::new());
        skill_with_memory(
            &registry,
            "git",
            &["git.last_workspace"],
            &["git.last_workspace"],
            true,
        );
        let source = Arc::new(MockMemory::new());
        let memory = handle(&registry, Arc::clone(&source), "git");

        // Handle works while the skill is enabled.
        memory.store("git.last_workspace", "x").await.unwrap();

        // Disable AFTER the handle exists → the same handle is denied.
        registry.disable("git").unwrap();
        assert_eq!(
            memory.store("git.last_workspace", "y").await,
            Err(MemoryError::SkillDisabled)
        );
        assert_eq!(
            memory.read("git.last_workspace").await,
            Err(MemoryError::SkillDisabled)
        );
        assert!(memory.search("x", 5).await.is_empty());
    }

    #[tokio::test]
    async fn search_is_scoped_to_declared_read_keys() {
        // Search must never leak entries under keys the skill did not declare
        // — even when the store holds them.
        let registry = Arc::new(SkillRegistry::new());
        skill_with_memory(
            &registry,
            "minecraft",
            &["minecraft.server_state"],
            &["minecraft.server_state"],
            true,
        );
        let source = Arc::new(MockMemory::new());
        // The store holds a sensitive key the skill never declared.
        source
            .store("user.credentials", "hunter2")
            .await
            .expect("seed undeclared key");
        let memory = handle(&registry, Arc::clone(&source), "minecraft");

        let hits = memory.search("hunter2", 5).await;
        assert!(
            hits.is_empty(),
            "search must not leak undeclared keys: {hits:?}"
        );
    }
}
