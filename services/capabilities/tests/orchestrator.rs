//! Orchestrator + Cost Estimator tests.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;

use friday_capabilities::model::Capability;
use friday_capabilities::orchestrator::{
    RecoveryPolicy, RecoveryStep, ServiceOrchestrator, ServiceProbe, ServiceState,
    StaticRecoveryPolicy,
};
use friday_capabilities::registry::CapabilityRegistry;
use friday_capabilities::Health;
use friday_capabilities::{CostEstimator, PlanCost};

struct ConstProbe(Health);

#[async_trait]
impl ServiceProbe for ConstProbe {
    async fn probe(&self) -> Health {
        self.0
    }
}

struct NoopPolicy;

#[async_trait]
impl RecoveryPolicy for NoopPolicy {
    fn steps(&self) -> Vec<RecoveryStep> {
        vec![]
    }
    async fn execute(&self, _: RecoveryStep) -> bool {
        false
    }
}

#[tokio::test]
async fn healthy_service_stays_healthy() {
    let reg = Arc::new(CapabilityRegistry::new());
    let orch = Arc::new(ServiceOrchestrator::new(reg));
    orch.register(
        "filesystem",
        Arc::new(ConstProbe(Health::Healthy)),
        Arc::new(NoopPolicy),
    );
    orch.start("filesystem");
    orch.tick().await;
    assert_eq!(orch.state("filesystem"), Some(ServiceState::Healthy));
    assert!(orch.available("filesystem"));
}

#[tokio::test]
async fn busy_service_is_still_available() {
    let reg = Arc::new(CapabilityRegistry::new());
    let orch = Arc::new(ServiceOrchestrator::new(reg));
    orch.register(
        "terminal",
        Arc::new(ConstProbe(Health::Busy)),
        Arc::new(NoopPolicy),
    );
    orch.tick().await;
    assert_eq!(orch.state("terminal"), Some(ServiceState::Busy));
    assert!(orch.available("terminal"));
}

#[tokio::test]
async fn offline_service_recovers_via_policy() {
    let reg = Arc::new(CapabilityRegistry::new());
    let orch = Arc::new(ServiceOrchestrator::new(reg));
    let healed = Arc::new(AtomicBool::new(false));

    struct FlipProbe(Arc<AtomicBool>);
    #[async_trait]
    impl ServiceProbe for FlipProbe {
        async fn probe(&self) -> Health {
            if self.0.load(Ordering::SeqCst) {
                Health::Healthy
            } else {
                Health::Offline
            }
        }
    }

    let probe_heal = healed.clone();
    let pol_heal = healed.clone();
    let policy = StaticRecoveryPolicy::new(
        vec![RecoveryStep::Restart],
        Arc::new(move |step| {
            if step == RecoveryStep::Restart {
                pol_heal.store(true, Ordering::SeqCst);
                true
            } else {
                false
            }
        }),
    );

    orch.register("browser", Arc::new(FlipProbe(probe_heal)), Arc::new(policy));
    orch.tick().await;
    assert_eq!(orch.state("browser"), Some(ServiceState::Healthy));
    assert!(healed.load(Ordering::SeqCst), "recovery step must have run");
}

#[tokio::test]
async fn offline_without_recovery_stays_offline() {
    let reg = Arc::new(CapabilityRegistry::new());
    let orch = Arc::new(ServiceOrchestrator::new(reg));
    orch.register(
        "git",
        Arc::new(ConstProbe(Health::Offline)),
        Arc::new(NoopPolicy),
    );
    orch.tick().await;
    assert_eq!(orch.state("git"), Some(ServiceState::Offline));
    assert!(!orch.available("git"));
    let tel = orch.telemetry("git").unwrap();
    assert_eq!(tel.failures, 1);
}

#[tokio::test]
async fn orchestrator_syncs_registry_availability() {
    let reg = Arc::new(CapabilityRegistry::new());
    reg.register(
        "windows-host",
        Capability::new("windows.open", "Open", "Windows"),
    );
    let orch = Arc::new(ServiceOrchestrator::new(reg.clone()));
    orch.register(
        "windows-host",
        Arc::new(ConstProbe(Health::Offline)),
        Arc::new(NoopPolicy),
    );
    orch.tick().await;
    assert!(
        !reg.available("windows.open"),
        "registry must reflect offline"
    );
}

#[tokio::test]
async fn background_loop_runs() {
    let reg = Arc::new(CapabilityRegistry::new());
    let orch = Arc::new(ServiceOrchestrator::new(reg));
    orch.register(
        "filesystem",
        Arc::new(ConstProbe(Health::Healthy)),
        Arc::new(NoopPolicy),
    );
    let loop_orch = orch.clone();
    let handle = loop_orch.start_loop(Duration::from_millis(5));
    tokio::time::sleep(Duration::from_millis(20)).await;
    handle.abort();
    assert_eq!(orch.state("filesystem"), Some(ServiceState::Healthy));
}

#[test]
fn cost_estimate_combines_steps() {
    let steps = vec![
        Capability::new("windows.open", "Open", "Windows")
            .with_latency(30)
            .with_reliability(0.9),
        Capability::new("windows.type", "Type", "Windows")
            .with_latency(20)
            .with_reliability(0.95),
    ];
    let c: PlanCost = CostEstimator::estimate(&steps);
    assert_eq!(c.estimated_ms, 50);
    assert!((c.expected_success - 0.855).abs() < 1e-9);
    assert!((c.risk - 0.145).abs() < 1e-9);
    assert_eq!(c.token_cost, 200);
}

#[test]
fn empty_plan_cost_is_zero() {
    let c = CostEstimator::estimate(&[]);
    assert_eq!(c.estimated_ms, 0);
    assert_eq!(c.expected_success, 0.0);
    assert_eq!(c.token_cost, 0);
}
