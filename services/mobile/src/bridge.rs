//! Event-bus → mobile bridge (M6.2a).
//!
//! The mobile service **subscribes** to the FRIDAY [`EventBus`] and republishes
//! relevant events to connected phones. It never polls: it is a passive bus
//! subscriber, exactly like every other FRIDAY plugin. The phone thus sees the
//! *same* event stream the rest of the system produces — planner lifecycle, task
//! execution, recovery, telemetry, notifications — with no synthetic dashboard.

use friday_core::event::{Event, EventBus};
use tokio::sync::broadcast;

use crate::protocol::ServerMessage;

/// Translates a bus [`Event`] into a [`ServerMessage`] for the phone.
///
/// The default behaviour is a faithful, topic-tagged forward: the event `kind`
/// becomes the phone-side topic, and the payload is passed through unchanged. The
/// launcher renders by topic; it never inspects FRIDAY internals.
pub fn translate(ev: &Event) -> ServerMessage {
    // A few well-known kinds get friendlier topic names so the launcher's five
    // tabs (Tasks / Terminal / Files / Settings / Home) can filter cheaply.
    let topic = if ev.kind.starts_with("notification") {
        "notifications".to_string()
    } else if ev.kind.starts_with("plan.")
        || ev.kind.starts_with("task.")
        || ev.kind.starts_with("goal.")
        || ev.kind.starts_with("intent.")
    {
        "tasks".to_string()
    } else {
        ev.kind.clone()
    };
    let mut payload = ev.payload.clone();
    // Surface the original event kind/source so a thin client can derive status
    // (e.g. task.started vs task.completed) without knowing FRIDAY internals.
    if let Some(obj) = payload.as_object_mut() {
        obj.insert(
            "_kind".to_string(),
            serde_json::Value::String(ev.kind.clone()),
        );
        obj.insert(
            "_source".to_string(),
            serde_json::Value::String(ev.source.clone()),
        );
    }
    ServerMessage::Event { topic, payload }
}

/// Bridges the bus to the mobile outbound channel.
pub struct BusBridge {
    bus: EventBus,
    outbound: broadcast::Sender<ServerMessage>,
}

impl BusBridge {
    pub fn new(bus: EventBus, outbound: broadcast::Sender<ServerMessage>) -> Self {
        Self { bus, outbound }
    }

    /// Spawn the forwarding task. Runs until the bus and every sender are dropped.
    pub fn start(self) {
        tokio::spawn(async move {
            let mut rx = self.bus.subscribe();
            tracing::info!("mobile bus-bridge subscribed");
            while let Ok(ev) = rx.recv().await {
                let msg = translate(&ev);
                if self.outbound.send(msg).is_err() {
                    // No connected clients; harmless.
                    break;
                }
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use friday_core::event::Event;

    #[test]
    fn translate_tags_known_kinds() {
        let planner = Event::new("plan.created", "planner", serde_json::json!({"x":1}));
        match translate(&planner) {
            ServerMessage::Event { topic, .. } => assert_eq!(topic, "tasks"),
            _ => panic!(),
        }
        let note = Event::new("notification.build", "recovery", serde_json::json!({}));
        match translate(&note) {
            ServerMessage::Event { topic, .. } => assert_eq!(topic, "notifications"),
            _ => panic!(),
        }
        let sys = Event::new("telemetry.system", "probe", serde_json::json!({}));
        match translate(&sys) {
            ServerMessage::Event { topic, .. } => assert_eq!(topic, "telemetry.system"),
            _ => panic!(),
        }
    }
}
