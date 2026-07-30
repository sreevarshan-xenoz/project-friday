use std::collections::HashMap;

use crate::contract::{Capability, PluginDescriptor};

/// Capability Registry — maps capabilities to plugins that provide them.
///
/// Internally represented as a graph (capability → plugin list) rather
/// than a flat list, enabling future capability inheritance and grouping.
pub struct CapabilityRegistry {
    capabilities: HashMap<Capability, Vec<String>>,
    descriptors: HashMap<String, PluginDescriptor>,
}

impl Default for CapabilityRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl CapabilityRegistry {
    pub fn new() -> Self {
        Self {
            capabilities: HashMap::new(),
            descriptors: HashMap::new(),
        }
    }

    /// Register a plugin and all its capabilities.
    pub fn register(&mut self, descriptor: PluginDescriptor) {
        let plugin_id = descriptor.id.clone();

        for cap in &descriptor.capabilities {
            self.capabilities
                .entry(cap.clone())
                .or_default()
                .push(plugin_id.clone());
        }

        self.descriptors.insert(plugin_id, descriptor);
    }

    /// Unregister a plugin and remove it from all capability mappings.
    pub fn unregister(&mut self, plugin_id: &str) {
        self.descriptors.remove(plugin_id);
        self.capabilities.values_mut().for_each(|plugins| {
            plugins.retain(|id| id != plugin_id);
        });
        self.capabilities.retain(|_, plugins| !plugins.is_empty());
    }

    /// Find all plugin IDs that support the given capability.
    pub fn plugins_for(&self, capability: &Capability) -> Vec<&str> {
        self.capabilities
            .get(capability)
            .map(|plugins| plugins.iter().map(|s| s.as_str()).collect())
            .unwrap_or_default()
    }

    /// Check if a specific plugin supports a capability.
    pub fn plugin_supports(&self, plugin_id: &str, capability: &Capability) -> bool {
        self.descriptors
            .get(plugin_id)
            .map(|d| d.capabilities.contains(capability))
            .unwrap_or(false)
    }

    /// Get the descriptor for a plugin.
    pub fn descriptor(&self, plugin_id: &str) -> Option<&PluginDescriptor> {
        self.descriptors.get(plugin_id)
    }

    /// Get all registered plugin descriptors.
    pub fn all_descriptors(&self) -> Vec<&PluginDescriptor> {
        self.descriptors.values().collect()
    }

    /// Get the total number of registered plugins.
    pub fn plugin_count(&self) -> usize {
        self.descriptors.len()
    }

    /// Get the total number of registered capabilities (distinct).
    pub fn capability_count(&self) -> usize {
        self.capabilities.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contract::ProviderProfile;

    fn fs_descriptor() -> PluginDescriptor {
        PluginDescriptor {
            id: "filesystem".into(),
            name: "Filesystem Plugin".into(),
            version: "1.0.0".into(),
            author: "FRIDAY".into(),
            capabilities: vec![Capability::ReadFile, Capability::WriteFile, Capability::ListDirectory],
            permissions: vec!["fs.read".into(), "fs.write".into()],
            profile: ProviderProfile::default_for("filesystem"),
        }
    }

    fn browser_descriptor() -> PluginDescriptor {
        PluginDescriptor {
            id: "browser".into(),
            name: "Browser Plugin".into(),
            version: "1.0.0".into(),
            author: "FRIDAY".into(),
            capabilities: vec![Capability::OpenBrowser, Capability::SearchWeb],
            permissions: vec!["network".into()],
            profile: ProviderProfile {
                plugin_id: "browser".into(),
                deterministic: false,
                offline_capable: false,
                streaming: true,
                has_side_effects: false,
                requires_network: true,
                max_concurrency: 2,
                preferred_contexts: vec!["network".into(), "web".into()],
            },
        }
    }

    #[test]
    fn register_and_query() {
        let mut reg = CapabilityRegistry::new();
        reg.register(fs_descriptor());

        let plugins = reg.plugins_for(&Capability::ReadFile);
        assert_eq!(plugins, vec!["filesystem"]);

        assert!(reg.plugin_supports("filesystem", &Capability::ReadFile));
        assert!(!reg.plugin_supports("filesystem", &Capability::SearchWeb));
    }

    #[test]
    fn multiple_plugins_same_capability() {
        let mut reg = CapabilityRegistry::new();

        let mut fs2 = fs_descriptor();
        fs2.id = "filesystem-alt".into();
        reg.register(fs2);
        reg.register(browser_descriptor());

        reg.register(PluginDescriptor {
            id: "s3-fs".into(),
            name: "S3 Filesystem".into(),
            version: "1.0.0".into(),
            author: "FRIDAY".into(),
            capabilities: vec![Capability::ReadFile, Capability::WriteFile],
            permissions: vec!["network".into(), "fs.write".into()],
            profile: ProviderProfile {
                plugin_id: "s3-fs".into(),
                deterministic: true,
                offline_capable: false,
                streaming: false,
                has_side_effects: true,
                requires_network: true,
                max_concurrency: 2,
                preferred_contexts: vec!["remote".into(), "cloud".into()],
            },
        });

        let readers = reg.plugins_for(&Capability::ReadFile);
        assert_eq!(readers.len(), 2);
        assert!(readers.contains(&"filesystem-alt"));
        assert!(readers.contains(&"s3-fs"));
    }

    #[test]
    fn unregister_removes_plugin() {
        let mut reg = CapabilityRegistry::new();
        reg.register(fs_descriptor());
        reg.register(browser_descriptor());

        assert_eq!(reg.plugin_count(), 2);
        reg.unregister("filesystem");

        assert_eq!(reg.plugin_count(), 1);
        assert!(reg.plugins_for(&Capability::ReadFile).is_empty());
        assert_eq!(reg.plugins_for(&Capability::OpenBrowser).len(), 1);
    }

    #[test]
    fn descriptor_lookup() {
        let mut reg = CapabilityRegistry::new();
        reg.register(fs_descriptor());

        let desc = reg.descriptor("filesystem").unwrap();
        assert_eq!(desc.version, "1.0.0");
        assert_eq!(desc.capabilities.len(), 3);

        assert!(reg.descriptor("nonexistent").is_none());
    }

    #[test]
    fn all_descriptors_and_counts() {
        let mut reg = CapabilityRegistry::new();
        assert_eq!(reg.plugin_count(), 0);
        assert_eq!(reg.capability_count(), 0);

        reg.register(fs_descriptor());
        assert_eq!(reg.plugin_count(), 1);
        assert_eq!(reg.capability_count(), 3);

        reg.register(browser_descriptor());
        assert_eq!(reg.plugin_count(), 2);
    }
}
