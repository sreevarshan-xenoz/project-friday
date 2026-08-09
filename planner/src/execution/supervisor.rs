//! GoalSupervisor — the orchestration core of the M6 Autonomous Execution
//! Engine.
//!
//! Per the architecture, the supervisor contains **no execution logic**. It
//! owns:
//!
//! * the goal-level state machine ([`GoalRuntime`]),
//! * capability resolution ([`CapabilityRegistry`]),
//! * the high-risk permission gate ([`PermissionEngine`]),
//! * reflection dispatch ([`Reflector`]),
//! * durable persistence ([`GoalStore`]),
//! * and emitting the `exec.action` events the scheduler builds.
//!
//! It reacts to pipeline events and drives the DAG to completion, pausing only
//! at explicit high-risk safety gates. Every transition is persisted as an
//! immutable [`GoalEvent`] for replay/debugging.
//!
//! **Locking discipline (DO NOT VIOLATE):**
//!
//! 1. The supervisor holds `self.goals` for the duration of a single
//!    `handle_event` call. All helper methods therefore take `&mut GoalRuntime`
//!    (already borrowed) rather than re-locking — `Mutex` is not reentrant, so
//!    calling another supervisor method that locks `self.goals` while the lock
//!    is held **will deadlock** (this happened once during development).
//!
//! 2. Guideline: **never call another supervisor method that locks `self.goals`
//!    while already holding the goal lock.** If a helper needs goal state, pass
//!    it `&mut GoalRuntime`. The only methods that may take the lock are the
//!    top-level `on_*` entry points invoked from `handle_event`.
//!
//! 3. External locks (`CapabilityRegistry`'s `RwLock`, `GoalStore`'s redb
//!    mutex) are safe to touch while holding `self.goals` — they never acquire
//!    `self.goals` in return. Keep it that way.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::Mutex;

use async_trait::async_trait;
use friday_core::event::Event;
use friday_core::plugin::{Plugin, PluginMetadata};
use serde_json::json;
use uuid::Uuid;

use crate::events;
use crate::execution::capability_registry::{CapabilityRegistry, Risk};
use crate::execution::dispatcher;
use crate::execution::lifecycle::{ExecutionBudget, GoalRuntime};
use crate::execution::persistence::{GoalEvent, GoalStore};
use crate::execution::reflection::{ReflectionAction, Reflector, RuleReflector};
use crate::planning::{GoalState, TaskGraph, TaskState};

/// Policy authority for execution. The supervisor consults it for medium-risk
/// gating; high-risk gating is driven by capability [`Risk`] metadata.
pub trait PolicyAuthority: Send + Sync {
    /// True if a capability (resolved to `tool.action`) is permitted by policy
    /// without human interaction.
    fn allows(&self, tool: &str, action: &str) -> bool;
}

/// The orchestration plugin.
pub struct GoalSupervisor {
    meta: PluginMetadata,
    registry: Arc<CapabilityRegistry>,
    policy: Arc<dyn PolicyAuthority>,
    reflector: Arc<dyn Reflector>,
    store: Arc<GoalStore>,
    pool_size: usize,
    goals: Mutex<HashMap<Uuid, GoalRuntime>>,
}

impl GoalSupervisor {
    pub fn new(
        registry: Arc<CapabilityRegistry>,
        policy: Arc<dyn PolicyAuthority>,
        store: Arc<GoalStore>,
    ) -> Self {
        Self::with_reflector_and_pool(registry, policy, store, Arc::new(RuleReflector::new(1)), 4)
    }

