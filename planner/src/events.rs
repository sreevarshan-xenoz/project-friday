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
/// Emitted by the runtime when an action produces a durable artifact (a file or
/// directory written to disk). Carries `path`, `capability`, `task_id`,
/// `plan_id`. Reflection and memory consume this to track what work was actually
/// produced, independent of which backend created it.
pub const ARTIFACT_CREATED: &str = "artifact.created";
/// Emitted by the runtime when a Git commit succeeds. Carries `commit_hash`,
/// `branch`, `files_changed`, `capability`, `task_id`, `plan_id`.
pub const GIT_COMMIT_CREATED: &str = "git.commit.created";
/// Emitted when the active branch changes (checkout or create-branch).
/// Carries `branch`, `capability`, `task_id`, `plan_id`.
pub const GIT_BRANCH_CHANGED: &str = "git.branch.changed";
/// Emitted when a diff is generated. Carries `diff_length`, `files_affected`,
/// `capability`, `task_id`, `plan_id`.
pub const GIT_DIFF_GENERATED: &str = "git.diff.generated";
pub const VERIFICATION_STARTED: &str = "verification.started";
pub const VERIFICATION_COMPLETED: &str = "verification.completed";
pub const VERIFICATION_FAILED: &str = "verification.failed";

// ---- M6 Autonomous Execution Engine: goal-level lifecycle ----
pub const GOAL_STARTED: &str = "goal.started";
pub const GOAL_COMPLETED: &str = "goal.completed";
pub const GOAL_FAILED: &str = "goal.failed";
pub const GOAL_PAUSED: &str = "goal.paused";
pub const GOAL_RESUMED: &str = "goal.resumed";
pub const GOAL_CANCELLED: &str = "goal.cancelled";
pub const TASK_READY: &str = "task.ready";
pub const TASK_PROGRESS: &str = "task.progress";
/// Incremental output line from a running task (M6.4 streaming). Unlike
/// `task.progress` (a 0..1 fraction), this carries a text chunk so clients can
/// render live logs (e.g. `Checking serde...`, `Compiling planner...`).
pub const TASK_STREAM: &str = "task.stream";
pub const TASK_VERIFYING: &str = "task.verifying";
pub const TASK_RETRYING: &str = "task.retrying";
pub const TASK_BLOCKED: &str = "task.blocked";
pub const PLAN_UPDATED: &str = "plan.updated";
pub const PERMISSION_PAUSED: &str = "goal.permission_paused";
/// Planning telemetry: emitted once per plan with routing/validation/outcome
/// metrics so rule vs LLM planning can be compared on evidence (M6.2).
pub const PLANNING_TELEMETRY: &str = "planning.telemetry";
/// Control events: external request to resume a paused goal (after a high-risk
/// approval) or cancel a running goal. The supervisor owns these.
pub const GOAL_RESUME: &str = "goal.resume";
pub const GOAL_CANCEL: &str = "goal.cancel";

/// Build a standard pipeline event carrying a JSON payload, preserving the
/// trace id so the whole interaction can be correlated end to end.
pub fn event(kind: &str, source: &str, payload: Value, trace: Uuid) -> Event {
    Event::new(kind, source, payload).with_trace(trace)
}
