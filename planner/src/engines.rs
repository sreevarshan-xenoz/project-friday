use async_trait::async_trait;
use friday_core::event::Event;
use friday_core::plugin::{Plugin, PluginMetadata};

use crate::events;
use crate::intent::{parse, Intent};

/// The Intent Engine. Its only job is understanding the request: it turns free
/// text into a structured [`Intent`]. It performs no planning and no execution.
pub struct IntentEngine {
    meta: PluginMetadata,
}

impl Default for IntentEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl IntentEngine {
    pub fn new() -> Self {
        let meta = PluginMetadata::builder("intent", "Intent Engine")
            .description("Resolves a raw user request into a structured goal")
            .subscribes(&[events::GOAL_RAW])
            .emits(&[events::INTENT_CREATED])
            .build();
        Self { meta }
    }
}

#[async_trait]
impl Plugin for IntentEngine {
    fn metadata(&self) -> &PluginMetadata {
        &self.meta
    }

    async fn handle_event(&self, event: &Event) -> friday_core::Result<Vec<Event>> {
        let raw = event
            .payload
            .get("text")
            .and_then(|v| v.as_str())
            .or_else(|| event.payload.as_str())
            .unwrap_or("")
            .to_string();
        let intent = parse(&raw);
        tracing::info!(intent = %intent.intent, "intent resolved");
        Ok(vec![events::event(
            events::INTENT_CREATED,
            "intent",
            serde_json::to_value(&intent)?,
            event.trace_id,
        )])
    }
}

/// The Planning Engine. It consumes an [`Intent`] and produces an executable
/// [`TaskGraph`](crate::planning::TaskGraph). It never executes anything.
pub struct PlanningEngine {
    meta: PluginMetadata,
}

impl Default for PlanningEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl PlanningEngine {
    pub fn new() -> Self {
        let meta = PluginMetadata::builder("planning", "Planning Engine")
            .description("Decomposes an intent into an executable task graph")
            .subscribes(&[events::INTENT_CREATED])
            .emits(&[events::PLAN_CREATED])
            .build();
        Self { meta }
    }
}

#[async_trait]
impl Plugin for PlanningEngine {
    fn metadata(&self) -> &PluginMetadata {
        &self.meta
    }

    async fn handle_event(&self, event: &Event) -> friday_core::Result<Vec<Event>> {
        let intent: Intent = serde_json::from_value(event.payload.clone())
            .map_err(|e| friday_core::FridayError::Internal(format!("invalid intent: {e}")))?;
        let graph = crate::planning::plan(&intent);
        tracing::info!(plan_id = %graph.plan_id, tasks = graph.tasks.len(), "plan created");
        Ok(vec![events::event(
            events::PLAN_CREATED,
            "planning",
            serde_json::to_value(&graph)?,
            event.trace_id,
        )])
    }
}
