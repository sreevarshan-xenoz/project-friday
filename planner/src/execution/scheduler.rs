//! The Scheduler: promotes ready tasks and dispatching them as `exec.action`
//! events. It is the **only** module that touches the action runtime on behalf
//! of a goal, and it does so purely by emitting events — it never executes work
//! itself.
//!
//! The worker pool is a bounded set of concurrent slots. Ready tasks are
//! dispatched up to `pool_size` at a time; when results arrive the supervisor
//! calls [`Scheduler::collect_ready`] again to fill the pool. This keeps the
//! design parallel-ready without baking in "single worker" assumptions.

use friday_core::event::Event;

use crate::events;
use crate::execution::dispatcher;
use crate::execution::lifecycle::GoalRuntime;

/// Default concurrent worker slots. Small today; the pool is the seam where
/// specialized agents later attach.
pub const DEFAULT_POOL_SIZE: usize = 4;

/// Emit `task.ready` + `task.started` + `exec.action` for the next batch of
/// ready tasks, respecting the worker pool size and the goal's remaining
/// budget. Returns the events to publish (the supervisor appends them to the
/// bus). Mutates `rt` to mark the tasks `Running`.
pub fn schedule_batch(rt: &mut GoalRuntime, pool_size: usize) -> Vec<Event> {
    let mut out = Vec::new();
    let ready = rt.ready_tasks();

    // Respect the in-flight pool: don't start more than `pool_size` running.
    let in_flight = rt.running.len();
    let slots = pool_size.saturating_sub(in_flight);
    if slots == 0 {
        return out;
    }

    // Respect the step budget.
    let max_steps = rt.budget.max_steps.unwrap_or(u32::MAX);

    for tid in ready.into_iter().take(slots) {
        if rt.steps_taken >= max_steps {
            break;
        }
        if let Some(task) = rt.graph.task(tid).cloned() {
            out.push(events::event(
                events::TASK_READY,
                "supervisor",
                serde_json::json!({
                    "goal_id": rt.goal_id.to_string(),
                    "plan_id": rt.graph.plan_id.to_string(),
                    "task_id": tid.to_string(),
                    "capability": task.capability,
                }),
                rt.trace_id,
            ));
            rt.mark_running(tid);
            out.push(events::event(
                events::TASK_STARTED,
                "supervisor",
                serde_json::json!({
                    "goal_id": rt.goal_id.to_string(),
                    "plan_id": rt.graph.plan_id.to_string(),
                    "task_id": tid.to_string(),
                    "state": "running",
                    "intent": rt.intent,
                }),
                rt.trace_id,
            ));
            out.push(dispatcher::dispatch_event(
                &task,
                rt.graph.plan_id,
                "supervisor",
                rt.trace_id,
            ));
        }
    }
    out
}
