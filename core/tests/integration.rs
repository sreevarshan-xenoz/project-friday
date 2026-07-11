//! Integration test: proves the event bus routes an event to a registered
//! plugin and that the plugin's emitted event is re-published onto the bus.

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
        Ok(vec![Event::new("demo.pong", &self.meta.id, event.payload.clone())
            .with_trace(event.trace_id)])
    }
}

#[tokio::test]
async fn routes_ping_to_pong() {
    friday_core::init_telemetry();

    let mut friday = Friday::new();
    let meta = PluginMetadata::builder("echo", "Echo")
        .subscribes(&["demo.ping"])
        .emits(&["demo.pong"])
        .build();
    friday.register_plugin(Box::new(EchoPlugin { meta })).await.unwrap();

    let bus = friday.bus().clone();
    let mut rx = bus.subscribe();

    bus.publish(Event::new("demo.ping", "test", serde_json::json!({ "x": 1 })))
        .await
        .unwrap();

    let handle =
        tokio::spawn(async move { friday.run_with_timeout(Duration::from_secs(1)).await });

    let got = tokio::time::timeout(Duration::from_secs(1), async {
        loop {
            let ev = rx.recv().await.unwrap();
            if ev.kind == "demo.pong" {
                return ev;
            }
        }
    })
    .await
    .expect("should receive demo.pong");

    assert_eq!(got.payload, serde_json::json!({ "x": 1 }));
    handle.await.unwrap().unwrap();
}
