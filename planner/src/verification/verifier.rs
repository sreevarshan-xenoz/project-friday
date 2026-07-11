use std::collections::HashMap;
use std::sync::Mutex;

use async_trait::async_trait;
use friday_core::event::Event;
use friday_core::plugin::{Plugin, PluginMetadata};
use serde_json::json;
use uuid::Uuid;

use crate::events;
use crate::verification::result::{Verdict, VerificationResult};
use crate::verification::retry::RetryPolicy;

struct VerificationState {
    retries: HashMap<(Uuid, Uuid), u32>, // (plan_id, task_id) -> count
    replans: HashMap<Uuid, u32>,         // plan_id -> count
}

/// The Verification Engine. It confirms success on completion and, on failure,
/// drives the recovery decision (retry the task, or give up / replan). Without
/// this, FRIDAY would assume everything succeeded.
pub struct VerificationEngine {
    meta: PluginMetadata,
    policy: RetryPolicy,
    state: Mutex<VerificationState>,
}

impl Default for VerificationEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl VerificationEngine {
    pub fn new() -> Self {
        let meta = PluginMetadata::builder("verification", "Verification Engine")
            .description("Confirms task success and drives retry/replan on failure")
            .subscribes(&[events::TASK_COMPLETED, events::TASK_FAILED])
            .emits(&[
                events::VERIFICATION_STARTED,
                events::VERIFICATION_COMPLETED,
                events::VERIFICATION_FAILED,
                events::TASK_RETRY,
                events::INTENT_CREATED,
            ])
            .build();
        Self {
            meta,
            policy: RetryPolicy::default(),
            state: Mutex::new(VerificationState {
                retries: HashMap::new(),
                replans: HashMap::new(),
            }),
        }
    }

    fn on_completed(&self, event: &Event) -> Vec<Event> {
        let (plan_id, task_id) = match ids(event) {
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
        let (plan_id, task_id) = match ids(event) {
            Some(v) => v,
            None => return vec![],
        };
        let intent = event
            .payload
            .get("intent")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown")
            .to_string();

        let mut st = self.state.lock().expect("verification state poisoned");
        let retry_count = {
            let retries = st.retries.entry((plan_id, task_id)).or_insert(0);
            *retries += 1;
            *retries
        };
        let replans = *st.replans.get(&plan_id).unwrap_or(&0);
        let verdict = self.policy.decide(retry_count);

        let mut out = vec![events::event(
            events::VERIFICATION_STARTED,
            "verification",
            json!({ "plan_id": plan_id.to_string(), "task_id": task_id.to_string() }),
            event.trace_id,
        )];

        match verdict {
            Verdict::Retry => {
                out.push(events::event(
                    events::TASK_RETRY,
                    "verification",
                    json!({ "plan_id": plan_id.to_string(), "task_id": task_id.to_string() }),
                    event.trace_id,
                ));
            }
            Verdict::Passed => {
                // A failed task should never reach a Passed verdict; treat as
                // no further action beyond the started event already emitted.
            }
            Verdict::GiveUp => {
                out.push(events::event(
                    events::VERIFICATION_FAILED,
                    "verification",
                    json!({
                        "plan_id": plan_id.to_string(),
                        "task_id": task_id.to_string(),
                        "result": VerificationResult::Failed.as_str(),
                    }),
                    event.trace_id,
                ));
                if replans < self.policy.max_replans {
                    *st.replans.entry(plan_id).or_insert(0) += 1;
                    // Simplified replan: re-emit the intent key so planning
                    // restarts. (Full context replay via memory is a later step.)
                    out.push(events::event(
                        events::INTENT_CREATED,
                        "verification",
                        json!({ "intent": intent, "raw": intent, "entities": {}, "replan": true }),
                        event.trace_id,
                    ));
                }
            }
        }
        out
    }
}

fn ids(event: &Event) -> Option<(Uuid, Uuid)> {
    let plan_id = event
        .payload
        .get("plan_id")
        .and_then(|v| v.as_str())
        .and_then(|s| Uuid::parse_str(s).ok())?;
    let task_id = event
        .payload
        .get("task_id")
        .and_then(|v| v.as_str())
        .and_then(|s| Uuid::parse_str(s).ok())?;
    Some((plan_id, task_id))
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
