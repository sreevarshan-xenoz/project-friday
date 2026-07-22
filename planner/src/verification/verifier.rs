use async_trait::async_trait;
use friday_core::event::Event;
use friday_core::plugin::{Plugin, PluginMetadata};
use serde_json::json;

use crate::events;
use crate::verification::result::VerificationResult;

/// The Verification Engine. It is a **pure outcome verifier**: it inspects a
/// task's result and emits `VERIFICATION_*` signals. It deliberately no longer
/// drives retry/replan — that recovery orchestration now lives in the
/// [`crate::execution::supervisor::GoalSupervisor`], which owns the goal
/// lifecycle and avoids double-dispatch with this engine.
pub struct VerificationEngine {
    meta: PluginMetadata,
}

impl Default for VerificationEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl VerificationEngine {
    pub fn new() -> Self {
        let meta = PluginMetadata::builder("verification", "Verification Engine")
            .description("Confirms task success and emits verification signals")
            .subscribes(&[events::TASK_COMPLETED, events::TASK_FAILED])
            .emits(&[
                events::VERIFICATION_STARTED,
                events::VERIFICATION_COMPLETED,
                events::VERIFICATION_FAILED,
            ])
            .build();
        Self { meta }
    }

    fn on_completed(&self, event: &Event) -> Vec<Event> {
        let (task_id, plan_id) = match events::extract_ids(event) {
            Some(v) => v,
            None => return vec![],
        };
        vec![
            events::event(
                events::VERIFICATION_STARTED,
                "verification",
                json!({ "plan_id": plan_id.to_string(), "task_id": task_id.to_string() }),
                event.trace_id,
            ),
            events::event(
                events::VERIFICATION_COMPLETED,
                "verification",
                json!({
                    "plan_id": plan_id.to_string(),
                    "task_id": task_id.to_string(),
                    "result": VerificationResult::Passed.as_str(),
                }),
                event.trace_id,
            ),
        ]
    }

    fn on_failed(&self, event: &Event) -> Vec<Event> {
        let (task_id, plan_id) = match events::extract_ids(event) {
            Some(v) => v,
            None => return vec![],
        };
        vec![
            events::event(
                events::VERIFICATION_STARTED,
                "verification",
                json!({ "plan_id": plan_id.to_string(), "task_id": task_id.to_string() }),
                event.trace_id,
            ),
            events::event(
                events::VERIFICATION_FAILED,
                "verification",
                json!({
                    "plan_id": plan_id.to_string(),
                    "task_id": task_id.to_string(),
                    "result": VerificationResult::Failed.as_str(),
                }),
                event.trace_id,
            ),
        ]
    }
}

#[async_trait]
impl Plugin for VerificationEngine {
    fn metadata(&self) -> &PluginMetadata {
        &self.meta
    }

    async fn handle_event(&self, event: &Event) -> friday_core::Result<Vec<Event>> {
        Ok(match event.kind.as_str() {
            events::TASK_COMPLETED => self.on_completed(event),
            events::TASK_FAILED => self.on_failed(event),
            _ => vec![],
        })
    }
}