    pub fn with_reflector_and_pool(
        registry: Arc<CapabilityRegistry>,
        policy: Arc<dyn PolicyAuthority>,
        store: Arc<GoalStore>,
        reflector: Arc<dyn Reflector>,
        pool_size: usize,
    ) -> Self {
        let meta = PluginMetadata::builder("supervisor", "Goal Supervisor")
            .description(
                "Owns goal lifecycle, scheduling, risk gating, reflection, and persistence",
            )
            .subscribes(&[
                events::PLAN_CREATED,
                events::EXEC_RESULT,
                events::GOAL_RESUME,
                events::GOAL_CANCEL,
            ])
            .emits(&[
                events::GOAL_STARTED,
                events::GOAL_COMPLETED,
                events::GOAL_FAILED,
                events::GOAL_PAUSED,
                events::GOAL_RESUMED,
                events::GOAL_CANCELLED,
                events::TASK_READY,
                events::TASK_STARTED,
                events::TASK_COMPLETED,
                events::TASK_FAILED,
                events::TASK_RETRYING,
                events::TASK_PROGRESS,
                events::TASK_STREAM,
                events::PERMISSION_PAUSED,
                events::EXEC_ACTION,
            ])
            .build();
        Self {
            meta,
            registry,
            policy,
            reflector,
            store,
            pool_size,
            goals: Mutex::new(HashMap::new()),
        }
    }

    fn emit_goal_created(&self, goal_id: Uuid, rt: &GoalRuntime) -> Event {
        self.store
            .append(&GoalEvent::GoalCreated {
                goal_id,
                trace_id: rt.trace_id,
                intent: rt.intent.clone(),
            })
            .ok();
        events::event(
            events::GOAL_STARTED,
            "supervisor",
            json!({
                "goal_id": goal_id.to_string(),
                "trace_id": rt.trace_id.to_string(),
                "intent": rt.intent,
                "state": rt.state.as_str(),
            }),
            rt.trace_id,
        )
    }

    fn on_plan(&self, event: &Event) -> Vec<Event> {
        let graph: TaskGraph = match serde_json::from_value(event.payload.clone()) {
            Ok(g) => g,
            Err(e) => {
                tracing::error!(error = %e, "supervisor: invalid plan payload");
                return vec![];
            }
        };
        let mut goals = self.goals.lock().expect("goal map poisoned");
        // Find or create the goal runtime for this trace.
        let goal_id =
            if let Some((id, _)) = goals.iter().find(|(_, g)| g.trace_id == event.trace_id) {
                *id
            } else {
                let id = Uuid::new_v4();
                goals.insert(
                    id,
                    GoalRuntime::new(
                        id,
                        event.trace_id,
                        graph.intent.clone(),
                        ExecutionBudget::default(),
                    ),
                );
                id
            };
        let rt = goals.get_mut(&goal_id).expect("goal just inserted");
        rt.set_plan(graph);
        self.store
            .append(&GoalEvent::PlanSet {
                goal_id,
                plan_id: rt.graph.plan_id,
            })
            .ok();
        let created = self.emit_goal_created(goal_id, rt);
        let mut out = vec![created];
        out.extend(self.fill_pool(rt));
        out
    }

