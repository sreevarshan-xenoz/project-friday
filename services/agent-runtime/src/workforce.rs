//! Persistent Agent Workforce — the durable sibling of [`crate::AgentDirector`].
//!
//! The director proves the topology (one parent, bounded role-typed children,
//! gated results); the workforce makes the *workforce* itself durable:
//!
//! ```text
//!                 Workforce (persistent layer)
//!                      │  owns
//!        ┌─────────────┼──────────────────┐
//!        │             │                  │
//!  WorkerRegistry  TaskBoard        AgentRuntime
//!  (who exists)   (what to do)      (how it runs)
//!        │             │                  │
//!        └─────────────┼──────────────────┘
//!                      ▼
//!           isolated per-worker workspace
//!                      ▼
//!              AgentSpec{ cwd = workspace }
//!                      ▼
//!               [ the runtime runs it ]
//! ```
//!
//! The constitution's acceptance (P3 / §3.3): **"A worker can be assigned a
//! task, picked up across runs, report progress/blockers, and its workspace
//! persists."**
//!
//! Design rules:
//!
//! - **The store is the registry.** [`WorkforceStore`] persists [`WorkerRecord`]s
//!   and [`BoardTask`]s (InMemory for tests, atomic Json for real runs) —
//!   a fresh [`Workforce`] over the same store sees the same workers, tasks,
//!   and assignments. A worker *is* its record; a task *is* its record.
//! - **Roles are constraints, not implementations** (same rule as the
//!   director). A task carries a `required_role`; assignment refuses a
//!   role mismatch. The worker's behavior comes from the [`crate::AgentDriver`]
//!   the caller supplies (production: `SkillAgentDriver`).
//! - **Isolated workspaces.** Every registered worker gets its own directory
//!   under the workforce's workspace root. When the task runs, `spec.cwd` is
//!   that workspace — the worker's actions genuinely operate in *its* space,
//!   and the space persists across runs and across instances.
//! - **Honest progress.** [`Workforce::report_progress`] and
//!   [`Workforce::report_blocker`] write notes onto the worker's current task
//!   (a blocker also flips the task to `Blocked`); nothing is fabricated —
//!   a task with no notes has no notes. `TaskOutcome` reflects the runtime's
//!   [`crate::AgentState`]: only `Succeeded` yields `Done`.
//! - **Runs are bounded.** The workforce owns the same runtime bounds as the
//!   director (pool cap, deadline, step budget) so a persistent worker can
//!   never run unbounded.
//! - **No re-execution of done work.** A `Done`/`Failed` task is terminal; a
//!   worker that finished is `Idle` again and can be assigned new work.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::director::AgentRole;
use crate::driver::AgentDriver;
use crate::runtime::AgentRuntime;
use crate::spec::{AgentResult, AgentSpec};

// ── Data model ─────────────────────────────────────────────────────────

/// Lifecycle of a registered worker. `Idle → Assigned → Running → Idle`,
/// with `Suspended` pausing an assigned worker and `Retired` terminal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum WorkerStatus {
    Idle,
    Assigned,
    Running,
    Suspended,
    Retired,
}

impl WorkerStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            WorkerStatus::Idle => "idle",
            WorkerStatus::Assigned => "assigned",
            WorkerStatus::Running => "running",
            WorkerStatus::Suspended => "suspended",
            WorkerStatus::Retired => "retired",
        }
    }
}

/// Lifecycle of a task on the assignable board.
/// `Open → Assigned → InProgress → Done | Failed`, with `Blocked` when a
/// worker reports a blocker (resolvable back to `Assigned`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TaskStatus {
    Open,
    Assigned,
    InProgress,
    Blocked,
    Done,
    Failed,
}

impl TaskStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            TaskStatus::Open => "open",
            TaskStatus::Assigned => "assigned",
            TaskStatus::InProgress => "in_progress",
            TaskStatus::Blocked => "blocked",
            TaskStatus::Done => "done",
            TaskStatus::Failed => "failed",
        }
    }
}

/// One progress/blocker note on a task — the honest audit trail of "what has
/// this worker said about its work".
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BoardNote {
    pub at_ms: u64,
    pub note: String,
}

/// The persistent registry entry: who the worker is and where it lives.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkerRecord {
    pub id: String,
    pub role: AgentRole,
    /// Human/planner-readable description of this worker's specialty.
    pub description: String,
    pub status: WorkerStatus,
    /// The task currently bound to this worker (if any).
    #[serde(default)]
    pub current_task: Option<String>,
    /// The isolated workspace for this worker — persists across runs.
    pub workspace: PathBuf,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
}

/// The assignable board entry: what a worker is asked to do.
///
/// Not `PartialEq` — it carries the run's [`AgentResult`] which is
/// equality-free by design (results are evidence, not values).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BoardTask {
    pub id: String,
    pub title: String,
    /// The actual instruction the worker runs against.
    pub goal: String,
    /// The role allowed to pick this task up.
    pub required_role: AgentRole,
    pub status: TaskStatus,
    /// Id of the assigned worker (None while Open).
    #[serde(default)]
    pub assignee: Option<String>,
    /// Capability params for the run (keyed by canonical capability id),
    /// applied to the `AgentSpec` when the task runs.
    #[serde(default)]
    pub params: serde_json::Map<String, serde_json::Value>,
    #[serde(default)]
    pub progress: Vec<BoardNote>,
    #[serde(default)]
    pub blockers: Vec<BoardNote>,
    /// The persisted result of the last run (None until the task runs).
    #[serde(default)]
    pub result: Option<AgentResult>,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
}

