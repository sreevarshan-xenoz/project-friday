//! # FRIDAY Planner
//!
//! The "brain" of FRIDAY, split into four independent engines that communicate
//! only through the event bus (see [`events`] for the vocabulary):
//!
//! * **Intent Engine** — understands the request (`goal.raw` → `intent.created`).
//! * **Planning Engine** — decomposes intent into a task graph
//!   (`intent.created` → `plan.created`).
//! * **Execution Engine** — routes tasks to plugins and tracks lifecycle
//!   (`plan.created` → `task.started` / `exec.action` → `task.completed`).
//! * **Verification Engine** — confirms success and drives retry/replan
//!   (`task.completed`/`task.failed` → `verification.*`).
//!
//! Plans are modelled as a DAG ([`planning::TaskGraph`]) so independent tasks
//! can later run in parallel, and every task has an explicit
//! [`planning::TaskState`] lifecycle for debuggability.

pub mod engines;
pub mod events;
pub mod execution;
pub mod intent;
pub mod planning;
pub mod verification;

pub use engines::{IntentEngine, PlanningEngine};
pub use events::{
    EXEC_ACTION, EXEC_RESULT, GOAL_RAW, INTENT_CREATED, PLAN_CREATED, TASK_COMPLETED, TASK_FAILED,
    TASK_RETRY, TASK_STARTED, VERIFICATION_COMPLETED, VERIFICATION_FAILED, VERIFICATION_STARTED,
};
pub use execution::ExecutionEngine;
pub use intent::{parse, Intent, IntentType};
pub use planning::{levels, plan, Task, TaskGraph, TaskState};
pub use verification::{RetryPolicy, Verdict, VerificationEngine, VerificationResult};

use friday_core::Friday;
use friday_core::Result;

/// Register all four planner engines with a runtime. Order does not matter;
/// they are wired together purely by the events they emit and consume.
pub async fn register_engines(friday: &mut Friday) -> Result<()> {
    friday
        .register_plugin(Box::new(IntentEngine::new()))
        .await?;
    friday
        .register_plugin(Box::new(PlanningEngine::new()))
        .await?;
    friday
        .register_plugin(Box::new(ExecutionEngine::new()))
        .await?;
    friday
        .register_plugin(Box::new(VerificationEngine::new()))
        .await?;
    Ok(())
}
