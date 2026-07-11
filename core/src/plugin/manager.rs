use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::broadcast::{error::RecvError, Receiver};

use crate::error::{FridayError, Result};
use crate::event::{Event, EventBus, MAX_HOPS};

use super::sdk::{Plugin, PluginContext};

/// Owns all registered plugins and routes bus events to the ones that declared
/// interest. It also feeds plugin-produced events back onto the bus, forming
/// the event-driven pipeline described in the architecture (voice → planner →
/// execution → verification → memory → voice).
pub struct PluginManager {
    plugins: Vec<Box<dyn Plugin>>,
    bus: EventBus,
    /// Subscribed eagerly at construction so no event published after the
    /// manager exists is ever missed, even before `run` is called.
    rx: Receiver<Event>,
}

impl PluginManager {
    pub fn new(bus: EventBus) -> Self {
        let rx = bus.subscribe();
        Self {
            plugins: Vec::new(),
            bus,
            rx,
        }
    }

    /// Register and initialise a plugin. Its metadata is used for routing and
    /// it is given access to the bus and configuration.
    pub async fn register(
        &mut self,
        mut plugin: Box<dyn Plugin>,
        config: Arc<HashMap<String, serde_json::Value>>,
    ) -> Result<()> {
        let id = plugin.metadata().id.clone();
        let ctx = PluginContext {
            bus: self.bus.clone(),
            config,
        };
        plugin
            .init(&ctx)
            .await
            .map_err(|e| FridayError::Plugin(id.clone(), e.to_string()))?;
        self.plugins.push(plugin);
        tracing::info!(plugin = %id, "plugin registered");
        Ok(())
    }

    fn plugins_for<'a>(&'a self, kind: &'a str) -> impl Iterator<Item = &'a Box<dyn Plugin>> + 'a {
        self.plugins.iter().filter(move |p| {
            p.metadata()
                .subscribes
                .iter()
                .any(|k| matches_kind(k, kind))
        })
    }

    /// Begin routing. Subscribes to the bus and dispatches every received
    /// event to interested plugins, re-publishing their outputs (bounded by
    /// [`MAX_HOPS`]). Runs until the bus is closed.
    pub async fn run(&mut self) -> Result<()> {
        tracing::info!(plugins = self.plugins.len(), "plugin router started");
        loop {
            match self.rx.recv().await {
                Ok(event) => self.dispatch(event).await,
                Err(RecvError::Closed) => break,
                Err(RecvError::Lagged(n)) => {
                    tracing::warn!(skipped = n, "router lagged behind event bus");
                }
            }
        }
        Ok(())
    }

    async fn dispatch(&self, event: Event) {
        for plugin in self.plugins_for(&event.kind) {
            let id = plugin.metadata().id.clone();
            match plugin.handle_event(&event).await {
                Ok(events) => {
                    for mut out in events {
                        if out.trace_id == event.trace_id {
                            out.hops = event.hops + 1;
                        }
                        if out.hops > MAX_HOPS {
                            tracing::warn!(kind = %out.kind, "event exceeded MAX_HOPS; dropped");
                            continue;
                        }
                        if let Err(e) = self.bus.publish(out).await {
                            tracing::error!(error = %e, "failed to publish plugin output");
                        }
                    }
                }
                Err(e) => {
                    tracing::error!(plugin = %id, error = %e, "plugin failed to handle event")
                }
            }
        }
    }

    pub fn plugin_count(&self) -> usize {
        self.plugins.len()
    }
}

/// Exact match for now; a future version can support wildcards (`vision.*`).
fn matches_kind(subscribed: &str, kind: &str) -> bool {
    subscribed == kind
}