// ── Persistence seam ───────────────────────────────────────────────────

/// The persistence seam for the workforce — mirrors the `WorkflowStore`
/// shape (goal-execution) so the two durable layers speak the same language:
/// save/load/list, never "how".
#[async_trait]
pub trait WorkforceStore: Send + Sync {
    async fn save_worker(&self, worker: &WorkerRecord) -> Result<(), String>;
    async fn load_worker(&self, id: &str) -> Result<Option<WorkerRecord>, String>;
    async fn list_workers(&self) -> Result<Vec<WorkerRecord>, String>;
    async fn save_task(&self, task: &BoardTask) -> Result<(), String>;
    async fn load_task(&self, id: &str) -> Result<Option<BoardTask>, String>;
    async fn list_tasks(&self) -> Result<Vec<BoardTask>, String>;
}

/// In-memory store — tests and short-lived boards.
#[derive(Default)]
pub struct InMemoryWorkforceStore {
    workers: std::sync::Mutex<HashMap<String, WorkerRecord>>,
    tasks: std::sync::Mutex<HashMap<String, BoardTask>>,
}

impl InMemoryWorkforceStore {
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl WorkforceStore for InMemoryWorkforceStore {
    async fn save_worker(&self, worker: &WorkerRecord) -> Result<(), String> {
        self.workers
            .lock()
            .map_err(|_| "worker store poisoned".to_string())?
            .insert(worker.id.clone(), worker.clone());
        Ok(())
    }

    async fn load_worker(&self, id: &str) -> Result<Option<WorkerRecord>, String> {
        Ok(self
            .workers
            .lock()
            .map_err(|_| "worker store poisoned".to_string())?
            .get(id)
            .cloned())
    }

    async fn list_workers(&self) -> Result<Vec<WorkerRecord>, String> {
        let mut v: Vec<_> = self
            .workers
            .lock()
            .map_err(|_| "worker store poisoned".to_string())?
            .values()
            .cloned()
            .collect();
        // (created_at, numeric suffix) — deterministic even when two records
        // share a millisecond.
        v.sort_by(|a, b| {
            a.created_at_ms
                .cmp(&b.created_at_ms)
                .then(Workforce::id_seq(&a.id).cmp(&Workforce::id_seq(&b.id)))
        });
        Ok(v)
    }

    async fn save_task(&self, task: &BoardTask) -> Result<(), String> {
        self.tasks
            .lock()
            .map_err(|_| "task store poisoned".to_string())?
            .insert(task.id.clone(), task.clone());
        Ok(())
    }

    async fn load_task(&self, id: &str) -> Result<Option<BoardTask>, String> {
        Ok(self
            .tasks
            .lock()
            .map_err(|_| "task store poisoned".to_string())?
            .get(id)
            .cloned())
    }

    async fn list_tasks(&self) -> Result<Vec<BoardTask>, String> {
        let mut v: Vec<_> = self
            .tasks
            .lock()
            .map_err(|_| "task store poisoned".to_string())?
            .values()
            .cloned()
            .collect();
        v.sort_by(|a, b| {
            a.created_at_ms
                .cmp(&b.created_at_ms)
                .then(Workforce::id_seq(&a.id).cmp(&Workforce::id_seq(&b.id)))
        });
        Ok(v)
    }
}

/// Json file store — one file per worker/task under `root/workers` and
/// `root/tasks`, written atomically (temp + rename) so a crash never leaves a
/// half-written record. This is what makes the workforce *persistent across
/// runs*: a fresh [`Workforce`] over the same root sees the same world.
pub struct JsonWorkforceStore {
    root: PathBuf,
}

impl JsonWorkforceStore {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    fn worker_path(&self, id: &str) -> PathBuf {
        self.root.join("workers").join(format!("{id}.json"))
    }

    fn task_path(&self, id: &str) -> PathBuf {
        self.root.join("tasks").join(format!("{id}.json"))
    }

    fn write_atomic(&self, path: &Path, bytes: &[u8]) -> Result<(), String> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, bytes).map_err(|e| e.to_string())?;
        std::fs::rename(&tmp, path).map_err(|e| e.to_string())?;
        Ok(())
    }
}

#[async_trait]
impl WorkforceStore for JsonWorkforceStore {
    async fn save_worker(&self, worker: &WorkerRecord) -> Result<(), String> {
        let json = serde_json::to_vec_pretty(worker).map_err(|e| e.to_string())?;
        self.write_atomic(&self.worker_path(&worker.id), &json)
    }

    async fn load_worker(&self, id: &str) -> Result<Option<WorkerRecord>, String> {
        let path = self.worker_path(id);
        if !path.exists() {
            return Ok(None);
        }
        let bytes = std::fs::read(&path).map_err(|e| e.to_string())?;
        serde_json::from_slice(&bytes)
            .map(Some)
            .map_err(|e| format!("corrupt worker record {id}: {e}"))
    }

