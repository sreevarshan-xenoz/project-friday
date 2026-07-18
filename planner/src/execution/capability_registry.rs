//! Capability Registry — the single resolution point from a canonical
//! capability id to an executable binding.
//!
//! This is deliberately *not* a hardcoded map. Every capability (whether
//! backed by a native [`ActionAdapter`], an MCP server, or a plugin) registers
//! an [`ExecutionBinding`] here. The supervisor resolves capabilities through
//! this registry and never knows where the implementation lives — which is
//! exactly what keeps MCP / plugin loading / dynamic tool discovery free of a
//! future refactor.
//!
//! Risk lives on the capability metadata ([`Risk`]); the [`PermissionEngine`]
//! (in `friday-actions`) consults it when deciding policy, but registration
//! here is policy-agnostic.

use std::collections::HashMap;
use std::sync::RwLock;

/// Re-export the workspace-wide resolution contract so existing importers of
/// `crate::execution::capability_registry::{Risk, CapabilitySpec, ...}` keep
/// working. These types are defined in `friday-capabilities::spec`.
pub use friday_capabilities::spec::{
    CapabilityHealth, CapabilitySource, CapabilitySpec, ExecutionBinding, Risk,
};

/// A capability plus its discovery score (higher = more relevant to the goal).
/// Returned by [`CapabilityRegistry::search`].
pub struct RankedCapability {
    pub spec: CapabilitySpec,
    pub score: f64,
}

/// The registry that maps capability id -> binding.
///
/// Resolution is O(1) and the registry is safe for concurrent use by the
/// scheduler's worker pool. Registration is expected at startup (native
/// adapters) and at runtime (MCP/plugins later) — both go through the same
/// [`CapabilityRegistry::register`] call.
#[derive(Default)]
pub struct CapabilityRegistry {
    bindings: RwLock<HashMap<String, CapabilitySpec>>,
    /// Live discovery-health per capability id (MCP/remote tools can go
    /// offline). Read separately so registration stays cheap; [`CapabilityRegistry::search`]
    /// skips [`CapabilityHealth::Offline`] capabilities.
    health: RwLock<HashMap<String, CapabilityHealth>>,
}

