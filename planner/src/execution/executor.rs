use std::collections::{HashMap, HashSet};
use std::sync::Mutex;

use async_trait::async_trait;
use friday_core::event::Event;
use friday_core::plugin::{Plugin, PluginMetadata};
use serde_json::json;
use uuid::Uuid;

use crate::events;
use crate::execution::dispatcher;
use crate::planning::{TaskGraph, TaskState};

/// Per-plan execution state. The engine is a plugin, so state lives behind a
/// mutex; `handle_event` locks, mutates, and unlocks without awaiting, keeping
/// it safe under the bus's sequential dispatch.
struct PlanRuntime {
    graph: TaskGraph,
    completed: HashSet<Uuid>,
}

/// The Execution Engine. It receives a plan, walks the task graph in dependency
/// order, dispatches each task to its plugin via `exec.action`, and tracks the
/// task lifecycle. It decides **which** plugin — never **how**.
pub struct ExecutionEngine {
    meta: PluginMetadata,
    state: Mutex<HashMap<Uuid, PlanRuntime>>,
}

impl Default for ExecutionEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl ExecutionEngine {
    pub fn new() -> Self {
        let meta = PluginMetadata::builder("execution", "Execution Engine")
            .description("Routes plan tasks to plugins and tracks their lifecycle")
            .subscribes(&[
                events::PLAN_CREATED,
                events::EXEC_RESULT,
                events::TASK_RETRY,
            ])
            .emits(&[
                events::TASK_STARTED,
                events::EXEC_ACTION,
                events::TASK_COMPLETED,
                events::TASK_FAILED,
            ])
            .build();
        Self {
            meta,
            state: Mutex::new(HashMap::new()),
        }
    }

    fn on_plan(&self, event: &Event) -> Vec<Event> {
        let graph: TaskGraph = match serde_json::from_value(event.payload.clone()) {
            Ok(g) => g,
            Err(e) => {
                tracing::error!(error = %e, "execution: invalid plan payload");
                return vec![];
            }
        };
        let plan_id = graph.plan_id;
        {
            let mut plans = self.state.lock().expect("execution state poisoned");
            plans.insert(
                plan_id,
                PlanRuntime {
                    graph: graph.clone(),
                    completed: HashSet::new(),
                },
            );
        }
        let mut out = Vec::new();
        let intent = graph.intent.clone();
        for tid in graph.ready_tasks(&HashSet::new()) {
            if let Some(task) = graph.task(tid) {
                out.push(task_event(
                    events::TASK_STARTED,
                    plan_id,
                    tid,
                    "running",
                    &intent,
                    event.trace_id,
                ));
                out.push(dispatcher::dispatch_event(task, plan_id, event.trace_id));
            }
        }
        out
    }

    fn on_result(&self, event: &Event) -> Vec<Event> {
        let (task_id, plan_id) = match ids(event) {
            Some(v) => v,
            None => return vec![],
        };
        let success = event
            .payload
            .get("success")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        let mut plans = self.state.lock().expect("execution state poisoned");
        let rt = match plans.get_mut(&plan_id) {
            Some(r) => r,
            None => return vec![],
        };

        let mut out = Vec::new();
        if success {
            rt.completed.insert(task_id);
            if let Some(t) = rt.graph.task_mut(task_id) {
                t.state = TaskState::Completed;
            }
            out.push(task_event(
                events::TASK_COMPLETED,
                plan_id,
                task_id,
                "completed",
                &rt.graph.intent,
                event.trace_id,
            ));
            for tid in rt.graph.ready_tasks(&rt.completed) {
                if let Some(task) = rt.graph.task(tid) {
                    out.push(task_event(
                        events::TASK_STARTED,
                        plan_id,
                        tid,
                        "running",
                        &rt.graph.intent,
                        event.trace_id,
                    ));
                    out.push(dispatcher::dispatch_event(task, plan_id, event.trace_id));
                }
            }
        } else {
            if let Some(t) = rt.graph.task_mut(task_id) {
                t.state = TaskState::Failed;
            }
            out.push(task_event(
                events::TASK_FAILED,
                plan_id,
                task_id,
                "failed",
                &rt.graph.intent,
                event.trace_id,
            ));
        }
        out
    }

    fn on_retry(&self, event: &Event) -> Vec<Event> {
        let (task_id, plan_id) = match ids(event) {
            Some(v) => v,
            None => return vec![],
        };
        let mut plans = self.state.lock().expect("execution state poisoned");
        let rt = match plans.get_mut(&plan_id) {
            Some(r) => r,
            None => return vec![],
        };
        let mut out = Vec::new();
        {
            if let Some(t) = rt.graph.task_mut(task_id) {
                t.state = TaskState::Retrying;
            }
        }
        if let Some(task) = rt.graph.task(task_id) {
            out.push(task_event(
                events::TASK_STARTED,
                plan_id,
                task_id,
                "retrying",
                &rt.graph.intent,
                event.trace_id,
            ));
            out.push(dispatcher::dispatch_event(task, plan_id, event.trace_id));
        }
        out
    }
}

fn ids(event: &Event) -> Option<(Uuid, Uuid)> {
    let task_id = event
        .payload
        .get("task_id")
        .and_then(|v| v.as_str())
        .and_then(|s| Uuid::parse_str(s).ok())?;
    let plan_id = event
        .payload
        .get("plan_id")
        .and_then(|v| v.as_str())
        .and_then(|s| Uuid::parse_str(s).ok())?;
    Some((task_id, plan_id))
}

fn task_event(
    kind: &str,
    plan_id: Uuid,
    task_id: Uuid,
    state: &str,
    intent: &str,
    trace: Uuid,
) -> Event {
    events::event(
        kind,
        "execution",
        json!({
            "plan_id": plan_id.to_string(),
            "task_id": task_id.to_string(),
            "state": state,
            "intent": intent,
        }),
        trace,
    )
}

#[async_trait]
impl Plugin for ExecutionEngine {
    fn metadata(&self) -> &PluginMetadata {
        &self.meta
    }

    async fn handle_event(&self, event: &Event) -> friday_core::Result<Vec<Event>> {
        Ok(match event.kind.as_str() {
            events::PLAN_CREATED => self.on_plan(event),
            events::EXEC_RESULT => self.on_result(event),
            events::TASK_RETRY => self.on_retry(event),
            _ => vec![],
        })
    }
}
