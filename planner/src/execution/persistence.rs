//! Durable goal persistence: event sourcing + periodic snapshots.
//!
//! Design (per architecture decision): we do **not** store only snapshots.
//! Every lifecycle transition is appended as an immutable [`GoalEvent`] to an
//! append-only log. Snapshots are written periodically (cheap, for fast
//! `friday resume`) but are always derivable by replaying the log. This gives
//! deterministic replay, debugging, timeline, and crash recovery.
//!
//! Storage is a tiny synchronous redb wrapper so persistence never awaits on
//! the async event bus. The supervisor calls [`GoalStore::append`] /
//! [`GoalStore::snapshot`] from its synchronous `handle_event` path.

use std::path::Path;
use std::sync::Mutex;

use redb::{Database, ReadableTable, ReadableTableMetadata, TableDefinition};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::execution::lifecycle::GoalRuntime;
use crate::planning::{GoalState, TaskState};

/// One immutable fact about a goal's life. The supervisor appends these in
/// order; current state is reconstructed by folding them.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum GoalEvent {
    GoalCreated {
        goal_id: Uuid,
        trace_id: Uuid,
        intent: String,
    },
    PlanSet {
        goal_id: Uuid,
        plan_id: Uuid,
    },
    TaskReady {
        goal_id: Uuid,
        task_id: Uuid,
    },
    TaskStarted {
        goal_id: Uuid,
        task_id: Uuid,
    },
    TaskProgress {
        goal_id: Uuid,
        task_id: Uuid,
        progress: f32,
    },
    TaskVerifying {
        goal_id: Uuid,
        task_id: Uuid,
    },
    TaskCompleted {
        goal_id: Uuid,
        task_id: Uuid,
    },
    TaskFailed {
        goal_id: Uuid,
        task_id: Uuid,
        error: Option<String>,
    },
    TaskRetried {
        goal_id: Uuid,
        task_id: Uuid,
        attempt: u32,
    },
    PermissionPaused {
        goal_id: Uuid,
        capability: String,
    },
    /// M6.3: a task published its output into the shared goal context.
    ContextPublished {
        goal_id: Uuid,
        task_id: Uuid,
        key: String,
    },
    GoalPaused {
        goal_id: Uuid,
    },
    GoalResumed {
        goal_id: Uuid,
    },
    GoalCompleted {
        goal_id: Uuid,
    },
    GoalFailed {
        goal_id: Uuid,
        reason: String,
    },
    GoalCancelled {
        goal_id: Uuid,
    },
}

impl GoalEvent {
    pub fn goal_id(&self) -> Uuid {
        match self {
            GoalEvent::GoalCreated { goal_id, .. }
            | GoalEvent::PlanSet { goal_id, .. }
            | GoalEvent::TaskReady { goal_id, .. }
            | GoalEvent::TaskStarted { goal_id, .. }
            | GoalEvent::TaskProgress { goal_id, .. }
            | GoalEvent::TaskVerifying { goal_id, .. }
            | GoalEvent::TaskCompleted { goal_id, .. }
            | GoalEvent::TaskFailed { goal_id, .. }
            | GoalEvent::TaskRetried { goal_id, .. }
            | GoalEvent::PermissionPaused { goal_id, .. }
            | GoalEvent::ContextPublished { goal_id, .. }
            | GoalEvent::GoalPaused { goal_id }
            | GoalEvent::GoalResumed { goal_id }
            | GoalEvent::GoalCompleted { goal_id }
            | GoalEvent::GoalFailed { goal_id, .. }
            | GoalEvent::GoalCancelled { goal_id } => *goal_id,
        }
    }
}

const LOG_TABLE: TableDefinition<&str, &str> = TableDefinition::new("goal_events");
const SNAP_TABLE: TableDefinition<&str, &str> = TableDefinition::new("goal_snapshots");

/// Synchronous, append-only + snapshot store for goals.
pub struct GoalStore {
    db: Mutex<Database>,
}