impl CapabilityRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register (or overwrite) a capability binding. Newly registered
    /// capabilities start [`CapabilityHealth::Healthy`].
    pub fn register(&self, spec: CapabilitySpec) {
        let id = spec.id.clone();
        self.bindings
            .write()
            .expect("capability registry poisoned")
            .insert(id.clone(), spec);
        self.health
            .write()
            .expect("capability registry poisoned")
            .entry(id)
            .or_insert(CapabilityHealth::Healthy);
    }

    /// Register many capabilities at once (e.g. a native adapter's full set).
    pub fn register_all(&self, specs: Vec<CapabilitySpec>) {
        let mut bguard = self.bindings.write().expect("capability registry poisoned");
        let mut hguard = self.health.write().expect("capability registry poisoned");
        for s in specs {
            hguard.entry(s.id.clone()).or_insert(CapabilityHealth::Healthy);
            bguard.insert(s.id.clone(), s);
        }
    }

    /// Update the live discovery-health of a capability (used by MCP/remote
    /// polling). Offline capabilities are excluded from [`CapabilityRegistry::search`].
    pub fn set_health(&self, capability: &str, health: CapabilityHealth) {
        *self
            .health
            .write()
            .expect("capability registry poisoned")
            .entry(capability.to_string())
            .or_insert(CapabilityHealth::Healthy) = health;
    }

    /// Current discovery-health of a capability (defaults to
    /// [`CapabilityHealth::Healthy`] for unrecorded ids).
    pub fn health_of(&self, capability: &str) -> CapabilityHealth {
        self.health
            .read()
            .expect("capability registry poisoned")
            .get(capability)
            .copied()
            .unwrap_or(CapabilityHealth::Healthy)
    }

    /// Resolve a canonical capability id to its executable binding.
    pub fn resolve(&self, capability: &str) -> Option<ExecutionBinding> {
        self.bindings
            .read()
            .expect("capability registry poisoned")
            .get(capability)
            .map(|s| s.binding.clone())
    }

    /// The risk classification for a capability (defaults to [`Risk::Safe`] if
    /// the capability is unknown — fail-open is acceptable for a missing
    /// binding because the runtime will reject unknown adapters anyway).
    pub fn risk_of(&self, capability: &str) -> Risk {
        self.bindings
            .read()
            .expect("capability registry poisoned")
            .get(capability)
            .map(|s| s.binding.risk)
            .unwrap_or(Risk::Safe)
    }

    pub fn known(&self) -> Vec<String> {
        self.bindings
            .read()
            .expect("capability registry poisoned")
            .keys()
            .cloned()
            .collect()
    }

    /// Capability discovery + ranking (M6.2 discovery).
    ///
    /// Given a free-text goal, returns the top-`limit` most relevant capabilities
    /// rather than the whole registry — which shortens planner prompts, cuts
    /// token usage, and improves model accuracy. The score blends:
    ///
    /// * **textual relevance** of the capability id/name/description to the goal
    ///   (token overlap + substring hits), and
    /// * **risk penalty** (high-risk capabilities are surfaced lower so the
    ///   planner does not over-reach into destructive actions).
    ///
    /// The formula is transparent and tunable; MCP/plugin capabilities register
    /// through the same `register` call and therefore appear here automatically.
    pub fn search(&self, goal: &str, limit: usize) -> Vec<RankedCapability> {
        let goal_lower = goal.to_lowercase();
        let goal_tokens: Vec<&str> = goal_lower
            .split(|c: char| !c.is_alphanumeric())
            .filter(|t| t.len() >= 3)
            .collect();
        let bindings = self.bindings.read().expect("capability registry poisoned");
        let health = self.health.read().expect("capability registry poisoned");
        let mut scored: Vec<RankedCapability> = bindings
            .values()
            .filter(|spec| {
                // Discovery ignores offline capabilities so planning never
                // wastes context on unavailable tools.
                health
                    .get(&spec.id)
                    .copied()
                    .unwrap_or(CapabilityHealth::Healthy)
                    .discoverable()
            })
            .map(|spec| {
                let score = score_capability(&goal_lower, &goal_tokens, spec);
                RankedCapability {
                    spec: spec.clone(),
                    score,
                }
            })
            .collect();
        scored.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        scored.truncate(limit);
        scored
    }
}

/// Transparent discovery score in 0..1. Combines lexical relevance with a risk
/// penalty; safe/read actions score highest.
fn score_capability(goal_lower: &str, goal_tokens: &[&str], spec: &CapabilitySpec) -> f64 {
    let text = format!("{} {} {}", spec.id, spec.name, spec.description).to_lowercase();
    // Token overlap (Jaccard-ish on the higher-count side).
    let mut hits = 0u32;
    for t in goal_tokens {
        if text.contains(t) {
            hits += 1;
        }
    }
    let relevance = if goal_tokens.is_empty() {
        0.5
    } else {
        (hits as f64 / goal_tokens.len() as f64).clamp(0.0, 1.0)
    };
    // Direct substring hits on the capability id are a strong signal.
    let id_lower = spec.id.to_lowercase();
    let substring_boost = if goal_lower.contains(&id_lower)
        || id_lower.split('.').any(|part| goal_lower.contains(part) && part.len() >= 3)
    {
        0.3
    } else {
        0.0
    };
    // Risk penalty: high-risk actions are surfaced lower so the planner does not
    // over-suggest destructive operations for an open-ended goal.
    let risk_penalty = match spec.binding.risk {
        Risk::Safe => 0.0,
        Risk::Medium => 0.1,
        Risk::High => 0.35,
    };
    (relevance * 0.7 + substring_boost).clamp(0.0, 1.0) - risk_penalty
}
