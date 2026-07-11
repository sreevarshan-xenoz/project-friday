use friday_core::event::Event;
use uuid::Uuid;

use crate::events;
use crate::planning::Task;

/// Build the `exec.action` event that dispatches a task to its executor plugin.
/// The event kind is always `exec.action`; the `capability` field tells the
/// receiving plugin which of its skills to invoke (see `plugin_selector`).
pub fn dispatch_event(task: &Task, plan_id: Uuid, trace: Uuid) -> Event {
    let payload = serde_json::json!({
        "task_id": task.id.to_string(),
        "plan_id": plan_id.to_string(),
        "capability": task.capability,
        "params": task.params,
    });
    Event::new(events::EXEC_ACTION, "execution", payload).with_trace(trace)
}
