//! Service Orchestrator (M4.6.5).
//!
//! Complements the Capability Registry (which answers *what exists*) by tracking
//! *what is happening right now*: each service moves through a lifecycle
//! (`Created → Starting → Healthy → Busy → Recovering → Offline → Disabled`),
//! is kept alive by health probes, healed by a per-service `RecoveryPolicy`, and
//! emits telemetry. The planner never polls services directly — it consults this
//! orchestrator (and the registry it keeps in sync).
//!
//! Distinct from the execution [`crate::registry::CapabilityRegistry`]: the
//! registry owns *capability* facts; the orchestrator owns *service* runtime
//! state.

use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use std::time::Duration;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::model::Health;
use crate::registry::CapabilityRegistry;

/// Live lifecycle state of a whole service (not a single execution).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ServiceState {
    Created,
    Starting,
    Healthy,
    Busy,
    Recovering,
    Offline,
    Disabled,
}

impl ServiceState {
    /// A service is usable unless it is `Offline` or `Disabled`.
    pub fn available(&self) -> bool {
        !matches!(self, ServiceState::Offline | ServiceState::Disabled)
    }
}

/// Snapshot of a service's current state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceStatus {
    pub state: ServiceState,
    pub since: DateTime<Utc>,
    pub health: Health,
    pub message: String,
}

/// Per-cycle telemetry emitted by a service. CPU/memory are populated by richer
/// probes when available; the orchestrator always measures latency and counts
/// requests/failures (feeding M4.8/M10).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Telemetry {
    pub service: String,
    pub heartbeat_at: DateTime<Utc>,
    pub latency_ms: u64,
    pub cpu: f32,
    pub memory_mb: u64,
    pub requests: u64,
    pub failures: u64,
    pub errors: u64,
}

/// Recovery step attempted when a service is `Offline`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RecoveryStep {
    /// Re-establish a dropped connection (e.g. reconnect the Windows pipe).
    Reconnect,
    /// Restart an internal session (e.g. a browser tab/session).
    RestartSession,
    /// Restart the backing host process (e.g. `windows-host`).
    RestartHost,
    /// Verify the executable still exists/is runnable (e.g. `git --version`).
    CheckExecutable,
    /// Generic restart.
    Restart,
    /// Cannot auto-heal; escalate to a human / admin.
    Escalate,
}

/// Per-service recovery strategy. Different services deserve different steps:
/// a Browser restarts its session, Git just checks the executable, the Windows
/// bridge reconnects the pipe then restarts the host, then escalates.
#[async_trait]
pub trait RecoveryPolicy: Send + Sync {
    /// Ordered steps to try, best first.
    fn steps(&self) -> Vec<RecoveryStep>;
    /// Perform one step; return `true` if the step succeeded.
    async fn execute(&self, step: RecoveryStep) -> bool;
}

/// Service-level liveness probe. Returns the service's current [`Health`]. This
/// is intentionally richer than "is the process alive" — e.g. Filesystem creates
/// and deletes a temp file, Git runs `git --version`, the Windows bridge pings
/// its named pipe, Terminal echoes a sentinel.
#[async_trait]
pub trait ServiceProbe: Send + Sync {
    async fn probe(&self) -> Health;
}

/// Reusable policy built from a step list + an executor closure. Real services
/// implement [`RecoveryPolicy`] directly when steps need service-specific logic.
pub struct StaticRecoveryPolicy {
    steps: Vec<RecoveryStep>,
    exec: Arc<dyn Fn(RecoveryStep) -> bool + Send + Sync>,
}

impl StaticRecoveryPolicy {
    pub fn new(
        steps: Vec<RecoveryStep>,
        exec: Arc<dyn Fn(RecoveryStep) -> bool + Send + Sync>,
    ) -> Self {
        Self { steps, exec }
    }
}

#[async_trait]
impl RecoveryPolicy for StaticRecoveryPolicy {
    fn steps(&self) -> Vec<RecoveryStep> {
        self.steps.clone()
    }

    async fn execute(&self, step: RecoveryStep) -> bool {
        (self.exec)(step)
    }
}

#[derive(Clone)]
struct ServiceEntry {
    probe: Arc<dyn ServiceProbe>,
    policy: Arc<dyn RecoveryPolicy>,
}

/// The orchestrator: owns service lifecycle, probes, recovery, and telemetry,
/// and keeps the [`CapabilityRegistry`] availability in sync with reality.
pub struct ServiceOrchestrator {
    registry: Arc<CapabilityRegistry>,
    entries: RwLock<HashMap<String, ServiceEntry>>,
    status: RwLock<HashMap<String, ServiceStatus>>,
    telemetry: RwLock<HashMap<String, Telemetry>>,
}

impl ServiceOrchestrator {
    pub fn new(registry: Arc<CapabilityRegistry>) -> Self {
        Self {
            registry,
            entries: RwLock::new(HashMap::new()),
            status: RwLock::new(HashMap::new()),
            telemetry: RwLock::new(HashMap::new()),
        }
    }

