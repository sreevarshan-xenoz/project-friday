//! Capability-registry resolver for `action` → `action_class`.
//!
//! The planner's taxonomy can change without changing what an action *is*; the
//! capability registry is closer to executable reality, so it is the source of the
//! class. If resolution fails, `None` is returned and the caller must **not** invent
//! a class (per 8B design): the raw action is kept in provenance and is not promoted
//! into a broadly reusable lesson.

use std::sync::Arc;

use friday_capabilities::registry::CapabilityRegistry;

/// Resolve a raw action string to its canonical `action_class`.
pub trait CapabilityResolver: Send + Sync {
    fn resolve(&self, action: &str) -> Option<String>;
}

/// Hermetic default: the action string itself (lower-cased) is the class. Used by
/// tests and when no registry is wired.
pub struct IdentityResolver;

impl CapabilityResolver for IdentityResolver {
    fn resolve(&self, action: &str) -> Option<String> {
        Some(action.to_lowercase())
    }
}

/// Production resolver backed by the `CapabilityRegistry`. Matches the action against
/// capability `id` / `name` / `group` / `actions` (case-insensitive) and returns the
/// canonical capability `id` as the class. Returns `None` when nothing matches.
pub struct RegistryCapabilityResolver {
    registry: Arc<CapabilityRegistry>,
}

impl RegistryCapabilityResolver {
    pub fn new(registry: Arc<CapabilityRegistry>) -> Self {
        Self { registry }
    }
}

impl CapabilityResolver for RegistryCapabilityResolver {
    fn resolve(&self, action: &str) -> Option<String> {
        let a = action.to_lowercase();
        self.registry
            .capabilities()
            .iter()
            .find(|c| {
                c.id.to_lowercase() == a
                    || c.name.to_lowercase() == a
                    || c.group.to_lowercase() == a
                    || c.actions.iter().any(|act| act.to_lowercase() == a)
            })
            .map(|c| c.id.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use friday_capabilities::registry::CapabilityRegistry;
    use friday_capabilities::Capability;

    #[test]
    fn identity_resolver_lowercases() {
        assert_eq!(
            IdentityResolver.resolve("Browser.Search"),
            Some("browser.search".into())
        );
    }

    #[test]
    fn registry_resolver_maps_action_to_capability_id() {
        let reg = CapabilityRegistry::new();
        let cap = Capability::new("web.search", "Web Search", "web")
            .with_actions(&["browser.search", "lookup"]);
        reg.register_provider("search-svc", vec![cap]);

        let resolver = RegistryCapabilityResolver::new(std::sync::Arc::new(reg));
        // Matches by action string → canonical capability id.
        assert_eq!(
            resolver.resolve("browser.search"),
            Some("web.search".into())
        );
        assert_eq!(resolver.resolve("WEB.SEARCH"), Some("web.search".into()));
        // Unknown action → no invented class.
        assert_eq!(resolver.resolve("totally-unknown-action"), None);
    }
}
