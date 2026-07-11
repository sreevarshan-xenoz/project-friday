//! Standard event vocabulary for the planner pipeline.
//!
//! Every stage of the brain communicates exclusively through these events.
//! The six user-facing lifecycle events (`intent.created`, `plan.created`,
//! `task.started`, `task.completed`, `task.failed`, `verification.*`) are the
//! contract; the control events (`exec.action`, `exec.result`, `task.retry`)
//! are internal plumbing between the execution and verification engines.

use friday_core::event::Event;
use serde_json::Value;
use uuid::Uuid;

pub const GOAL_RAW: &str = "goal.raw";
pub const INTENT_CREATED: &str = "intent.created";
pub const PLAN_CREATED: &str = "plan.created";
pub const TASK_STARTED: &str = "task.started";
pub const TASK_COMPLETED: &str = "task.completed";
pub const TASK_FAILED: &str = "task.failed";
pub const TASK_RETRY: &str = "task.retry";
pub const EXEC_ACTION: &str = "exec.action";
pub const EXEC_RESULT: &str = "exec.result";
pub const VERIFICATION_STARTED: &str = "verification.started";
pub const VERIFICATION_COMPLETED: &str = "verification.completed";
pub const VERIFICATION_FAILED: &str = "verification.failed";

/// Build a standard pipeline event carrying a JSON payload, preserving the
/// trace id so the whole interaction can be correlated end to end.
pub fn event(kind: &str, source: &str, payload: Value, trace: Uuid) -> Event {
    Event::new(kind, source, payload).with_trace(trace)
}