    async fn list_workers(&self) -> Result<Vec<WorkerRecord>, String> {
        let dir = self.root.join("workers");
        let mut out = Vec::new();
        if let Ok(entries) = std::fs::read_dir(&dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|e| e.to_str()) == Some("json") {
                    if let Ok(bytes) = std::fs::read(&path) {
                        if let Ok(w) = serde_json::from_slice::<WorkerRecord>(&bytes) {
                            out.push(w);
                        }
                    }
                }
            }
        }
        out.sort_by(|a, b| {
            a.created_at_ms
                .cmp(&b.created_at_ms)
                .then(Workforce::id_seq(&a.id).cmp(&Workforce::id_seq(&b.id)))
        });
        Ok(out)
    }

    async fn save_task(&self, task: &BoardTask) -> Result<(), String> {
        let json = serde_json::to_vec_pretty(task).map_err(|e| e.to_string())?;
        self.write_atomic(&self.task_path(&task.id), &json)
    }

    async fn load_task(&self, id: &str) -> Result<Option<BoardTask>, String> {
        let path = self.task_path(id);
        if !path.exists() {
            return Ok(None);
        }
        let bytes = std::fs::read(&path).map_err(|e| e.to_string())?;
        serde_json::from_slice(&bytes)
            .map(Some)
            .map_err(|e| format!("corrupt task record {id}: {e}"))
    }

    async fn list_tasks(&self) -> Result<Vec<BoardTask>, String> {
        let dir = self.root.join("tasks");
        let mut out = Vec::new();
        if let Ok(entries) = std::fs::read_dir(&dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|e| e.to_str()) == Some("json") {
                    if let Ok(bytes) = std::fs::read(&path) {
                        if let Ok(t) = serde_json::from_slice::<BoardTask>(&bytes) {
                            out.push(t);
                        }
                    }
                }
            }
        }
        out.sort_by(|a, b| {
            a.created_at_ms
                .cmp(&b.created_at_ms)
                .then(Workforce::id_seq(&a.id).cmp(&Workforce::id_seq(&b.id)))
        });
        Ok(out)
    }
}

// ── Orchestrator ───────────────────────────────────────────────────────

/// The result of running a worker's task.
#[derive(Debug)]
pub enum TaskOutcome {
    /// The worker's run reached [`crate::AgentState::Succeeded`]; the task is
    /// `Done` and carries the persisted result.
    Done { task: BoardTask, result: AgentResult },
    /// The run ended in any non-success terminal state; the task is `Failed`
    /// and the honest result (state, error, steps) is persisted for audit.
    Failed { task: BoardTask, result: AgentResult },
}

/// The persistent workforce: registry + board + runtime, one owner.
pub struct Workforce {
    store: Arc<dyn WorkforceStore>,
    runtime: AgentRuntime,
    driver: Arc<dyn AgentDriver>,
    workspace_root: PathBuf,
    max_steps: u32,
}

impl Workforce {
    pub fn new(
        store: Arc<dyn WorkforceStore>,
        driver: Arc<dyn AgentDriver>,
        workspace_root: impl Into<PathBuf>,
    ) -> Self {
        Self::with_config(store, driver, workspace_root, 16)
    }

    /// `max_steps` bounds every worker's run (the runtime pool cap and
    /// deadline stay at the runtime defaults; use
    /// [`Workforce::with_runtime`] for full control).
    pub fn with_config(
        store: Arc<dyn WorkforceStore>,
        driver: Arc<dyn AgentDriver>,
        workspace_root: impl Into<PathBuf>,
        max_steps: u32,
    ) -> Self {
        Self {
            store,
            runtime: AgentRuntime::new(),
            driver,
            workspace_root: workspace_root.into(),
            max_steps,
        }
    }

    /// Full runtime control (pool cap / deadline) for the workforce's runs.
    pub fn with_runtime(mut self, runtime: AgentRuntime) -> Self {
        self.runtime = runtime;
        self
    }

    pub fn store(&self) -> &Arc<dyn WorkforceStore> {
        &self.store
    }

    /// The root under which every worker's isolated workspace lives.
    pub fn workspace_root(&self) -> &Path {
        &self.workspace_root
    }

