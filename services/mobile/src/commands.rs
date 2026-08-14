//! Structured command gateway (M6.2a).
//!
//! The phone never sends free text and never understands FRIDAY internals. It
//! sends a small set of **structured commands**; this gateway translates each into
//! a real FRIDAY [`EventBus`] event that the Planner / AgentKernel / Recovery would
//! consume. The result is returned to the phone as a [`ServerMessage::CommandResult`].
//!
//! This is the single, stable protocol the launcher codes against. New capabilities
//! are new command methods here — not new phone-side logic.

use std::sync::Arc;

use friday_core::event::{Event, EventBus};
use serde_json::json;

use crate::protocol::ServerMessage;
use crate::session::SessionRegistry;
use crate::transport::Command;

/// Canonical command methods. The launcher codes against these constants only.
pub mod method {
    pub const PLANNER_GOAL: &str = "planner.goal";
    pub const TASK_CANCEL: &str = "task.cancel";
    pub const TASK_PAUSE: &str = "task.pause";
    pub const TASK_APPROVE: &str = "task.approve";
    pub const SYSTEM_INFO: &str = "system.info";
}

/// Translates structured mobile commands into bus events + command results.
pub struct CommandGateway {
    bus: EventBus,
    sessions: Arc<SessionRegistry>,
}

impl CommandGateway {
    pub fn new(bus: EventBus, sessions: Arc<SessionRegistry>) -> Self {
        Self { bus, sessions }
    }

    /// Handle one command and return the result to send back to the phone.
    pub async fn dispatch(&self, cmd: &Command) -> ServerMessage {
        let id = cmd.id.clone();
        let result = match cmd.method.as_str() {
            method::PLANNER_GOAL => {
                let goal = cmd
                    .params
                    .get("goal")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                // The GoalRunner subscribes to `plan.goal` and runs the goal.
                // The session id rides along so the runner keeps this
                // conversation continuous (per-session context, S1).
                let _ = self
                    .bus
                    .publish(Event::new(
                        "plan.goal",
                        "mobile",
                        json!({ "goal": goal, "session_id": cmd.session_id }),
                    ))
                    .await;
                json!({ "accepted": true })
            }
            method::TASK_CANCEL | method::TASK_PAUSE | method::TASK_APPROVE => {
                let suffix = match cmd.method.as_str() {
                    method::TASK_CANCEL => "cancel",
                    method::TASK_PAUSE => "pause",
                    _ => "approve",
                };
                let task_id = cmd
                    .params
                    .get("task_id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let _ = self
                    .bus
                    .publish(Event::new(
                        format!("task.{suffix}"),
                        "mobile",
                        json!({ "task_id": task_id }),
                    ))
                    .await;
                self.track_task(&cmd.session_id, &cmd.method, &task_id);
                json!({ "accepted": true })
            }
            method::SYSTEM_INFO => {
                // Real, fresh host metrics (no caching, no synthetic data).
                let s = crate::probe::SystemProbe::sample();
                serde_json::to_value(&s).unwrap_or_else(|_| json!({}))
            }
            other => {
                return ServerMessage::CommandResult {
                    id,
                    ok: false,
                    result: json!({ "error": "unknown command", "method": other }),
                };
            }
        };
        ServerMessage::CommandResult {
            id,
            ok: true,
            result,
        }
    }

    fn track_task(&self, session_id: &str, method: &str, task_id: &str) {
        if session_id.is_empty() || task_id.is_empty() {
            return;
        }
        let sid = session_id.to_string();
        let tid = task_id.to_string();
        match method {
            method::TASK_APPROVE => self.sessions.update(&sid, |s| {
                s.pending_approvals.retain(|t| t != &tid);
            }),
            method::TASK_CANCEL | method::TASK_PAUSE => self.sessions.update(&sid, |s| {
                s.active_tasks.remove(&tid);
            }),
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::SessionRegistry;

    fn gateway() -> CommandGateway {
        CommandGateway::new(EventBus::new(), SessionRegistry::new())
    }

    #[tokio::test]
    async fn unknown_command_is_rejected() {
        let g = gateway();
        let r = g
            .dispatch(&Command {
                session_id: "s".into(),
                id: "1".into(),
                method: "bogus".into(),
                params: json!({}),
            })
            .await;
        match r {
            ServerMessage::CommandResult { ok, .. } => assert!(!ok),
            _ => panic!(),
        }
    }

    #[tokio::test]
    async fn system_info_returns_real_sample() {
        let g = gateway();
        let r = g
            .dispatch(&Command {
                session_id: "s".into(),
                id: "1".into(),
                method: method::SYSTEM_INFO.into(),
                params: json!({}),
            })
            .await;
        match r {
            ServerMessage::CommandResult { ok, result, .. } => {
                assert!(ok);
                assert!(result.get("memory_total_mb").is_some());
            }
            _ => panic!(),
        }
    }

    #[tokio::test]
    async fn planner_goal_is_accepted() {
        let g = gateway();
        let r = g
            .dispatch(&Command {
                session_id: "s".into(),
                id: "1".into(),
                method: method::PLANNER_GOAL.into(),
                params: json!({ "goal": "Run cargo test" }),
            })
            .await;
        match r {
            ServerMessage::CommandResult { ok, result, .. } => {
                assert!(ok);
                assert_eq!(result["accepted"], true);
            }
            _ => panic!(),
        }
    }

    /// S1 acceptance: the session id must survive ingress — the gateway keeps
    /// it on the `plan.goal` event it publishes for the GoalRunner, so the
    /// runner can key per-session continuity by it.
    #[tokio::test]
    async fn planner_goal_publishes_session_id_on_plan_event() {
        let bus = EventBus::new();
        let mut rx = bus.subscribe();
        let g = CommandGateway::new(bus, SessionRegistry::new());
        let r = g
            .dispatch(&Command {
                session_id: "sess-abc".into(),
                id: "1".into(),
                method: method::PLANNER_GOAL.into(),
                params: json!({ "goal": "Check the git status" }),
            })
            .await;
        match r {
            ServerMessage::CommandResult { ok, .. } => assert!(ok),
            _ => panic!(),
        }
        let ev = tokio::time::timeout(std::time::Duration::from_secs(5), rx.recv())
            .await
            .expect("plan.goal must be published")
            .expect("bus alive");
        assert_eq!(ev.kind, "plan.goal");
        assert_eq!(ev.payload["goal"], "Check the git status");
        assert_eq!(ev.payload["session_id"], "sess-abc");
    }
}
