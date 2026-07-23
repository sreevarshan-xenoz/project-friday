use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use crate::error::Result;
use crate::event::EventBus;
use crate::memory::MemoryEngine;
use crate::memory::MemoryLayer;
use crate::plugin::{Plugin, PluginManager};

/// The FRIDAY runtime: the minimal wired core that everything else plugs into.
///
/// It owns the event bus, the plugin router, and the memory engine. The core
/// itself contains no intelligence — it only routes events and stores memory.
/// All behaviour lives in plugins; this struct merely wires them together.
pub struct Friday {
    bus: EventBus,
    plugins: PluginManager,
    memory: MemoryEngine,
    config: Arc<HashMap<String, serde_json::Value>>,
}

impl Default for Friday {
    fn default() -> Self {
        Self::new()
    }
}

impl Friday {
    pub fn new() -> Self {
        let bus = EventBus::new();
        Self {
            bus: bus.clone(),
            plugins: PluginManager::new(bus),
            memory: MemoryEngine::new(),
            config: Arc::new(HashMap::new()),
        }
    }

    /// Register a capability plugin. It is initialised immediately against the
    /// shared configuration.
    pub async fn register_plugin(&mut self, plugin: Box<dyn Plugin>) -> Result<()> {
        self.plugins.register(plugin, self.config.clone()).await
    }

    /// Register a memory tier (working, session, long-term, ...).
    pub fn register_memory_layer(&mut self, layer: Box<dyn MemoryLayer>) {
        self.memory.register_layer(layer);
    }

    /// Shared, read-only configuration (cloned `Arc`).
    pub fn config(&self) -> &Arc<HashMap<String, serde_json::Value>> {
        &self.config
    }

    /// Start the event router. This future resolves only when the bus closes.
    pub async fn run(&mut self) -> Result<()> {
        self.plugins.run().await
    }

    /// Run the router until either the bus closes or `timeout` elapses. Useful
    /// for tests and demos that must terminate.
    pub async fn run_with_timeout(&mut self, timeout: Duration) -> Result<()> {
        tokio::select! {
            r = self.plugins.run() => r,
            _ = tokio::time::sleep(timeout) => {
                tracing::info!("runtime timeout reached; stopping router");
                Ok(())
            }
        }
    }

    pub fn bus(&self) -> &EventBus {
        &self.bus
    }

    pub fn memory(&self) -> &MemoryEngine {
        &self.memory
    }

    pub fn plugin_count(&self) -> usize {
        self.plugins.plugin_count()
    }
}
