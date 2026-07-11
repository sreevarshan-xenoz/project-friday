//! Health monitoring + self-healing.
//!
//! The [`Monitor`] periodically probes every provider capability, writes the
//! resulting [`Health`] back into the registry, and — on `Offline` — invokes an
//! optional recovery hook (e.g. ask the runtime to restart the service). This is
//! the basis for autonomous recovery: a crashed service is avoided by the
//! planner until it heals.

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;

use crate::model::{Capability, Health};
use crate::registry::CapabilityRegistry;

/// Probes the live state of one provider capability. Implemented per service
/// (e.g. ping the Windows named pipe, check the Git binary). Returns the
/// observed [`Health`].
#[async_trait]
pub trait HealthProbe: Send + Sync {
    async fn probe(&self, provider: &str, cap: &Capability) -> Health;
}

/// Recovery callback invoked with `(provider, capability_id)` when a capability
/// is found `Offline`, so the runtime can restart/heal the service.
pub type RecoveryFn = Arc<dyn Fn(&str, &str) + Send + Sync>;

/// Periodic health checker + self-healing coordinator.
pub struct Monitor {
    registry: Arc<CapabilityRegistry>,
    probe: Arc<dyn HealthProbe>,
    interval: Duration,
    recovery: Option<RecoveryFn>,
}

impl Monitor {
    pub fn new(
        registry: Arc<CapabilityRegistry>,
        probe: Arc<dyn HealthProbe>,
        interval: Duration,
    ) -> Self {
        Self {
            registry,
            probe,
            interval,
            recovery: None,
        }
    }

    /// Set a callback invoked with `(provider, capability_id)` when a capability
    /// is found `Offline`, so the runtime can restart/heal the service.
    pub fn with_recovery(mut self, f: RecoveryFn) -> Self {
        self.recovery = Some(f);
        self
    }

    /// Run one negotiation pass: probe all capabilities, write health back, fire
    /// recovery for anything offline.
    pub async fn tick(&self) {
        let snapshot = self.registry.all();
        for (provider, cap) in snapshot {
            let health = self.probe.probe(&provider, &cap).await;
            self.registry.set_health(&provider, &cap.id, health);
            if health == Health::Offline {
                if let Some(r) = &self.recovery {
                    r(&provider, &cap.id);
                }
            }
        }
    }

    /// Spawn the background loop (uses the async runtime's scheduler).
    pub fn start(self) -> tokio::task::JoinHandle<()> {
        tokio::spawn(async move {
            loop {
                self.tick().await;
                tokio::time::sleep(self.interval).await;
            }
        })
    }
}
