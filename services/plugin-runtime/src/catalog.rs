use std::collections::HashMap;

use crate::contract::{Capability, CapabilityCategory, CapabilityDescriptor, SafetyLevel};

/// Capability Catalog — defines what capabilities exist and their metadata.
///
/// A catalog answers "what is ReadFile?" rather than "who provides ReadFile?".
/// Plugins register implementations against catalog entries. The catalog
/// itself never knows about plugins.
pub struct CapabilityCatalog {
    entries: HashMap<Capability, CapabilityDescriptor>,
}

impl Default for CapabilityCatalog {
    fn default() -> Self {
        Self::with_defaults()
    }
}

impl CapabilityCatalog {
    pub fn new() -> Self {
        Self {
            entries: HashMap::new(),
        }
    }

    /// Create a catalog pre-populated with all standard capability descriptors.
    pub fn with_defaults() -> Self {
        let mut c = Self::new();
        c.register(descriptor(
            Capability::ReadFile,
            "1.0.0",
            CapabilityCategory::Filesystem,
            "Read the contents of a file from the local filesystem",
            vec!["fs.read"],
            0.05,
            5,
            SafetyLevel::Safe,
            vec!["read", "local"],
        ));
        c.register(descriptor(
            Capability::WriteFile,
            "1.0.0",
            CapabilityCategory::Filesystem,
            "Write content to a file on the local filesystem",
            vec!["fs.write"],
            0.10,
            10,
            SafetyLevel::Moderate,
            vec!["write", "local"],
        ));
        c.register(descriptor(
            Capability::DeleteFile,
            "1.0.0",
            CapabilityCategory::Filesystem,
            "Delete a file from the local filesystem",
            vec!["fs.write"],
            0.15,
            15,
            SafetyLevel::Sensitive,
            vec!["write", "delete", "local"],
        ));
        c.register(descriptor(
            Capability::ListDirectory,
            "1.0.0",
            CapabilityCategory::Filesystem,
            "List the contents of a directory",
            vec!["fs.read"],
            0.05,
            5,
            SafetyLevel::Safe,
            vec!["read", "local"],
        ));
        c.register(descriptor(
            Capability::ExecuteCommand,
            "1.0.0",
            CapabilityCategory::System,
            "Execute a shell command or program",
            vec!["system.exec"],
            0.30,
            50,
            SafetyLevel::Critical,
            vec!["run", "system"],
        ));
        c.register(descriptor(
            Capability::OpenBrowser,
            "1.0.0",
            CapabilityCategory::Browser,
            "Open a web browser to a specified URL",
            vec!["network"],
            0.10,
            200,
            SafetyLevel::Moderate,
            vec!["network", "ui"],
        ));
        c.register(descriptor(
            Capability::SearchWeb,
            "1.0.0",
            CapabilityCategory::Browser,
            "Search the web for information",
            vec!["network"],
            0.15,
            500,
            SafetyLevel::Safe,
            vec!["network", "search"],
        ));
        c.register(descriptor(
            Capability::SearchMemory,
            "1.0.0",
            CapabilityCategory::Knowledge,
            "Search the cognitive memory for past experiences",
            vec!["memory.read"],
            0.05,
            2,
            SafetyLevel::Safe,
            vec!["memory", "search"],
        ));
        c
    }

    /// Register a capability descriptor in the catalog.
    pub fn register(&mut self, descriptor: CapabilityDescriptor) {
        let cap = descriptor.capability.clone();
        self.entries.insert(cap, descriptor);
    }

    /// Get the descriptor for a capability.
    pub fn get(&self, capability: &Capability) -> Option<&CapabilityDescriptor> {
        self.entries.get(capability)
    }

    /// Check if a capability is defined in the catalog.
    pub fn contains(&self, capability: &Capability) -> bool {
        self.entries.contains_key(capability)
    }

    /// Get all registered capability descriptors.
    pub fn all(&self) -> Vec<&CapabilityDescriptor> {
        self.entries.values().collect()
    }

    /// Get all capabilities in a given category.
    pub fn by_category(&self, category: &CapabilityCategory) -> Vec<&CapabilityDescriptor> {
        self.entries
            .values()
            .filter(|d| d.category == *category)
            .collect()
    }

    /// Get the number of registered capabilities.
    pub fn count(&self) -> usize {
        self.entries.len()
    }
}

#[allow(clippy::too_many_arguments)]
fn descriptor(
    capability: Capability,
    version: &str,
    category: CapabilityCategory,
    description: &str,
    permissions: Vec<&str>,
    estimated_cost: f64,
    expected_latency_ms: u64,
    safety_level: SafetyLevel,
    tags: Vec<&str>,
) -> CapabilityDescriptor {
    CapabilityDescriptor {
        capability,
        version: version.to_string(),
        category,
        description: description.to_string(),
        required_permissions: permissions.into_iter().map(|s| s.to_string()).collect(),
        estimated_cost,
        expected_latency_ms,
        safety_level,
        tags: tags.into_iter().map(|s| s.to_string()).collect(),
    }
}

#[allow(clippy::too_many_arguments)]
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_catalog_contains_standard_capabilities() {
        let catalog = CapabilityCatalog::with_defaults();
        assert!(catalog.contains(&Capability::ReadFile));
        assert!(catalog.contains(&Capability::WriteFile));
        assert!(catalog.contains(&Capability::ListDirectory));
        assert!(catalog.contains(&Capability::SearchWeb));
        assert!(catalog.contains(&Capability::ExecuteCommand));
        assert_eq!(catalog.count(), 8);
    }

    #[test]
    fn catalog_returns_descriptor_metadata() {
        let catalog = CapabilityCatalog::with_defaults();
        let desc = catalog.get(&Capability::ReadFile).unwrap();
        assert_eq!(desc.category, CapabilityCategory::Filesystem);
        assert_eq!(desc.safety_level, SafetyLevel::Safe);
        assert!(desc.estimated_cost < 0.1);
        assert!(desc.expected_latency_ms < 10);
    }

    #[test]
    fn catalog_groups_by_category() {
        let catalog = CapabilityCatalog::with_defaults();
        let fs = catalog.by_category(&CapabilityCategory::Filesystem);
        assert_eq!(fs.len(), 4);
        assert!(fs.iter().any(|d| d.capability == Capability::ReadFile));

        let browser = catalog.by_category(&CapabilityCategory::Browser);
        assert_eq!(browser.len(), 2);
    }

    #[test]
    fn custom_capability_not_in_default_catalog() {
        let catalog = CapabilityCatalog::with_defaults();
        assert!(!catalog.contains(&Capability::Custom("unknown".into())));
    }

    #[test]
    fn register_custom_capability() {
        let mut catalog = CapabilityCatalog::new();
        catalog.register(CapabilityDescriptor {
            capability: Capability::Custom("vision.scan".into()),
            version: "0.1.0".into(),
            category: CapabilityCategory::Custom("Vision".into()),
            description: "Scan an image for text".into(),
            required_permissions: vec!["camera".into()],
            estimated_cost: 0.50,
            expected_latency_ms: 1000,
            safety_level: SafetyLevel::Moderate,
            tags: vec!["vision".into(), "ml".into()],
        });

        assert!(catalog.contains(&Capability::Custom("vision.scan".into())));
        assert_eq!(catalog.count(), 1);
    }

    #[test]
    fn execute_command_is_critical() {
        let catalog = CapabilityCatalog::with_defaults();
        let desc = catalog.get(&Capability::ExecuteCommand).unwrap();
        assert_eq!(desc.safety_level, SafetyLevel::Critical);
        assert!(desc.estimated_cost > 0.2);
    }
}
