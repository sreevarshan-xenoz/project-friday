//! Goal lifecycle: the orchestration-level state machine and the in-memory
//! runtime that the supervisor owns.
//!
//! This module is **orchestration only** — it holds state and enforces valid
//! transitions. It contains no execution logic; the scheduler and reflection
//! engine read and mutate [`GoalRuntime`] through the methods here.

use std::collections::{HashMap, HashSet};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::planning::{GoalState, TaskGraph, TaskState};

/// Budgets that bound an autonomous loop so it can never spiral forever.
///
/// Every field is optional; `None` means "unbounded" for that dimension. The
/// supervisor checks these after each task and pauses/fails the goal when a
/// budget is exceeded.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct ExecutionBudget {
    /// Maximum number of tasks the goal may start (across retries).
    pub max_steps: Option<u32>,
    /// Maximum retries per task.
    pub max_retries: Option<u32>,
    /// Maximum cumulative model tokens the goal may spend (reserved for the
    /// reasoning capability; not enforced by the kernel directly).
    pub max_tokens: Option<u64>,
    /// Maximum estimated cost in arbitrary units.
    pub max_cost: Option<f64>,
    /// Hard deadline after which the goal must not start new work.
    pub deadline: Option<DateTime<Utc>>,
    /// Wall-clock timeout for the entire goal.
    pub timeout: Option<chrono::Duration>,
}

impl Default for ExecutionBudget {
    fn default() -> Self {
        Self {
            max_steps: Some(200),
            max_retries: Some(3),
            max_tokens: None,
            max_cost: None,
            deadline: None,
            timeout: Some(chrono::Duration::hours(2)),
        }
    }
}

/// The live, orchestration-level state of one goal.
///
/// It owns the plan DAG plus the completion/retry bookkeeping. The supervisor
/// keeps one of these per `goal_id` (and persists it — see
/// [`crate::execution::persistence`]).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GoalRuntime {
    pub goal_id: Uuid,
    /// Correlation id shared with the whole `goal.raw` → `plan.created` → …
    /// pipeline so every event can be tied back to this goal.
    pub trace_id: Uuid,
    pub intent: String,
    pub state: GoalState,
    pub graph: TaskGraph,
    /// Tasks that have completed successfully (used for DAG readiness).
    pub completed: HashSet<Uuid>,
    /// Tasks currently executing (for cancellation fan-out).
    pub running: HashSet<Uuid>,
    /// Per-task retry counters (the source of truth; `Task.retries` mirrors it).
    pub retries: HashMap<Uuid, u32>,
    /// Running count of tasks started (compared against `budget.max_steps`).
    pub steps_taken: u32,
    pub budget: ExecutionBudget,
    pub created_at: DateTime<Utc>,
    /// Why the goal paused (e.g. a high-risk capability awaiting approval).
    pub pending_capability: Option<String>,
    /// The specific task awaiting approval when paused for a high-risk gate.
    pub pending_task: Option<Uuid>,
    /// Shared, goal-scoped execution context. Tasks publish computed artifacts
    /// here (e.g. `cargo metadata` → `workspace_root`, `members`) and later
    /// tasks consume them, so the same work is never recomputed. Keyed by a
    /// logical name; the producing task id is recorded for provenance/trace.
    ///
    /// This is the M6.3 shared context — it is part of the persisted goal state
    /// so a resumed goal keeps its accumulated knowledge.
    pub context: GoalContext,
}

/// A single published value in the goal-scoped execution context.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextEntry {
    /// Task that produced this value.
    pub produced_by: Uuid,
    /// When it was published.
    pub at: DateTime<Utc>,
    /// The value itself (any JSON-serializable artifact).
    pub value: serde_json::Value,
}

/// Goal-scoped, mutable bag of shared artifacts that tasks publish into and
/// later tasks read from. Immutable per key once published (re-publishing the
/// same key overwrites, which is intentional for idempotent recomputation).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GoalContext {
    entries: HashMap<String, ContextEntry>,
}

impl GoalContext {
    /// Publish a named value produced by `task_id`. Overwrites any prior value
    /// under the same key.
    pub fn publish(&mut self, key: impl Into<String>, produced_by: Uuid, value: serde_json::Value) {
        self.entries.insert(
            key.into(),
            ContextEntry {
                produced_by,
                at: Utc::now(),
                value,
            },
        );
    }

