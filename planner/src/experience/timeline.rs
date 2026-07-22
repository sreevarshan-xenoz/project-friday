//! Append-only, durable timeline store backed by redb.
//!
//! The Timeline is the canonical source of truth for the Experience Layer.
//! Entries are never updated or deleted — only appended. This makes the
//! timeline trivially testable and trivially debuggable.
//!
//! Follows the same persistence pattern as [`crate::execution::persistence::GoalStore`]:
//! synchronous redb wrapper, JSON-serialized entries, append-only log.

use std::path::Path;
use std::sync::Mutex;

use chrono::Utc;
use redb::{Database, ReadableTable, ReadableTableMetadata, TableDefinition};
use uuid::Uuid;

use super::{ReflectionRecord, TimelineEntry, Workflow};

const TIMELINE_TABLE: TableDefinition<&str, &str> = TableDefinition::new("timeline_entries");
const WORKFLOW_TABLE: TableDefinition<&str, &str> = TableDefinition::new("workflows");
const REFLECTION_TABLE: TableDefinition<&str, &str> = TableDefinition::new("reflections");

/// Durable, append-only timeline store. Implements ADR-0008 invariants:
/// - Append-only (no updates, no deletes)
/// - Stores semantics (capability identifiers, not raw UI events)
/// - Reflection produces separate records (never edits timeline)
pub struct TimelineStore {
    db: Mutex<Database>,
}