    /// Schedule the next batch of ready, non-gated tasks. High-risk tasks cause
    /// a goal pause instead of dispatch. Operates on an already-borrowed
    /// runtime (no re-lock).
    fn fill_pool(&self, rt: &mut GoalRuntime) -> Vec<Event> {
        let mut out = Vec::new();
        if rt.state != GoalState::Running {
            return out;
        }
        let ready: Vec<Uuid> = rt.ready_tasks();
        let in_flight = rt.running.len();
        let slots = self.pool_size.saturating_sub(in_flight);
        let max_steps = rt.budget.max_steps.unwrap_or(u32::MAX);
        let mut dispatched = 0usize;
        for tid in ready {
            if dispatched >= slots || rt.steps_taken >= max_steps {
                break;
            }
            let cap = rt
                .graph
                .task(tid)
                .map(|t| t.capability.clone())
                .unwrap_or_default();
            let risk = self.registry.risk_of(&cap);
            match risk {
                Risk::High => {
                    self.pause_for_permission(&mut out, rt, tid, cap);
                    break;
                }
                Risk::Safe | Risk::Medium => {
                    let binding = self.registry.resolve(&cap);
                    let (tool, action) =
                        binding.map(|b| (b.adapter, b.action)).unwrap_or_else(|| {
                            let mut parts = cap.splitn(2, '.');
                            (
                                parts.next().unwrap_or(&cap).to_string(),
                                parts.next().unwrap_or("").to_string(),
                            )
                        });
                    if risk == Risk::Medium && !self.policy.allows(&tool, &action) {
                        self.pause_for_permission(&mut out, rt, tid, cap);
                        break;
                    }
                    let task = rt.graph.task(tid).cloned().expect("task exists");
                    out.push(events::event(
                        events::TASK_READY,
                        "supervisor",
                        json!({
                            "goal_id": rt.goal_id.to_string(),
                            "plan_id": rt.graph.plan_id.to_string(),
                            "task_id": tid.to_string(),
                            "capability": cap,
                        }),
                        rt.trace_id,
                    ));
                    rt.mark_running(tid);
                    out.push(events::event(
                        events::TASK_STARTED,
                        "supervisor",
                        json!({
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
                    dispatched += 1;
                }
            }
        }
        out
    }

    fn pause_for_permission(
        &self,
        out: &mut Vec<Event>,
        rt: &mut GoalRuntime,
        task_id: Uuid,
        cap: String,
    ) {
        rt.pending_capability = Some(cap.clone());
        rt.pending_task = Some(task_id);
        rt.transition(GoalState::Paused);
        let goal_id = rt.goal_id;
        self.store
            .append(&GoalEvent::PermissionPaused {
                goal_id,
                capability: cap.clone(),
            })
            .ok();
        self.store.append(&GoalEvent::GoalPaused { goal_id }).ok();
        self.store.snapshot(rt).ok();
        out.push(events::event(
            events::PERMISSION_PAUSED,
            "supervisor",
            json!({
                "goal_id": goal_id.to_string(),
                "task_id": task_id.to_string(),
                "capability": cap,
                "state": "paused",
            }),
            rt.trace_id,
        ));
        out.push(events::event(
            events::GOAL_PAUSED,
            "supervisor",
            json!({
                "goal_id": goal_id.to_string(),
                "state": "paused",
                "reason": "high_risk_capability",
            }),
            rt.trace_id,
        ));
    }

    fn on_result(&self, event: &Event) -> Vec<Event> {
        let (task_id, plan_id) = match events::extract_ids(event) {
            Some(v) => v,
            None => return vec![],
        };
        let success = event
            .payload
            .get("success")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let error = event
            .payload
            .get("error")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        let output = event.payload.get("output").cloned().unwrap_or(json!(null));

        let mut goals = self.goals.lock().expect("goal map poisoned");
        let goal_id = match goals
            .iter()
            .find(|(_, g)| g.graph.plan_id == plan_id)
            .map(|(id, _)| *id)
        {
            Some(id) => id,
            None => return vec![],
        };
        let rt = match goals.get_mut(&goal_id) {
            Some(rt) => rt,
            None => return vec![],
        };

        // M6.4: emit any incremental stream chunks / progress carried by the
        // exec result so clients can render live output (no architecture change
        // needed downstream).
        let mut out = Vec::new();
        if let Some(chunks) = event.payload.get("stream").and_then(|v| v.as_array()) {
            for chunk in chunks {
                if let Some(line) = chunk.as_str() {
                    out.push(events::event(
                        events::TASK_STREAM,
                        "supervisor",
                        json!({
                            "goal_id": goal_id.to_string(),
                            "plan_id": plan_id.to_string(),
                            "task_id": task_id.to_string(),
                            "chunk": line,
                        }),
                        rt.trace_id,
                    ));
                }
            }
        }
        if let Some(p) = event.payload.get("progress").and_then(|v| v.as_f64()) {
            out.push(events::event(
                events::TASK_PROGRESS,
                "supervisor",
                json!({
                    "goal_id": goal_id.to_string(),
                    "plan_id": plan_id.to_string(),
                    "task_id": task_id.to_string(),
                    "progress": p,
                }),
                rt.trace_id,
            ));
        }

        if success {
            rt.mark_completed(task_id, output.clone());
            // M6.3: publish the task's output into the shared goal context under
            // the key declared by `Task.produces`, so downstream tasks can reuse
            // it instead of recomputing (e.g. cargo metadata → workspace_root).
            if let Some(key) = rt.graph.task(task_id).and_then(|t| t.produces.clone()) {
                let key_clone = key.clone();
                rt.context.publish(key_clone, task_id, output.clone());
                self.store
                    .append(&GoalEvent::ContextPublished {
                        goal_id,
                        task_id,
                        key,
                    })
                    .ok();
            }
            self.store
                .append(&GoalEvent::TaskCompleted { goal_id, task_id })
                .ok();
            out.push(events::event(
                events::TASK_COMPLETED,
                "supervisor",
                json!({
                    "goal_id": goal_id.to_string(),
                    "plan_id": plan_id.to_string(),
                    "task_id": task_id.to_string(),
                    "state": "completed",
                    "output": output,
                }),
                rt.trace_id,
            ));

            match self
                .reflector
                .reflect(rt, task_id, TaskState::Completed, true, None)
            {
                ReflectionAction::Continue => {}
                other => out.extend(self.apply_reflection(rt, goal_id, task_id, other)),
            }

            if rt.is_complete() {
                rt.transition(GoalState::Completed);
                self.store
                    .append(&GoalEvent::GoalCompleted { goal_id })
                    .ok();
                self.store.snapshot(rt).ok();
                out.push(events::event(
                    events::GOAL_COMPLETED,
                    "supervisor",
                    json!({
                        "goal_id": goal_id.to_string(),
                        "state": "completed",
                        "intent": rt.intent,
                    }),
                    rt.trace_id,
                ));
            } else {
                out.extend(self.fill_pool(rt));
            }
            out
        } else {
            rt.mark_failed(task_id);
            self.store
                .append(&GoalEvent::TaskFailed {
                    goal_id,
                    task_id,
                    error: error.clone(),
                })
                .ok();
            let mut out = vec![events::event(
                events::TASK_FAILED,
                "supervisor",
                json!({
                    "goal_id": goal_id.to_string(),
                    "plan_id": plan_id.to_string(),
                    "task_id": task_id.to_string(),
                    "state": "failed",
                    "error": error,
                }),
                rt.trace_id,
            )];
            let action = self
                .reflector
                .reflect(rt, task_id, TaskState::Failed, false, None);
            out.extend(self.apply_reflection(rt, goal_id, task_id, action));
            out
        }
    }

    /// Translate a reflection decision into orchestration events. Operates on an
    /// already-borrowed runtime (no re-lock). Does NOT call `fill_pool`; the
    /// caller does that after this returns.
    fn apply_reflection(
        &self,
        rt: &mut GoalRuntime,
        goal_id: Uuid,
        task_id: Uuid,
        action: ReflectionAction,
    ) -> Vec<Event> {
        let mut out = Vec::new();
        match action {
            ReflectionAction::Continue => {}
            ReflectionAction::Retry | ReflectionAction::Repair => {
                let attempt = rt.inc_retries(task_id);
                self.store
                    .append(&GoalEvent::TaskRetried {
                        goal_id,
                        task_id,
                        attempt,
                    })
                    .ok();
                if let Some(task) = rt.graph.task(task_id).cloned() {
                    out.push(events::event(
                        events::TASK_RETRYING,
                        "supervisor",
                        json!({
                            "goal_id": goal_id.to_string(),
                            "plan_id": rt.graph.plan_id.to_string(),
                            "task_id": task_id.to_string(),
                            "state": "retrying",
                            "attempt": attempt,
                        }),
                        rt.trace_id,
                    ));
                    rt.mark_running(task_id);
                    out.push(events::event(
                        events::TASK_STARTED,
                        "supervisor",
                        json!({
                            "goal_id": goal_id.to_string(),
                            "plan_id": rt.graph.plan_id.to_string(),
                            "task_id": task_id.to_string(),
                            "state": "retrying",
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
            ReflectionAction::Split | ReflectionAction::Replan => {
                out.push(events::event(
                    events::INTENT_CREATED,
                    "supervisor",
                    json!({
                        "intent": rt.intent,
                        "raw": rt.intent,
                        "entities": {},
                        "replan": true,
                        "goal_id": goal_id.to_string(),
                    }),
                    rt.trace_id,
                ));
            }
            ReflectionAction::Pause => {
                let cap = rt
                    .graph
                    .task(task_id)
                    .map(|t| t.capability.clone())
                    .unwrap_or_default();
                self.pause_for_permission(&mut out, rt, task_id, cap);
            }
            ReflectionAction::Abort => {
                rt.transition(GoalState::Failed);
                self.store
                    .append(&GoalEvent::GoalFailed {
                        goal_id,
                        reason: "reflection_aborted".to_string(),
                    })
                    .ok();
                self.store.snapshot(rt).ok();
                out.push(events::event(
                    events::GOAL_FAILED,
                    "supervisor",
                    json!({
                        "goal_id": goal_id.to_string(),
                        "state": "failed",
                        "reason": "reflection_aborted",
                    }),
                    rt.trace_id,
                ));
            }
        }
        out
    }

    fn on_resume(&self, event: &Event) -> Vec<Event> {
        let goal_id = match event
            .payload
            .get("goal_id")
            .and_then(|v| v.as_str())
            .and_then(|s| Uuid::parse_str(s).ok())
        {
            Some(id) => id,
            None => return vec![],
        };
        let mut goals = self.goals.lock().expect("goal map poisoned");
        let rt = match goals.get_mut(&goal_id) {
            Some(rt) => rt,
            None => return vec![],
        };
        if rt.state != GoalState::Paused {
            return vec![];
        }
        rt.transition(GoalState::Running);
        rt.pending_capability = None;
        let pending = rt.pending_task.take();
        let mut out = vec![events::event(
            events::GOAL_RESUMED,
            "supervisor",
            json!({ "goal_id": goal_id.to_string(), "state": "running" }),
            rt.trace_id,
        )];
        if let Some(tid) = pending {
            if let Some(task) = rt.graph.task(tid).cloned() {
                rt.mark_running(tid);
                out.push(events::event(
                    events::TASK_STARTED,
                    "supervisor",
                    json!({
                        "goal_id": goal_id.to_string(),
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
        out.extend(self.fill_pool(rt));
        out
    }

    fn on_cancel(&self, event: &Event) -> Vec<Event> {
        let goal_id = match event
            .payload
            .get("goal_id")
            .and_then(|v| v.as_str())
            .and_then(|s| Uuid::parse_str(s).ok())
        {
            Some(id) => id,
            None => return vec![],
        };
        let mut goals = self.goals.lock().expect("goal map poisoned");
        let rt = match goals.get_mut(&goal_id) {
            Some(rt) => rt,
            None => return vec![],
        };
        rt.transition(GoalState::Cancelled);
        self.store
            .append(&GoalEvent::GoalCancelled { goal_id })
            .ok();
        self.store.snapshot(rt).ok();
        vec![events::event(
            events::GOAL_CANCELLED,
            "supervisor",
            json!({ "goal_id": goal_id.to_string(), "state": "cancelled" }),
            rt.trace_id,
        )]
    }
}

#[async_trait]
impl Plugin for GoalSupervisor {
    fn metadata(&self) -> &PluginMetadata {
        &self.meta
    }

    async fn handle_event(&self, event: &Event) -> friday_core::Result<Vec<Event>> {
        Ok(match event.kind.as_str() {
            events::PLAN_CREATED => self.on_plan(event),
            events::EXEC_RESULT => self.on_result(event),
            events::GOAL_RESUME => self.on_resume(event),
            events::GOAL_CANCEL => self.on_cancel(event),
            _ => vec![],
        })
    }
}