    /// Register a service (state `Created`).
    pub fn register<P, R>(&self, name: &str, probe: Arc<P>, policy: Arc<R>)
    where
        P: ServiceProbe + 'static,
        R: RecoveryPolicy + 'static,
    {
        self.entries.write().unwrap().insert(
            name.to_string(),
            ServiceEntry {
                probe: probe as Arc<dyn ServiceProbe>,
                policy: policy as Arc<dyn RecoveryPolicy>,
            },
        );
        self.set_status(name, ServiceState::Created, Health::Healthy, "registered");
    }

    /// Mark a service as `Starting` (call once at boot).
    pub fn start(&self, name: &str) {
        if self.entries.read().unwrap().contains_key(name) {
            self.set_status(name, ServiceState::Starting, Health::Healthy, "starting");
        }
    }

    fn set_status(&self, name: &str, state: ServiceState, health: Health, msg: &str) {
        self.status.write().unwrap().insert(
            name.to_string(),
            ServiceStatus {
                state,
                since: Utc::now(),
                health,
                message: msg.to_string(),
            },
        );
    }

    fn update_status(
        &self,
        name: &str,
        state: ServiceState,
        health: Health,
        latency: u64,
        failed: bool,
    ) {
        self.set_status(name, state, health, &status_message(state));
        let mut tel = self
            .telemetry
            .read()
            .unwrap()
            .get(name)
            .cloned()
            .unwrap_or_else(|| Telemetry {
                service: name.to_string(),
                ..Default::default()
            });
        tel.heartbeat_at = Utc::now();
        tel.latency_ms = latency;
        tel.requests += 1;
        if failed {
            tel.failures += 1;
            tel.errors += 1;
        }
        self.telemetry
            .write()
            .unwrap()
            .insert(name.to_string(), tel);
    }

    /// Run one negotiation pass over all services: probe, map to lifecycle,
    /// recover anything `Offline`, record telemetry, sync the registry.
    pub async fn tick(&self) {
        let names: Vec<String> = self.entries.read().unwrap().keys().cloned().collect();
        for name in names {
            let entry = self.entries.read().unwrap().get(&name).cloned();
            let Some(entry) = entry else { continue };
            let start = std::time::Instant::now();
            let health = entry.probe.probe().await;
            let latency = start.elapsed().as_millis() as u64;

            let (new_state, failed) = match health {
                Health::Healthy => (ServiceState::Healthy, false),
                Health::Busy => (ServiceState::Busy, false),
                Health::Recovering => (ServiceState::Recovering, false),
                Health::Disabled => (ServiceState::Disabled, false),
                Health::Offline => {
                    self.set_status(
                        &name,
                        ServiceState::Recovering,
                        Health::Recovering,
                        "recovering",
                    );
                    let recovered = self.try_recover(&name, &entry.policy).await;
                    if recovered {
                        (ServiceState::Healthy, false)
                    } else {
                        (ServiceState::Offline, true)
                    }
                }
            };
            self.update_status(&name, new_state, health, latency, failed);
        }
        self.sync_to_registry();
    }

    async fn try_recover(&self, name: &str, policy: &Arc<dyn RecoveryPolicy>) -> bool {
        for step in policy.steps() {
            if !policy.execute(step).await {
                continue;
            }
            // Re-probe after the step.
            let entry = self.entries.read().unwrap().get(name).cloned();
            if let Some(e) = entry {
                let h = e.probe.probe().await;
                if matches!(h, Health::Healthy | Health::Busy) {
                    return true;
                }
            }
        }
        false
    }

    /// Push orchestrator state into the registry so the planner's capability
    /// queries reflect live availability.
    fn sync_to_registry(&self) {
        for (name, st) in self.status.read().unwrap().iter() {
            let h = match st.state {
                ServiceState::Healthy | ServiceState::Busy => Health::Healthy,
                ServiceState::Recovering => Health::Recovering,
                ServiceState::Offline => Health::Offline,
                ServiceState::Disabled => Health::Disabled,
                _ => continue,
            };
            self.registry.set_provider_health(name, h);
        }
    }

    /// State of a service right now.
    pub fn state(&self, name: &str) -> Option<ServiceState> {
        self.status.read().unwrap().get(name).map(|s| s.state)
    }

    pub fn status(&self, name: &str) -> Option<ServiceStatus> {
        self.status.read().unwrap().get(name).cloned()
    }

    pub fn all_status(&self) -> Vec<(String, ServiceStatus)> {
        self.status
            .read()
            .unwrap()
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect()
    }

    pub fn telemetry(&self, name: &str) -> Option<Telemetry> {
        self.telemetry.read().unwrap().get(name).cloned()
    }

    /// Is the service currently usable?
    pub fn available(&self, name: &str) -> bool {
        self.state(name).map(|s| s.available()).unwrap_or(false)
    }

    /// Spawn the background negotiation loop.
    pub fn start_loop(self: Arc<Self>, interval: Duration) -> tokio::task::JoinHandle<()> {
        tokio::spawn(async move {
            loop {
                self.tick().await;
                tokio::time::sleep(interval).await;
            }
        })
    }
}

fn status_message(state: ServiceState) -> String {
    match state {
        ServiceState::Created => "registered, not started",
        ServiceState::Starting => "starting up",
        ServiceState::Healthy => "healthy",
        ServiceState::Busy => "busy",
        ServiceState::Recovering => "recovering",
        ServiceState::Offline => "offline",
        ServiceState::Disabled => "disabled",
    }
    .to_string()
}
