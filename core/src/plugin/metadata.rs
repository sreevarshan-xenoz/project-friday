use serde::{Deserialize, Serialize};

/// Declarative description of a plugin. The core uses this to decide which
/// events a plugin receives and to present capabilities to users and the
/// permission layer.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginMetadata {
    pub id: String,
    pub name: String,
    pub version: String,
    pub description: String,
    /// Event `kind`s this plugin is interested in. The router only forwards
    /// matching events, keeping plugins fully decoupled.
    pub subscribes: Vec<String>,
    /// Event `kind`s this plugin may emit.
    pub emits: Vec<String>,
    /// Other plugin ids this plugin depends on (for startup ordering).
    pub dependencies: Vec<String>,
}

impl PluginMetadata {
    pub fn builder(id: impl Into<String>, name: impl Into<String>) -> PluginMetadataBuilder {
        PluginMetadataBuilder::new(id, name)
    }
}

/// Ergonomic builder so plugins declare metadata without boilerplate.
pub struct PluginMetadataBuilder {
    meta: PluginMetadata,
}

impl PluginMetadataBuilder {
    pub fn new(id: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            meta: PluginMetadata {
                id: id.into(),
                name: name.into(),
                version: "0.1.0".into(),
                description: String::new(),
                subscribes: Vec::new(),
                emits: Vec::new(),
                dependencies: Vec::new(),
            },
        }
    }

    pub fn version(mut self, v: impl Into<String>) -> Self {
        self.meta.version = v.into();
        self
    }

    pub fn description(mut self, d: impl Into<String>) -> Self {
        self.meta.description = d.into();
        self
    }

    pub fn subscribes(mut self, kinds: &[&str]) -> Self {
        self.meta.subscribes = kinds.iter().map(|s| s.to_string()).collect();
        self
    }

    pub fn emits(mut self, kinds: &[&str]) -> Self {
        self.meta.emits = kinds.iter().map(|s| s.to_string()).collect();
        self
    }

    pub fn dependencies(mut self, deps: &[&str]) -> Self {
        self.meta.dependencies = deps.iter().map(|s| s.to_string()).collect();
        self
    }

    pub fn build(self) -> PluginMetadata {
        self.meta
    }
}
