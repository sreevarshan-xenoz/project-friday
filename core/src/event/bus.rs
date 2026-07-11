use tokio::sync::broadcast;

use crate::error::{FridayError, Result};
use crate::event::Event;

/// Maximum number of times the router may re-publish a single event before
/// dropping it. Prevents runaway feedback loops between plugins.
pub const MAX_HOPS: u8 = 16;

/// Async, multi-producer / multi-consumer event bus built on a Tokio broadcast
/// channel. Every subscriber receives every event; filtering is the
/// subscriber's responsibility (see [`crate::plugin::PluginManager`]).
///
/// The bus is cheaply cloneable and is handed to every plugin so it can publish
/// without going through the core.
#[derive(Clone)]
pub struct EventBus {
    tx: broadcast::Sender<Event>,
}

impl Default for EventBus {
    fn default() -> Self {
        Self::new()
    }
}

impl EventBus {
    pub fn new() -> Self {
        let (tx, _rx) = broadcast::channel(2048);
        Self { tx }
    }

    /// Subscribe to the bus. Each receiver sees the full event stream.
    pub fn subscribe(&self) -> broadcast::Receiver<Event> {
        self.tx.subscribe()
    }

    /// Publish an event to all current subscribers.
    pub async fn publish(&self, event: Event) -> Result<()> {
        self.tx
            .send(event)
            .map(|_| ())
            .map_err(|e| FridayError::Bus(e.to_string()))
    }

    /// Number of active subscribers.
    pub fn subscriber_count(&self) -> usize {
        self.tx.receiver_count()
    }
}