    /// Read a previously published value (cloned). Returns `None` if absent.
    pub fn get(&self, key: &str) -> Option<serde_json::Value> {
        self.entries.get(key).map(|e| e.value.clone())
    }

    /// All published keys (useful for debugging / UI / replay).
    pub fn keys(&self) -> Vec<String> {
        self.entries.keys().cloned().collect()
    }

    /// Number of published artifacts.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

impl GoalRuntime {
    pub fn new(
        goal_id: Uuid,
        trace_id: Uuid,
        intent: impl Into<String>,
        budget: ExecutionBudget,
    ) -> Self {
        Self {
            goal_id,
            trace_id,
            intent: intent.into(),
            state: GoalState::Running,
            graph: TaskGraph::new(""),
            completed: HashSet::new(),
            running: HashSet::new(),
            retries: HashMap::new(),
            steps_taken: 0,
            budget,
            created_at: Utc::now(),
            pending_capability: None,
            pending_task: None,
            context: GoalContext::default(),
        }
    }

    /// Attach the plan produced by the planning engine.
    pub fn set_plan(&mut self, graph: TaskGraph) {
        self.graph = graph;
        // A freshly planned task starts `Pending`; the scheduler promotes to
        // `Ready` when its dependencies are satisfied.
        for t in self.graph.tasks.iter_mut() {
            t.state = TaskState::Pending;
        }
    }

    /// Tasks whose dependencies are all completed and that are not yet done.
    pub fn ready_tasks(&self) -> Vec<Uuid> {
        self.graph
            .tasks
            .iter()
            .filter(|t| {
                !self.completed.contains(&t.id)
                    && !self.running.contains(&t.id)
                    && t.state != TaskState::Completed
                    && t.state != TaskState::Failed
                    && t.depends_on.iter().all(|d| self.completed.contains(d))
            })
            .map(|t| t.id)
            .collect()
    }

    pub fn mark_running(&mut self, task_id: Uuid) {
        if let Some(t) = self.graph.task_mut(task_id) {
            t.state = TaskState::Running;
        }
        self.running.insert(task_id);
        self.steps_taken += 1;
    }

    pub fn mark_verifying(&mut self, task_id: Uuid) {
        if let Some(t) = self.graph.task_mut(task_id) {
            t.state = TaskState::Verifying;
        }
    }

    pub fn mark_completed(&mut self, task_id: Uuid, output: serde_json::Value) {
        self.completed.insert(task_id);
        self.running.remove(&task_id);
        if let Some(t) = self.graph.task_mut(task_id) {
            t.state = TaskState::Completed;
            t.progress = 1.0;
            t.output = output;
        }
    }

    pub fn mark_failed(&mut self, task_id: Uuid) {
        self.running.remove(&task_id);
        if let Some(t) = self.graph.task_mut(task_id) {
            t.state = TaskState::Failed;
        }
    }

    pub fn inc_retries(&mut self, task_id: Uuid) -> u32 {
        let n = self.retries.entry(task_id).or_insert(0);
        *n += 1;
        if let Some(t) = self.graph.task_mut(task_id) {
            t.retries = *n;
            t.state = TaskState::Retrying;
        }
        *n
    }

    pub fn retries_of(&self, task_id: Uuid) -> u32 {
        *self.retries.get(&task_id).unwrap_or(&0)
    }

    /// True when every task in the graph has completed.
    pub fn is_complete(&self) -> bool {
        self.graph
            .tasks
            .iter()
            .all(|t| self.completed.contains(&t.id))
            || self.graph.tasks.is_empty()
    }

    /// Validate and apply a goal-level transition. Returns false (and leaves
    /// state unchanged) on an illegal transition.
    pub fn transition(&mut self, next: GoalState) -> bool {
        let ok = matches!(
            (self.state, next),
            (GoalState::Running, GoalState::Paused)
                | (GoalState::Running, GoalState::Completed)
                | (GoalState::Running, GoalState::Failed)
                | (GoalState::Running, GoalState::Cancelled)
                | (GoalState::Paused, GoalState::Running)
                | (GoalState::Paused, GoalState::Cancelled)
                | (GoalState::Paused, GoalState::Failed)
        );
        if ok {
            self.state = next;
        }
        ok
    }
}