    fn now_ms() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0)
    }

    /// Ids are derived from the store, not a per-instance counter, so two
    /// instances over the same store can never collide (`researcher.1` in
    /// instance A and `researcher.1` in instance B would be the same worker).
    /// The numeric suffix is the count of same-role records plus one.
    async fn next_worker_id(&self, role: AgentRole) -> String {
        let existing = self.store.list_workers().await.unwrap_or_default();
        let n = existing.iter().filter(|w| w.role == role).count() + 1;
        format!("{}.{}", role.as_str(), n)
    }

    async fn next_task_id(&self) -> String {
        let existing = self.store.list_tasks().await.unwrap_or_default();
        format!("task.{}", existing.len() + 1)
    }

    /// The numeric suffix of an id (`task.12` → 12) — the deterministic FIFO
    /// tie-break when two records share a `created_at_ms`.
    fn id_seq(id: &str) -> u64 {
        id.rsplit('.')
            .next()
            .and_then(|s| s.parse().ok())
            .unwrap_or(0)
    }

    // ── Registry ───────────────────────────────────────────────────────

    /// **register** — create a persistent worker with an isolated workspace
    /// directory. The record (and the workspace) survives across runs: a
    /// fresh [`Workforce`] over the same store sees it.
    pub async fn register(
        &self,
        role: AgentRole,
        description: &str,
    ) -> Result<WorkerRecord, String> {
        let id = self.next_worker_id(role).await;
        let now = Self::now_ms();
        let workspace = self.workspace_root.join(&id);
        std::fs::create_dir_all(&workspace).map_err(|e| {
            format!("cannot create workspace for {id}: {e}")
        })?;
        let worker = WorkerRecord {
            id,
            role,
            description: description.to_string(),
            status: WorkerStatus::Idle,
            current_task: None,
            workspace,
            created_at_ms: now,
            updated_at_ms: now,
        };
        self.store.save_worker(&worker).await?;
        Ok(worker)
    }

    /// **create_task** — put an Open task on the assignable board.
    pub async fn create_task(
        &self,
        title: &str,
        goal: &str,
        required_role: AgentRole,
    ) -> Result<BoardTask, String> {
        let now = Self::now_ms();
        let task = BoardTask {
            id: self.next_task_id().await,
            title: title.to_string(),
            goal: goal.to_string(),
            required_role,
            status: TaskStatus::Open,
            assignee: None,
            params: serde_json::Map::new(),
            progress: Vec::new(),
            blockers: Vec::new(),
            result: None,
            created_at_ms: now,
            updated_at_ms: now,
        };
        self.store.save_task(&task).await?;
        Ok(task)
    }

    // ── Board ──────────────────────────────────────────────────────────

    /// **assign** — bind an Open task to an idle worker. Refuses a role
    /// mismatch, a busy worker, or a task that is not Open.
    pub async fn assign(&self, worker_id: &str, task_id: &str) -> Result<(), String> {
        let mut worker = self
            .store
            .load_worker(worker_id)
            .await?
            .ok_or_else(|| format!("no such worker: {worker_id}"))?;
        let mut task = self
            .store
            .load_task(task_id)
            .await?
            .ok_or_else(|| format!("no such task: {task_id}"))?;

        if worker.status != WorkerStatus::Idle {
            return Err(format!(
                "worker {worker_id} is {}, not idle",
                worker.status.as_str()
            ));
        }
        if task.status != TaskStatus::Open {
            return Err(format!(
                "task {task_id} is {}, not open",
                task.status.as_str()
            ));
        }
        if worker.role != task.required_role {
            return Err(format!(
                "task {task_id} requires a {} worker, but {worker_id} is a {}",
                task.required_role.as_str(),
                worker.role.as_str()
            ));
        }

        worker.status = WorkerStatus::Assigned;
        worker.current_task = Some(task.id.clone());
        worker.updated_at_ms = Self::now_ms();
        task.status = TaskStatus::Assigned;
        task.assignee = Some(worker.id.clone());
        task.updated_at_ms = Self::now_ms();

        self.store.save_worker(&worker).await?;
        self.store.save_task(&task).await?;
        Ok(())
    }

    /// **pick_up** — an idle worker takes the oldest Open task matching its
    /// role (FIFO across the board). Returns the assigned task, or `None` if
    /// nothing on the board fits.
    pub async fn pick_up(&self, worker_id: &str) -> Result<Option<BoardTask>, String> {
        let worker = self
            .store
            .load_worker(worker_id)
            .await?
            .ok_or_else(|| format!("no such worker: {worker_id}"))?;
        if worker.status != WorkerStatus::Idle {
            return Err(format!(
                "worker {worker_id} is {}, not idle",
                worker.status.as_str()
            ));
        }
        let candidate = self
            .store
            .list_tasks()
            .await?
            .into_iter()
            .find(|t| t.status == TaskStatus::Open && t.required_role == worker.role);
        let Some(task) = candidate else {
            return Ok(None);
        };
        self.assign(worker_id, &task.id).await?;
        // Return the post-assignment record (Assigned + bound worker), not the
        // pre-assignment snapshot the caller would otherwise see.
        self.store.load_task(&task.id).await
    }

    /// **run** — execute the worker's assigned task through the runtime with
    /// the driver, in the worker's isolated workspace (`spec.cwd`), with the
    /// task's capability params. Only `Succeeded` yields `Done`; every other
    /// terminal state yields `Failed` with the honest result persisted.
    pub async fn run(&self, worker_id: &str) -> Result<TaskOutcome, String> {
        let mut worker = self
            .store
            .load_worker(worker_id)
            .await?
            .ok_or_else(|| format!("no such worker: {worker_id}"))?;
        if worker.status != WorkerStatus::Assigned {
            return Err(format!(
                "worker {worker_id} is {}, not assigned",
                worker.status.as_str()
            ));
        }
        let task_id = worker
            .current_task
            .clone()
            .ok_or_else(|| format!("worker {worker_id} has no task assigned"))?;
        let mut task = self
            .store
            .load_task(&task_id)
            .await?
            .ok_or_else(|| format!("assigned task {task_id} is gone"))?;
        if task.status != TaskStatus::Assigned || task.assignee.as_deref() != Some(worker_id) {
            return Err(format!(
                "task {task_id} is {}, not assigned to {worker_id}",
                task.status.as_str()
            ));
        }

        // Mark both sides Running/InProgress before dispatch.
        worker.status = WorkerStatus::Running;
        worker.updated_at_ms = Self::now_ms();
        task.status = TaskStatus::InProgress;
        task.updated_at_ms = Self::now_ms();
        self.store.save_worker(&worker).await?;
        self.store.save_task(&task).await?;

        let mut spec = AgentSpec::new(worker.id.clone(), &task.goal)
            .with_allowed_skills(worker.role.default_skill_scope())
            .with_max_steps(self.max_steps)
            .with_cwd(worker.workspace.clone());
        for (capability, params) in &task.params {
            spec = spec.with_param(capability, params.clone());
        }

        let handle = self.runtime.spawn(spec, Arc::clone(&self.driver));
        let result = handle.await_result().await;

        task.result = Some(result.clone());
        if result.state.is_success() {
            task.status = TaskStatus::Done;
        } else {
            task.status = TaskStatus::Failed;
        }
        task.updated_at_ms = Self::now_ms();
        self.store.save_task(&task).await?;
        // The returned record is the *persisted* one — same timestamps, same
        // status, same result.
        let outcome = if task.status == TaskStatus::Done {
            TaskOutcome::Done {
                task: task.clone(),
                result,
            }
        } else {
            TaskOutcome::Failed {
                task: task.clone(),
                result,
            }
        };

        worker.status = WorkerStatus::Idle;
        worker.current_task = None;
        worker.updated_at_ms = Self::now_ms();
        self.store.save_worker(&worker).await?;

        Ok(outcome)
    }

    // ── Progress / blockers ────────────────────────────────────────────

    /// **report_progress** — append a progress note to the worker's current
    /// task. The note is persisted immediately; nothing is synthesized.
    pub async fn report_progress(
        &self,
        worker_id: &str,
        note: &str,
    ) -> Result<BoardTask, String> {
        let worker = self
            .store
            .load_worker(worker_id)
            .await?
            .ok_or_else(|| format!("no such worker: {worker_id}"))?;
        let task_id = worker
            .current_task
            .as_ref()
            .ok_or_else(|| format!("worker {worker_id} has no current task"))?;
        let mut task = self
            .store
            .load_task(task_id)
            .await?
            .ok_or_else(|| format!("task {task_id} is gone"))?;
        task.progress.push(BoardNote {
            at_ms: Self::now_ms(),
            note: note.to_string(),
        });
        task.updated_at_ms = Self::now_ms();
        self.store.save_task(&task).await?;
        Ok(task)
    }

    /// **report_blocker** — record a blocker on the worker's current task and
    /// flip it to `Blocked` (resolvable via [`Workforce::resolve_blocker`]).
    pub async fn report_blocker(
        &self,
        worker_id: &str,
        blocker: &str,
    ) -> Result<BoardTask, String> {
        let worker = self
            .store
            .load_worker(worker_id)
            .await?
            .ok_or_else(|| format!("no such worker: {worker_id}"))?;
        let task_id = worker
            .current_task
            .as_ref()
            .ok_or_else(|| format!("worker {worker_id} has no current task"))?;
        let mut task = self
            .store
            .load_task(task_id)
            .await?
            .ok_or_else(|| format!("task {task_id} is gone"))?;
        task.blockers.push(BoardNote {
            at_ms: Self::now_ms(),
            note: blocker.to_string(),
        });
        task.status = TaskStatus::Blocked;
        task.updated_at_ms = Self::now_ms();
        self.store.save_task(&task).await?;
        Ok(task)
    }

    /// **resolve_blocker** — a blocked task is assignable again. The blocker
    /// notes stay (honest history); only the status flips back to `Assigned`.
    pub async fn resolve_blocker(&self, task_id: &str) -> Result<BoardTask, String> {
        let mut task = self
            .store
            .load_task(task_id)
            .await?
            .ok_or_else(|| format!("no such task: {task_id}"))?;
        if task.status != TaskStatus::Blocked {
            return Err(format!(
                "task {task_id} is {}, not blocked",
                task.status.as_str()
            ));
        }
        task.status = TaskStatus::Assigned;
        task.updated_at_ms = Self::now_ms();
        self.store.save_task(&task).await?;
        Ok(task)
    }

    // ── Suspend / resume / retire ──────────────────────────────────────

    /// **suspend** — pause an assigned worker. It keeps its assigned task
    /// (the board entry is untouched) but cannot run until resumed. A running
    /// worker cannot be suspended mid-flight here — in-flight cancellation
    /// stays with the runtime (the run's own deadline/handle).
    pub async fn suspend(&self, worker_id: &str) -> Result<WorkerRecord, String> {
        let mut worker = self
            .store
            .load_worker(worker_id)
            .await?
            .ok_or_else(|| format!("no such worker: {worker_id}"))?;
        if worker.status != WorkerStatus::Assigned {
            return Err(format!(
                "worker {worker_id} is {}, not assigned",
                worker.status.as_str()
            ));
        }
        worker.status = WorkerStatus::Suspended;
        worker.updated_at_ms = Self::now_ms();
        self.store.save_worker(&worker).await?;
        Ok(worker)
    }

    /// **resume** — a suspended worker returns to `Assigned` and can run its
    /// kept task.
    pub async fn resume(&self, worker_id: &str) -> Result<WorkerRecord, String> {
        let mut worker = self
            .store
            .load_worker(worker_id)
            .await?
            .ok_or_else(|| format!("no such worker: {worker_id}"))?;
        if worker.status != WorkerStatus::Suspended {
            return Err(format!(
                "worker {worker_id} is {}, not suspended",
                worker.status.as_str()
            ));
        }
        worker.status = WorkerStatus::Assigned;
        worker.updated_at_ms = Self::now_ms();
        self.store.save_worker(&worker).await?;
        Ok(worker)
    }

    /// **retire** — permanently remove a worker from the rotation. Refuses a
    /// worker that is `Running` (a mid-flight run would otherwise flip it back
    /// to `Idle` after retirement). Its current task (if any) is released back
    /// to `Open` on the board.
    pub async fn retire(&self, worker_id: &str) -> Result<WorkerRecord, String> {
        let mut worker = self
            .store
            .load_worker(worker_id)
            .await?
            .ok_or_else(|| format!("no such worker: {worker_id}"))?;
        if worker.status == WorkerStatus::Retired {
            return Err(format!("worker {worker_id} is already retired"));
        }
        if worker.status == WorkerStatus::Running {
            return Err(format!(
                "worker {worker_id} is running; suspend or await the run first"
            ));
        }
        if let Some(task_id) = worker.current_task.take() {
            if let Some(mut task) = self.store.load_task(&task_id).await? {
                if task.status == TaskStatus::Assigned || task.status == TaskStatus::Blocked {
                    task.status = TaskStatus::Open;
                    task.assignee = None;
                    task.updated_at_ms = Self::now_ms();
                    self.store.save_task(&task).await?;
                }
            }
        }
        worker.status = WorkerStatus::Retired;
        worker.updated_at_ms = Self::now_ms();
        self.store.save_worker(&worker).await?;
        Ok(worker)
    }

    // ── Views ──────────────────────────────────────────────────────────

    pub async fn worker(&self, id: &str) -> Result<Option<WorkerRecord>, String> {
        self.store.load_worker(id).await
    }

    pub async fn task(&self, id: &str) -> Result<Option<BoardTask>, String> {
        self.store.load_task(id).await
    }

    pub async fn workers(&self) -> Result<Vec<WorkerRecord>, String> {
        self.store.list_workers().await
    }

    pub async fn tasks(&self) -> Result<Vec<BoardTask>, String> {
        self.store.list_tasks().await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::AgentContext;
    use crate::driver::FnDriver;

    fn ok_driver() -> Arc<dyn AgentDriver> {
        Arc::new(FnDriver::new(|ctx: AgentContext| {
            AgentResult::succeeded(ctx.spec.id, "did the task", vec![])
        }))
    }

    fn failing_driver() -> Arc<dyn AgentDriver> {
        Arc::new(FnDriver::new(|ctx: AgentContext| {
            AgentResult::failed(ctx.spec.id, "could not", vec![], "boom")
        }))
    }

    /// A driver that writes a marker file into its workspace (proving the
    /// worker genuinely operates in `spec.cwd`) and succeeds.
    fn workspace_writing_driver() -> Arc<dyn AgentDriver> {
        Arc::new(FnDriver::new(|ctx: AgentContext| {
            let cwd = ctx.spec.cwd.clone().expect("cwd must be the workspace");
            std::fs::write(cwd.join("worker-marker.txt"), "i was here").unwrap();
            AgentResult::succeeded(ctx.spec.id, "wrote marker", vec![])
        }))
    }

    fn in_memory() -> Arc<InMemoryWorkforceStore> {
        Arc::new(InMemoryWorkforceStore::new())
    }

    fn workspace_root(name: &str) -> PathBuf {
        let mut p = std::env::temp_dir();
        p.push(format!("friday-workforce-{name}"));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    #[tokio::test]
    async fn register_creates_a_persistent_worker_with_an_isolated_workspace() {
        let store = in_memory();
        let root = workspace_root("register");
        let wf = Workforce::new(store.clone(), ok_driver(), &root);

        let worker = wf
            .register(AgentRole::Researcher, "reads the docs")
            .await
            .unwrap();

        assert!(worker.id.starts_with("researcher."));
        assert_eq!(worker.status, WorkerStatus::Idle);
        assert!(worker.workspace.starts_with(&root));
        assert!(worker.workspace.exists(), "workspace dir must exist on disk");

        let loaded = wf.worker(&worker.id).await.unwrap().unwrap();
        assert_eq!(loaded.id, worker.id);
        assert_eq!(loaded.role, AgentRole::Researcher);
    }

    #[tokio::test]
    async fn assignment_refuses_role_mismatch_busy_worker_and_non_open_task() {
        let store = in_memory();
        let root = workspace_root("assign");
        let wf = Workforce::new(store.clone(), ok_driver(), &root);

        let researcher = wf
            .register(AgentRole::Researcher, "reads")
            .await
            .unwrap();
        let verifier = wf.register(AgentRole::Verifier, "proves").await.unwrap();
        let docs_task = wf
            .create_task("read docs", "read the docs", AgentRole::Researcher)
            .await
            .unwrap();
        let git_task = wf
            .create_task("check git", "check git status", AgentRole::Verifier)
            .await
            .unwrap();

        // Role mismatch refused.
        let err = wf.assign(&researcher.id, &git_task.id).await.unwrap_err();
        assert!(err.contains("requires a verifier worker"), "{err}");

        // Happy path.
        wf.assign(&researcher.id, &docs_task.id).await.unwrap();

        // Busy worker refused.
        let another = wf
            .create_task("more docs", "read more", AgentRole::Researcher)
            .await
            .unwrap();
        let err = wf.assign(&researcher.id, &another.id).await.unwrap_err();
        assert!(err.contains("not idle"), "{err}");

        // Non-open task refused.
        let err = wf.assign(&verifier.id, &docs_task.id).await.unwrap_err();
        assert!(err.contains("not open"), "{err}");
    }

    #[tokio::test]
    async fn pick_up_takes_the_oldest_open_task_matching_the_role() {
        let store = in_memory();
        let root = workspace_root("pickup");
        let wf = Workforce::new(store.clone(), ok_driver(), &root);

        wf.create_task("old", "old goal", AgentRole::Researcher)
            .await
            .unwrap();
        wf.create_task("new", "new goal", AgentRole::Researcher)
            .await
            .unwrap();
        let worker = wf
            .register(AgentRole::Researcher, "reads")
            .await
            .unwrap();

        let picked = wf.pick_up(&worker.id).await.unwrap().unwrap();
        assert_eq!(picked.title, "old", "FIFO: the oldest matching task");
        assert_eq!(picked.status, TaskStatus::Assigned, "returns the post-assignment record");
        assert_eq!(picked.assignee.as_deref(), Some(worker.id.as_str()));

        let worker = wf.worker(&worker.id).await.unwrap().unwrap();
        assert_eq!(worker.status, WorkerStatus::Assigned);
        assert_eq!(worker.current_task.as_deref(), Some(picked.id.as_str()));

        // A second pick_up finds nothing (the remaining task is still Open,
        // but this worker is now busy).
        let err = wf.pick_up(&worker.id).await.unwrap_err();
        assert!(err.contains("not idle"), "{err}");
    }

    #[tokio::test]
    async fn run_completes_the_task_in_the_worker_workspace_and_persists_the_result() {
        let store = in_memory();
        let root = workspace_root("run");
        let wf = Workforce::new(store.clone(), workspace_writing_driver(), &root);

        let worker = wf
            .register(AgentRole::Researcher, "reads")
            .await
            .unwrap();
        let task = wf
            .create_task("read docs", "read the docs", AgentRole::Researcher)
            .await
            .unwrap();
        wf.assign(&worker.id, &task.id).await.unwrap();

        let outcome = wf.run(&worker.id).await.unwrap();
        let TaskOutcome::Done { task, result } = outcome else {
            panic!("expected Done");
        };
        assert_eq!(task.status, TaskStatus::Done);
        assert_eq!(result.state.as_str(), "succeeded");
        assert!(
            task.result.is_some(),
            "the result must be persisted on the task"
        );

        // The worker's workspace persisted the marker the driver wrote.
        let worker = wf.worker(&worker.id).await.unwrap().unwrap();
        assert_eq!(worker.status, WorkerStatus::Idle);
        assert!(worker.current_task.is_none());
        assert!(
            worker.workspace.join("worker-marker.txt").exists(),
            "the worker must have operated in its own workspace"
        );

        // A fresh view over the same store sees the done task.
        let task = wf.task(&task.id).await.unwrap().unwrap();
        assert_eq!(task.status, TaskStatus::Done);
    }

    #[tokio::test]
    async fn a_failing_run_marks_the_task_failed_and_never_done() {
        let store = in_memory();
        let root = workspace_root("fail");
        let wf = Workforce::new(store.clone(), failing_driver(), &root);

        let worker = wf
            .register(AgentRole::Analyst, "analyzes")
            .await
            .unwrap();
        let task = wf
            .create_task("analyze", "analyze the data", AgentRole::Analyst)
            .await
            .unwrap();
        wf.assign(&worker.id, &task.id).await.unwrap();

        let outcome = wf.run(&worker.id).await.unwrap();
        let TaskOutcome::Failed { task, result } = outcome else {
            panic!("expected Failed");
        };
        assert_eq!(task.status, TaskStatus::Failed);
        assert_eq!(result.state.as_str(), "failed");
        assert_eq!(result.error.as_deref(), Some("boom"));
        assert!(
            task.result.as_ref().unwrap().error.as_deref() == Some("boom"),
            "the honest failure must be persisted"
        );

        let worker = wf.worker(&worker.id).await.unwrap().unwrap();
        assert_eq!(worker.status, WorkerStatus::Idle);
    }

    #[tokio::test]
    async fn run_refuses_unassigned_workers_and_never_re_runs_done_work() {
        let store = in_memory();
        let root = workspace_root("refuse");
        let wf = Workforce::new(store.clone(), ok_driver(), &root);

        let worker = wf
            .register(AgentRole::Researcher, "reads")
            .await
            .unwrap();

        // No task assigned.
        let err = wf.run(&worker.id).await.unwrap_err();
        assert!(err.contains("not assigned"), "{err}");

        let task = wf
            .create_task("read docs", "read", AgentRole::Researcher)
            .await
            .unwrap();
        wf.assign(&worker.id, &task.id).await.unwrap();
        wf.run(&worker.id).await.unwrap();

        // After Done, the worker is Idle with no task — the done task is
        // terminal and a second run is refused.
        let err = wf.run(&worker.id).await.unwrap_err();
        assert!(err.contains("not assigned"), "{err}");
        let done = wf.task(&task.id).await.unwrap().unwrap();
        assert_eq!(done.status, TaskStatus::Done);
    }

    #[tokio::test]
    async fn progress_and_blockers_are_recorded_and_persisted() {
        let store = in_memory();
        let root = workspace_root("progress");
        let wf = Workforce::new(store.clone(), ok_driver(), &root);

        let worker = wf
            .register(AgentRole::Researcher, "reads")
            .await
            .unwrap();
        let task = wf
            .create_task("read docs", "read", AgentRole::Researcher)
            .await
            .unwrap();
        wf.assign(&worker.id, &task.id).await.unwrap();

        let t = wf.report_progress(&worker.id, "found the index").await.unwrap();
        assert_eq!(t.progress.len(), 1);
        assert_eq!(t.progress[0].note, "found the index");

        let t = wf
            .report_blocker(&worker.id, "docs server unreachable")
            .await
            .unwrap();
        assert_eq!(t.blockers.len(), 1);
        assert_eq!(t.status, TaskStatus::Blocked);

        // The blocker keeps the notes; resolving only flips the status back.
        let t = wf.resolve_blocker(&task.id).await.unwrap();
        assert_eq!(t.status, TaskStatus::Assigned);
        assert_eq!(t.blockers.len(), 1);

        // A fresh view over the same store sees everything.
        let persisted = wf.task(&task.id).await.unwrap().unwrap();
        assert_eq!(persisted.progress.len(), 1);
        assert_eq!(persisted.blockers.len(), 1);
    }

    #[tokio::test]
    async fn suspend_resume_pauses_an_assigned_worker_without_losing_its_task() {
        let store = in_memory();
        let root = workspace_root("suspend");
        let wf = Workforce::new(store.clone(), ok_driver(), &root);

        let worker = wf
            .register(AgentRole::Researcher, "reads")
            .await
            .unwrap();
        let task = wf
            .create_task("read docs", "read", AgentRole::Researcher)
            .await
            .unwrap();
        wf.assign(&worker.id, &task.id).await.unwrap();

        let suspended = wf.suspend(&worker.id).await.unwrap();
        assert_eq!(suspended.status, WorkerStatus::Suspended);
        assert_eq!(
            suspended.current_task.as_deref(),
            Some(task.id.as_str()),
            "the assigned task survives suspension"
        );

        // Suspended worker cannot run.
        let err = wf.run(&worker.id).await.unwrap_err();
        assert!(err.contains("not assigned"), "{err}");

        let resumed = wf.resume(&worker.id).await.unwrap();
        assert_eq!(resumed.status, WorkerStatus::Assigned);

        let outcome = wf.run(&worker.id).await.unwrap();
        assert!(matches!(outcome, TaskOutcome::Done { .. }));
    }

    #[tokio::test]
    async fn retire_refuses_a_running_worker() {
        let store = in_memory();
        let root = workspace_root("retire-running");
        let wf = Workforce::new(store.clone(), ok_driver(), &root);

        let worker = wf
            .register(AgentRole::Researcher, "reads")
            .await
            .unwrap();
        let task = wf
            .create_task("read docs", "read", AgentRole::Researcher)
            .await
            .unwrap();
        wf.assign(&worker.id, &task.id).await.unwrap();

        // Simulate an in-flight run by flipping the worker to Running.
        let mut worker = wf.worker(&worker.id).await.unwrap().unwrap();
        worker.status = WorkerStatus::Running;
        store.save_worker(&worker).await.unwrap();

        let err = wf.retire(&worker.id).await.unwrap_err();
        assert!(err.contains("running"), "{err}");

        // Retirement still works once the worker is back to Idle.
        let mut worker = wf.worker(&worker.id).await.unwrap().unwrap();
        worker.status = WorkerStatus::Idle;
        store.save_worker(&worker).await.unwrap();
        let retired = wf.retire(&worker.id).await.unwrap();
        assert_eq!(retired.status, WorkerStatus::Retired);
    }

    #[tokio::test]
    async fn retire_releases_its_task_back_to_open_and_is_terminal() {
        let store = in_memory();
        let root = workspace_root("retire");
        let wf = Workforce::new(store.clone(), ok_driver(), &root);

        let worker = wf
            .register(AgentRole::Researcher, "reads")
            .await
            .unwrap();
        let task = wf
            .create_task("read docs", "read", AgentRole::Researcher)
            .await
            .unwrap();
        wf.assign(&worker.id, &task.id).await.unwrap();

        let retired = wf.retire(&worker.id).await.unwrap();
        assert_eq!(retired.status, WorkerStatus::Retired);

        let task = wf.task(&task.id).await.unwrap().unwrap();
        assert_eq!(task.status, TaskStatus::Open);
        assert!(task.assignee.is_none());

        let err = wf.retire(&worker.id).await.unwrap_err();
        assert!(err.contains("already retired"), "{err}");
    }

    /// THE constitution acceptance: a worker registered, assigned, and run
    /// through one instance is seen, picked up, and completed by a fresh
    /// instance over the same store — and its workspace persists on disk.
    #[tokio::test]
    async fn worker_and_task_and_workspace_persist_across_instances() {
        let root = workspace_root("across-instances");
        let store_root = root.join("store");
        let workspaces = root.join("workspaces");

        let shared: Arc<dyn WorkforceStore> = Arc::new(JsonWorkforceStore::new(&store_root));
        let driver = workspace_writing_driver();

        // Instance A: register + create + assign (never runs).
        {
            let wf = Workforce::new(Arc::clone(&shared), Arc::clone(&driver), &workspaces);
            let worker = wf
                .register(AgentRole::Researcher, "reads docs")
                .await
                .unwrap();
            let task = wf
                .create_task("read docs", "read the docs", AgentRole::Researcher)
                .await
                .unwrap();
            wf.assign(&worker.id, &task.id).await.unwrap();
        }

        // Instance B (fresh Workforce over the same store): the world is
        // still there, and the assigned task runs.
        let task_id = {
            let wf = Workforce::new(shared.clone(), Arc::clone(&driver), &workspaces);
            let workers = wf.workers().await.unwrap();
            assert_eq!(workers.len(), 1);
            let worker = &workers[0];
            assert_eq!(worker.status, WorkerStatus::Assigned);
            assert!(
                worker.workspace.exists(),
                "the workspace directory persists on disk"
            );

            let outcome = wf.run(&worker.id).await.unwrap();
            let TaskOutcome::Done { task, .. } = outcome else {
                panic!("expected Done");
            };
            task.id
        };

        // Instance C: the done task + persisted result are visible, and the
        // marker the worker's actions wrote into its workspace is still there.
        {
            let wf = Workforce::new(shared, Arc::clone(&driver), &workspaces);
            let task = wf.task(&task_id).await.unwrap().unwrap();
            assert_eq!(task.status, TaskStatus::Done);
            assert!(task.result.is_some());

            let workers = wf.workers().await.unwrap();
            assert_eq!(workers[0].status, WorkerStatus::Idle);
            assert!(workers[0].current_task.is_none());
            assert!(
                workers[0].workspace.join("worker-marker.txt").exists(),
                "the workspace content survives across instances"
            );
        }
    }
}
