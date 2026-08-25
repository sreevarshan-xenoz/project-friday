//! [`WorkforceDriver`] — the delegation seam between the [`AgentDirector`]
//! topology and the persistent [`Workforce`] board (R2-3 enabler).
//!
//! Before this adapter the two were siblings: the Director spawned ephemeral
//! runtime children while the Workforce owned durable board tasks, and no
//! production path let a Director delegate work *through* the board. This
//! driver is pure contract translation — it creates **no** scheduler, task
//! board, worker abstraction, persistence layer, or verification system:
//!
//! ```text
//! AgentDirector ── spawn(role, goal) ──▶ AgentRuntime
//!     │                                     │ driver.run(ctx)
//!     ▼                                     ▼
//! WorkforceDriver ── create_task + pick_up/assign + run ──▶ Workforce board
//!     │
//!     ├─ Done    { task, result } ──▶ AgentResult (Succeeded preserved)
//!     └─ Failed  { task, result } ──▶ AgentResult (honest failure preserved)
//! ```
//!
//! Honesty rule: the board's persisted `AgentResult` is forwarded verbatim —
//! a worker that failed is reported as failed to the Director's
//! `ResultGate`, never laundered into success. The only edit is annotating
//! the summary with the durable board identity so evidence chains can point
//! at the persistent task.
//!
//! Liveness note: the delegated run is bounded by the Workforce's own
//! runtime deadline (its pre-dispatch checkpoint records it); the parent
//! context's cancellation is not forwarded into the board — the run is
//! durable by design and outlives the delegating call's budget surface.

use std::sync::Arc;

use async_trait::async_trait;

use crate::context::AgentContext;
use crate::director::AgentRole;
use crate::driver::AgentDriver;
use crate::spec::{AgentResult, AgentStep};
use crate::workforce::{TaskOutcome, WorkerStatus, Workforce};

/// A driver whose every run becomes a durable Workforce board task executed
/// by a persistent worker.
pub struct WorkforceDriver {
    workforce: Arc<Workforce>,
    role: AgentRole,
}

impl WorkforceDriver {
    /// Delegate every run to `workforce` as a task requiring `role`.
    pub fn new(workforce: Arc<Workforce>, role: AgentRole) -> Self {
        Self { workforce, role }
    }

    /// The workforce this driver delegates to (for seam assertions).
    pub fn workforce(&self) -> &Arc<Workforce> {
        &self.workforce
    }

    /// Find an `Idle` worker of the role, or register a fresh persistent one.
    async fn acquire_worker(&self) -> Result<String, String> {
        for worker in self.workforce.workers().await? {
            if worker.role == self.role && worker.status == WorkerStatus::Idle {
                return Ok(worker.id);
            }
        }
        let worker = self
            .workforce
            .register(self.role, "delegated director child")
            .await?;
        Ok(worker.id)
    }
}

#[async_trait]
impl AgentDriver for WorkforceDriver {
    async fn run(&self, ctx: AgentContext) -> AgentResult {
        let agent_id = ctx.spec.id.clone();
        let goal = ctx.spec.goal.clone();

        let task = match self.workforce.create_task(&goal, &goal, self.role).await {
            Ok(t) => t,
            Err(e) => {
                return AgentResult::failed(
                    agent_id,
                    "workforce delegation failed at create_task",
                    vec![],
                    e,
                )
            }
        };

        let worker_id = match self.acquire_worker().await {
            Ok(id) => id,
            Err(e) => {
                return AgentResult::failed(
                    agent_id,
                    "workforce delegation failed at worker acquisition",
                    vec![],
                    e,
                )
            }
        };
        if let Err(e) = self.workforce.assign(&worker_id, &task.id).await {
            return AgentResult::failed(
                agent_id,
                "workforce delegation failed at assignment",
                vec![],
                e,
            );
        }

        // Honor cooperative cancellation before the durable dispatch; once
        // the checkpoint is written the run is durable by design.
        if ctx.check().is_err() {
            return AgentResult::failed(
                agent_id,
                format!("workforce task {} cancelled before dispatch", task.id),
                vec![],
                "cancelled",
            );
        }

        let outcome = self.workforce.run(&worker_id).await;
        match outcome {
            Ok(TaskOutcome::Done { task, mut result }) => {
                result.summary = format!(
                    "{} [workforce task {} done by {}]",
                    result.summary, task.id, worker_id
                );
                result
            }
            Ok(TaskOutcome::Failed { task, mut result }) => {
                result.summary = format!(
                    "{} [workforce task {} failed by {}]",
                    result.summary, task.id, worker_id
                );
                result
            }
            Err(e) => AgentResult::failed(
                agent_id,
                format!("workforce task {} errored", task.id),
                Vec::<AgentStep>::new(),
                e,
            ),
        }
    }
}
