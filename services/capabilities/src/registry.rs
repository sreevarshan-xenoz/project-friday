//! The capability registry: the planner's only view of what the platform can do.
//! Maps `provider` (a service name) to the `Capability` records it advertises,
//! and answers "who can do X, and in what order should I try them?"

use std::collections::HashMap;
use std::sync::RwLock;

use crate::graph::CapabilityGraph;
use crate::model::{Capability, Health};
use crate::ranking::score;

/// Thread-safe registry of provider capabilities.
pub struct CapabilityRegistry {
    providers: RwLock<HashMap<String, Vec<Capability>>>,
}

impl Default for CapabilityRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl CapabilityRegistry {
    pub fn new() -> Self {
        Self {
            providers: RwLock::new(HashMap::new()),
        }
    }

    /// Register a single capability under a provider.
    pub fn register(&self, provider: &str, cap: Capability) {
        self.providers
            .write()
            .unwrap()
            .entry(provider.to_string())
            .or_default()
            .push(cap);
    }

    /// Register a batch of capabilities under a provider.
    pub fn register_provider(&self, provider: &str, caps: Vec<Capability>) {
        self.providers
            .write()
            .unwrap()
            .insert(provider.to_string(), caps);
    }

    /// Remove a provider (and all its capabilities) entirely.
    pub fn deregister(&self, provider: &str) {
        self.providers.write().unwrap().remove(provider);
    }

    /// All `(provider, capability)` pairs claiming `capability_id`.
    pub fn providers_for(&self, capability_id: &str) -> Vec<(String, Capability)> {
        self.providers
            .read()
            .unwrap()
            .iter()
            .flat_map(|(p, caps)| {
                caps.iter()
                    .filter(|c| c.id == capability_id)
                    .map(move |c| (p.clone(), c.clone()))
            })
            .collect()
    }

    /// Providers for a capability, filtered to usable (available & not offline/
    /// disabled) and sorted by descending [`score`].
    pub fn ranked_providers_for(&self, capability_id: &str) -> Vec<(String, Capability)> {
        let mut list: Vec<(String, Capability)> = self
            .providers_for(capability_id)
            .into_iter()
            .filter(|(_, c)| c.available && c.health.available())
            .collect();
        list.sort_by(|a, b| {
            score(&b.1)
                .partial_cmp(&score(&a.1))
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        list
    }

    /// Is at least one healthy provider available for this capability?
    pub fn available(&self, capability_id: &str) -> bool {
        self.ranked_providers_for(capability_id)
            .iter()
            .any(|(_, c)| c.available)
    }

    /// Negotiation write-back: update a provider capability's health + availability.
    pub fn set_health(&self, provider: &str, capability_id: &str, health: Health) {
        if let Some(caps) = self.providers.write().unwrap().get_mut(provider) {
            if let Some(cap) = caps.iter_mut().find(|c| c.id == capability_id) {
                cap.health = health;
                cap.available = health.available();
            }
        }
    }

    /// Every `(provider, capability)` pair.
    pub fn all(&self) -> Vec<(String, Capability)> {
        self.providers
            .read()
            .unwrap()
            .iter()
            .flat_map(|(p, caps)| caps.iter().map(move |c| (p.clone(), c.clone())))
            .collect()
    }

    /// Every capability (across all providers), de-duplicated by id but keeping
    /// the highest-scoring provider's record.
    pub fn capabilities(&self) -> Vec<Capability> {
        self.all().into_iter().map(|(_, c)| c).collect()
    }

    /// Build the hierarchical capability graph for LLM traversal.
    pub fn graph(&self) -> CapabilityGraph {
        CapabilityGraph::from_capabilities(&self.capabilities())
    }
}
