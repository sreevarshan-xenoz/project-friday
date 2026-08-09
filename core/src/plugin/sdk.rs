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

/// Any plugin can be shared behind an `Arc` and still registered on the bus.
/// Lets hosts keep one instance for direct calls (e.g. an agent driver) while
/// the same instance serves the event bus.
///
/// `Arc<T>` derefs shared, so only the `&self` methods forward; `init` and
/// `shutdown` (which need `&mut T`) are no-ops on the shared wrapper — use an
/// owned registration when lifecycle hooks matter.
#[async_trait]
impl<T: Plugin + ?Sized> Plugin for Arc<T> {
    fn metadata(&self) -> &PluginMetadata {
        (**self).metadata()
    }

    async fn init(&mut self, _ctx: &PluginContext) -> Result<()> {
        Ok(())
    }

    async fn handle_event(&self, event: &Event) -> Result<Vec<Event>> {
        (**self).handle_event(event).await
    }

    async fn shutdown(&mut self) -> Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Noop;

    #[async_trait]
    impl Plugin for Noop {
        fn metadata(&self) -> &PluginMetadata {
            static META: std::sync::OnceLock<PluginMetadata> = std::sync::OnceLock::new();
            META.get_or_init(|| PluginMetadata::builder("noop", "Noop").build())
        }

        async fn handle_event(&self, _event: &Event) -> Result<Vec<Event>> {
            Ok(vec![])
        }
    }

    #[tokio::test]
    async fn arc_plugin_delegates_to_the_inner() {
        let plugin: Arc<dyn Plugin> = Arc::new(Noop);
        assert_eq!(plugin.metadata().id, "noop");
        let ctx = PluginContext {
            bus: crate::event::EventBus::default(),
            config: Arc::new(std::collections::HashMap::new()),
        };
        let mut arc = plugin;
        assert!(arc.init(&ctx).await.is_ok());
        assert!(arc
            .handle_event(&Event::new("x", "t", serde_json::json!({})))
            .await
            .is_ok());
        assert!(arc.shutdown().await.is_ok());
    }
}
