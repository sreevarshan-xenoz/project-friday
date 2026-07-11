//! Minimal demonstration of the plugin SDK and event bus.
//!
//! Run with: `cargo run -p friday-core --example echo_plugin`

use async_trait::async_trait;
use friday_core::event::Event;
use friday_core::plugin::{Plugin, PluginMetadata};
use friday_core::Friday;
use std::time::Duration;

struct EchoPlugin {
    meta: PluginMetadata,
}

#[async_trait]
impl Plugin for EchoPlugin {
    fn metadata(&self) -> &PluginMetadata {
        &self.meta
    }

    async fn handle_event(&self, event: &Event) -> friday_core::Result<Vec<Event>> {
        tracing::info!(received = %event.kind, "echo plugin handling event");
        let pong = Event::new("demo.pong", &self.meta.id, event.payload.clone())
            .with_trace(event.trace_id)
            .with_priority(event.priority);
        Ok(vec![pong])
    }
}

#[tokio::main]
async fn main() -> friday_core::Result<()> {
    friday_core::init_telemetry();

    let mut friday = Friday::new();

    let meta = PluginMetadata::builder("echo", "Echo Plugin")
        .description("Replies to pings with pongs")
        .subscribes(&["demo.ping"])
        .emits(&["demo.pong"])
        .build();
    friday.register_plugin(Box::new(EchoPlugin { meta })).await?;

    let bus = friday.bus().clone();
    bus.publish(Event::new(
        "demo.ping",
        "demo",
        serde_json::json!({ "hello": "world" }),
    ))
    .await?;

    tracing::info!("published demo.ping; running router for 500ms");
    friday.run_with_timeout(Duration::from_millis(500)).await?;

    Ok(())
}