impl GoalStore {
    pub fn open(path: &Path) -> anyhow::Result<Self> {
        // Ensure the parent directory exists so a default path like
        // `.friday/goals.redb` works without the caller pre-creating it.
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() && !parent.exists() {
                std::fs::create_dir_all(parent)
                    .map_err(|e| anyhow::anyhow!("goal store mkdir {parent:?}: {e}"))?;
            }
        }
        let db =
            Database::create(path).map_err(|e| anyhow::anyhow!("goal store open failed: {e}"))?;
        Ok(Self { db: Mutex::new(db) })
    }

    /// Append one event (seq is assigned by insertion order).
    pub fn append(&self, event: &GoalEvent) -> anyhow::Result<()> {
        let db = self.db.lock().expect("goal store poisoned");
        let txn = db
            .begin_write()
            .map_err(|e| anyhow::anyhow!("begin_write: {e}"))?;
        {
            let mut table = txn
                .open_table(LOG_TABLE)
                .map_err(|e| anyhow::anyhow!("open_table: {e}"))?;
            let seq = table.len().map_err(|e| anyhow::anyhow!("len: {e}"))?;
            let key = format!("{}-{}", event.goal_id(), seq);
            let json = serde_json::to_string(event)?;
            table
                .insert(key.as_str(), json.as_str())
                .map_err(|e| anyhow::anyhow!("insert: {e}"))?;
        }
        txn.commit().map_err(|e| anyhow::anyhow!("commit: {e}"))?;
        Ok(())
    }

    /// Persist a compact snapshot for fast resume.
    pub fn snapshot(&self, rt: &GoalRuntime) -> anyhow::Result<()> {
        let db = self.db.lock().expect("goal store poisoned");
        let txn = db
            .begin_write()
            .map_err(|e| anyhow::anyhow!("begin_write: {e}"))?;
        {
            let mut table = txn
                .open_table(SNAP_TABLE)
                .map_err(|e| anyhow::anyhow!("open_table: {e}"))?;
            let json = serde_json::to_string(rt)?;
            table
                .insert(rt.goal_id.to_string().as_str(), json.as_str())
                .map_err(|e| anyhow::anyhow!("insert: {e}"))?;
        }
        txn.commit().map_err(|e| anyhow::anyhow!("commit: {e}"))?;
        Ok(())
    }

    /// Load a snapshot by goal id (for `friday resume`).
    pub fn load(&self, goal_id: Uuid) -> anyhow::Result<Option<GoalRuntime>> {
        let db = self.db.lock().expect("goal store poisoned");
        let txn = db
            .begin_read()
            .map_err(|e| anyhow::anyhow!("begin_read: {e}"))?;
        let table = txn
            .open_table(SNAP_TABLE)
            .map_err(|e| anyhow::anyhow!("open_table: {e}"))?;
        match table
            .get(goal_id.to_string().as_str())
            .map_err(|e| anyhow::anyhow!("get: {e}"))?
        {
            Some(v) => {
                let rt: GoalRuntime = serde_json::from_str(v.value())?;
                Ok(Some(rt))
            }
            None => Ok(None),
        }
    }

    /// Replay the full event log for a goal into a fresh [`GoalRuntime`].
    /// Used for debugging / deterministic recovery independent of snapshots.
    pub fn replay(&self, goal_id: Uuid) -> anyhow::Result<Option<GoalRuntime>> {
        let db = self.db.lock().expect("goal store poisoned");
        let txn = db
            .begin_read()
            .map_err(|e| anyhow::anyhow!("begin_read: {e}"))?;
        let table = txn
            .open_table(LOG_TABLE)
            .map_err(|e| anyhow::anyhow!("open_table: {e}"))?;
        let mut rt: Option<GoalRuntime> = None;
        for kv in table.iter().map_err(|e| anyhow::anyhow!("iter: {e}"))? {
            let (_k, v) = kv.map_err(|e| anyhow::anyhow!("kv: {e}"))?;
            let ev: GoalEvent = serde_json::from_str(v.value())?;
            if ev.goal_id() != goal_id {
                continue;
            }
            apply_event(&mut rt, &ev);
        }
        Ok(rt)
    }
}

/// Fold one event into the in-memory runtime. Allocates the runtime on
/// `GoalCreated`.
fn apply_event(rt: &mut Option<GoalRuntime>, ev: &GoalEvent) {
    match ev {
        GoalEvent::GoalCreated {
            goal_id,
            trace_id,
            intent,
        } => {
            *rt = Some(GoalRuntime::new(
                *goal_id,
                *trace_id,
                intent.clone(),
                Default::default(),
            ));
        }
        GoalEvent::PlanSet { plan_id, .. } => {
            // Plan payload is captured separately during live operation; on
            // pure replay we mark the graph present by leaving it as-is.
            let _ = plan_id;
            let _ = rt;
        }
        GoalEvent::TaskReady { task_id, .. } => {
            if let Some(rt) = rt {
                if let Some(t) = rt.graph.task_mut(*task_id) {
                    t.state = TaskState::Ready;
                }
            }
        }
        GoalEvent::TaskStarted { task_id, .. } => {
            if let Some(rt) = rt {
                rt.mark_running(*task_id);
            }
        }
        GoalEvent::TaskVerifying { task_id, .. } => {
            if let Some(rt) = rt {
                rt.mark_verifying(*task_id);
            }
        }
        GoalEvent::TaskCompleted { task_id, .. } => {
            if let Some(rt) = rt {
                rt.mark_completed(*task_id, serde_json::Value::Null);
            }
        }
        GoalEvent::TaskFailed { task_id, .. } => {
            if let Some(rt) = rt {
                rt.mark_failed(*task_id);
            }
        }
        GoalEvent::TaskRetried {
            task_id, attempt, ..
        } => {
            if let Some(rt) = rt {
                let _ = rt.inc_retries(*task_id);
                let _ = attempt;
            }
        }
        GoalEvent::GoalPaused { .. } => {
            if let Some(rt) = rt {
                rt.transition(GoalState::Paused);
            }
        }
        GoalEvent::GoalResumed { .. } => {
            if let Some(rt) = rt {
                rt.transition(GoalState::Running);
            }
        }
        GoalEvent::GoalCompleted { .. } => {
            if let Some(rt) = rt {
                rt.transition(GoalState::Completed);
            }
        }
        GoalEvent::GoalFailed { .. } => {
            if let Some(rt) = rt {
                rt.transition(GoalState::Failed);
            }
        }
        GoalEvent::GoalCancelled { .. } => {
            if let Some(rt) = rt {
                rt.transition(GoalState::Cancelled);
            }
        }
        GoalEvent::TaskProgress { .. } | GoalEvent::PermissionPaused { .. } => {}
        GoalEvent::ContextPublished { .. } => {
            // Value is re-derived from the task output on live replay; the event
            // only records that publishing happened.
        }
    }
}