impl TimelineStore {
    pub fn open(path: &Path) -> anyhow::Result<Self> {
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() && !parent.exists() {
                std::fs::create_dir_all(parent)
                    .map_err(|e| anyhow::anyhow!("timeline store mkdir {parent:?}: {e}"))?;
            }
        }
        let db = Database::create(path)
            .map_err(|e| anyhow::anyhow!("timeline store open failed: {e}"))?;
        Ok(Self { db: Mutex::new(db) })
    }

    // -- Timeline (append-only) --

    /// Append one entry to the timeline. Returns the entry's id.
    pub fn append(&self, mut entry: TimelineEntry) -> anyhow::Result<Uuid> {
        let id = entry.id;
        let db = self.db.lock().expect("timeline store poisoned");
        let txn = db
            .begin_write()
            .map_err(|e| anyhow::anyhow!("begin_write: {e}"))?;
        {
            let mut table = txn
                .open_table(TIMELINE_TABLE)
                .map_err(|e| anyhow::anyhow!("open_table: {e}"))?;
            let seq = table.len().map_err(|e| anyhow::anyhow!("len: {e}"))?;
            let key = format!("{}-{:016}", entry.goal_id, seq);
            let json = serde_json::to_string(&entry)?;
            table
                .insert(key.as_str(), json.as_str())
                .map_err(|e| anyhow::anyhow!("insert: {e}"))?;
        }
        txn.commit()
            .map_err(|e| anyhow::anyhow!("commit: {e}"))?;
        Ok(id)
    }

    /// Create and append a timeline entry in one call. Returns the entry.
    #[allow(clippy::too_many_arguments)]
    pub fn record(
        &self,
        goal_id: Uuid,
        plan_id: Uuid,
        execution_id: Uuid,
        event_kind: &str,
        capability: &str,
        metadata: serde_json::Value,
        parent: Option<Uuid>,
    ) -> anyhow::Result<TimelineEntry> {
        let entry = TimelineEntry {
            id: Uuid::new_v4(),
            timestamp: Utc::now(),
            goal_id,
            plan_id,
            execution_id,
            event_kind: event_kind.to_string(),
            capability: capability.to_string(),
            metadata,
            parent,
        };
        self.append(entry.clone())?;
        Ok(entry)
    }

    /// All entries for a given goal, in insertion order.
    pub fn entries_for_goal(&self, goal_id: Uuid) -> anyhow::Result<Vec<TimelineEntry>> {
        let db = self.db.lock().expect("timeline store poisoned");
        let txn = db
            .begin_read()
            .map_err(|e| anyhow::anyhow!("begin_read: {e}"))?;
        let table = txn
            .open_table(TIMELINE_TABLE)
            .map_err(|e| anyhow::anyhow!("open_table: {e}"))?;
        let goal_prefix = format!("{}-", goal_id);
        let mut entries = Vec::new();
        for kv in table
            .iter()
            .map_err(|e| anyhow::anyhow!("iter: {e}"))?
        {
            let (k, v) = kv.map_err(|e| anyhow::anyhow!("kv: {e}"))?;
            if k.value().starts_with(&goal_prefix) {
                let entry: TimelineEntry = serde_json::from_str(v.value())?;
                entries.push(entry);
            }
        }
        Ok(entries)
    }

    /// All entries, in insertion order. Use sparingly — prefer goal-scoped queries.
    pub fn all_entries(&self) -> anyhow::Result<Vec<TimelineEntry>> {
        let db = self.db.lock().expect("timeline store poisoned");
        let txn = db
            .begin_read()
            .map_err(|e| anyhow::anyhow!("begin_read: {e}"))?;
        let table = txn
            .open_table(TIMELINE_TABLE)
            .map_err(|e| anyhow::anyhow!("open_table: {e}"))?;
        let mut entries = Vec::new();
        for kv in table
            .iter()
            .map_err(|e| anyhow::anyhow!("iter: {e}"))?
        {
            let (_k, v) = kv.map_err(|e| anyhow::anyhow!("kv: {e}"))?;
            let entry: TimelineEntry = serde_json::from_str(v.value())?;
            entries.push(entry);
        }
        Ok(entries)
    }

    /// Count entries for a goal.
    pub fn count_for_goal(&self, goal_id: Uuid) -> anyhow::Result<usize> {
        Ok(self.entries_for_goal(goal_id)?.len())
    }

    // -- Workflows (derived from timeline, stored for fast lookup) --

    /// Persist a workflow extracted by the Recorder.
    pub fn save_workflow(&self, workflow: &Workflow) -> anyhow::Result<()> {
        let db = self.db.lock().expect("timeline store poisoned");
        let txn = db
            .begin_write()
            .map_err(|e| anyhow::anyhow!("begin_write: {e}"))?;
        {
            let mut table = txn
                .open_table(WORKFLOW_TABLE)
                .map_err(|e| anyhow::anyhow!("open_table: {e}"))?;
            let json = serde_json::to_string(workflow)?;
            table
                .insert(workflow.id.to_string().as_str(), json.as_str())
                .map_err(|e| anyhow::anyhow!("insert: {e}"))?;
        }
        txn.commit()
            .map_err(|e| anyhow::anyhow!("commit: {e}"))?;
        Ok(())
    }

    /// Load all stored workflows.
    pub fn load_workflows(&self) -> anyhow::Result<Vec<Workflow>> {
        let db = self.db.lock().expect("timeline store poisoned");
        let txn = db
            .begin_read()
            .map_err(|e| anyhow::anyhow!("begin_read: {e}"))?;
        let table = txn
            .open_table(WORKFLOW_TABLE)
            .map_err(|e| anyhow::anyhow!("open_table: {e}"))?;
        let mut workflows = Vec::new();
        for kv in table
            .iter()
            .map_err(|e| anyhow::anyhow!("iter: {e}"))?
        {
            let (_k, v) = kv.map_err(|e| anyhow::anyhow!("kv: {e}"))?;
            let w: Workflow = serde_json::from_str(v.value())?;
            workflows.push(w);
        }
        Ok(workflows)
    }

    // -- Reflection records (derived from timeline, stored separately) --

    /// Persist a reflection record. Reflection never edits the timeline — it
    /// produces these as separate artifacts (ADR-0008 invariant 5).
    pub fn save_reflection(&self, record: &ReflectionRecord) -> anyhow::Result<()> {
        let db = self.db.lock().expect("timeline store poisoned");
        let txn = db
            .begin_write()
            .map_err(|e| anyhow::anyhow!("begin_write: {e}"))?;
        {
            let mut table = txn
                .open_table(REFLECTION_TABLE)
                .map_err(|e| anyhow::anyhow!("open_table: {e}"))?;
            let json = serde_json::to_string(record)?;
            table
                .insert(record.id.to_string().as_str(), json.as_str())
                .map_err(|e| anyhow::anyhow!("insert: {e}"))?;
        }
        txn.commit()
            .map_err(|e| anyhow::anyhow!("commit: {e}"))?;
        Ok(())
    }

    /// Load all reflection records.
    pub fn load_reflections(&self) -> anyhow::Result<Vec<ReflectionRecord>> {
        let db = self.db.lock().expect("timeline store poisoned");
        let txn = db
            .begin_read()
            .map_err(|e| anyhow::anyhow!("begin_read: {e}"))?;
        let table = txn
            .open_table(REFLECTION_TABLE)
            .map_err(|e| anyhow::anyhow!("open_table: {e}"))?;
        let mut records = Vec::new();
        for kv in table
            .iter()
            .map_err(|e| anyhow::anyhow!("iter: {e}"))?
        {
            let (_k, v) = kv.map_err(|e| anyhow::anyhow!("kv: {e}"))?;
            let r: ReflectionRecord = serde_json::from_str(v.value())?;
            records.push(r);
        }
        Ok(records)
    }
}
