use std::sync::Arc;

use async_trait::async_trait;
use serde_json::Value;

use crate::error::Result;
use crate::event::Event;

use super::metadata::PluginMetadata;

/// Shared, read-only context handed to a plugin on initialisation. It exposes
/// the event bus for publishing and a configuration map. Plugins must not hold
/// references that outlive `init`.
#[derive(Clone)]
pub struct PluginContext {
    pub bus: crate::event::EventBus,
    pub config: Arc<std::collections::HashMap<String, Value>>,
}

/// The contract every FRIDAY capability implements. The core knows nothing
/// about what a plugin does; it only routes events to it based on
/// [`PluginMetadata::subscribes`]. This is what makes capabilities swappable:
/// a better speech recogniser simply replaces the voice plugin. All
/// intelligence lives in plugins; the core only routes and stores.
#[async_trait]
pub trait Plugin: Send + Sync {
    /// Static declaration of the plugin's identity and interests.
    fn metadata(&self) -> &PluginMetadata;

    /// Called once before the router starts. Use it to acquire resources.
    async fn init(&mut self, ctx: &PluginContext) -> Result<()> {
        let _ = ctx;
        Ok(())
    }

    /// Handle a single matched event. Return zero or more events to publish
    /// back onto the bus, driving the next stage of the pipeline. Returning an
    /// empty `Vec` means "handled, no further action".
    async fn handle_event(&self, event: &Event) -> Result<Vec<Event>>;

    /// Called on shutdown. Release resources and flush state.
    async fn shutdown(&mut self) -> Result<()> {
        Ok(())
    }
}
