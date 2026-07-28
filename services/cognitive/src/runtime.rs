use crate::handler::PhaseResult;
use crate::phase::{CognitivePhase, TickId};
use crate::registry::PhaseRegistry;
use crate::workspace::{EngineId, GlobalWorkspace, Topic};
use std::time::{Duration, Instant};
use tokio::sync::watch;
use tracing::{debug, info, warn};

/// Configuration for the cognitive runtime.
#[derive(Debug, Clone)]
pub struct RuntimeConfig {
    /// Target time per tick (default: 100ms).
    pub tick_interval: Duration,
    /// Maximum handlers per phase before warning.
    pub max_handlers_per_phase: usize,
    /// Whether to log each phase transition.
    pub verbose: bool,
}

impl Default for RuntimeConfig {
    fn default() -> Self {
        Self {
            tick_interval: Duration::from_millis(100),
            max_handlers_per_phase: 32,
            verbose: false,
        }
    }
}

/// Metrics from the runtime.
#[derive(Debug, Clone, Default)]
pub struct RuntimeMetrics {
    pub total_ticks: u64,
    pub total_phases_executed: u64,
    pub phase_counts: std::collections::HashMap<CognitivePhase, u64>,
    pub average_tick_duration: Duration,
    pub errors: u64,
}

/// The Cognitive Runtime — the heartbeat.
///
/// Runs the Observe → Understand → Plan → Act → Verify → Reflect → Learn loop.
/// Every engine registers as a phase handler. The runtime doesn't know
/// any engine names — it only asks "who wants to participate in this phase?"
///
/// **Invariant:** This crate must compile and run with every other engine disabled.
pub struct CognitiveRuntime {
    config: RuntimeConfig,
    registry: PhaseRegistry,
    workspace: GlobalWorkspace,
    metrics: RuntimeMetrics,
}

impl CognitiveRuntime {
    pub fn new(config: RuntimeConfig, registry: PhaseRegistry, workspace: GlobalWorkspace) -> Self {
        Self {
            config,
            registry,
            workspace,
            metrics: RuntimeMetrics::default(),
        }
    }

    /// Get a reference to the workspace.
    pub fn workspace(&self) -> &GlobalWorkspace {
        &self.workspace
    }

    /// Get current metrics.
    pub fn metrics(&self) -> &RuntimeMetrics {
        &self.metrics
    }

    /// Run a single tick through all phases.
    ///
    /// Returns the tick result and the duration.
    pub async fn tick(&mut self, tick_number: u64) -> (TickId, Duration) {
        let tick_start = Instant::now();
        let tick_id = TickId::new();

        // Set the current tick context so publish() stamps events correctly.
        self.workspace.set_current_tick(tick_id.0);

        for &phase in CognitivePhase::all() {
            let handlers = self.registry.handlers_for(phase);

            if handlers.is_empty() {
                if self.config.verbose {
                    debug!(phase = %phase, tick = tick_number, "no handlers, skipping");
                }
                continue;
            }

            if handlers.len() > self.config.max_handlers_per_phase {
                warn!(
                    phase = %phase,
                    count = handlers.len(),
                    max = self.config.max_handlers_per_phase,
                    "too many handlers for phase"
                );
            }

            let phase_start = Instant::now();

            for handler in &handlers {
                let result = handler
                    .execute(phase, &self.workspace, tick_number)
                    .await;

                match result {
                    PhaseResult::Ok => {
                        if self.config.verbose {
                            debug!(
                                handler = handler.name(),
                                phase = %phase,
                                tick = tick_number,
                                "handler completed"
                            );
                        }
                    }
                    PhaseResult::Published { topics } => {
                        if self.config.verbose {
                            debug!(
                                handler = handler.name(),
                                phase = %phase,
                                topics = ?topics,
                                tick = tick_number,
                                "handler published events"
                            );
                        }
                    }
                    PhaseResult::SkipRemaining => {
                        debug!(
                            handler = handler.name(),
                            phase = %phase,
                            tick = tick_number,
                            "handler requested skip remaining"
                        );
                        break;
                    }
                    PhaseResult::Error { message } => {
                        warn!(
                            handler = handler.name(),
                            phase = %phase,
                            message = %message,
                            tick = tick_number,
                            "handler error"
                        );
                        self.metrics.errors += 1;
                    }
                }

                self.metrics.total_phases_executed += 1;
                *self
                    .metrics
                    .phase_counts
                    .entry(phase)
                    .or_insert(0) += 1;
            }

            let phase_duration = phase_start.elapsed();
            if self.config.verbose {
                debug!(
                    phase = %phase,
                    handlers = handlers.len(),
                    duration_ms = phase_duration.as_millis() as u64,
                    tick = tick_number,
                    "phase complete"
                );
            }
        }

        self.metrics.total_ticks += tick_number;

        let tick_duration = tick_start.elapsed();
        self.metrics.average_tick_duration = tick_duration;

        // Publish tick completion to workspace.
        self.workspace.publish_from(
            Topic::TickCompleted,
            EngineId::Runtime,
            serde_json::json!({
                "tick_number": tick_number,
                "duration_ms": tick_duration.as_millis() as u64,
            }),
        );

        (tick_id, tick_duration)
    }

    /// Run the heartbeat continuously until the shutdown signal is received.
    pub async fn run(&mut self, mut shutdown: watch::Receiver<bool>) {
        info!(
            tick_interval_ms = self.config.tick_interval.as_millis() as u64,
            "cognitive runtime starting"
        );

        let mut tick_number: u64 = 0;
        let mut interval = tokio::time::interval(self.config.tick_interval);
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

        loop {
            tokio::select! {
                _ = interval.tick() => {
                    tick_number += 1;
                    let (_tick_id, _duration) = self.tick(tick_number).await;
                }
                _ = shutdown.changed() => {
                    if *shutdown.borrow() {
                        info!(total_ticks = tick_number, "cognitive runtime shutting down");
                        break;
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::handler::{PhaseHandler, PhaseResult};
    use async_trait::async_trait;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    struct CountingHandler {
        name: String,
        phases: Vec<CognitivePhase>,
        count: Arc<AtomicUsize>,
    }

    #[async_trait]
    impl PhaseHandler for CountingHandler {
        fn name(&self) -> &str {
            &self.name
        }
        fn phases(&self) -> Vec<CognitivePhase> {
            self.phases.clone()
        }
        async fn execute(
            &self,
            _phase: CognitivePhase,
            _workspace: &GlobalWorkspace,
            _tick_number: u64,
        ) -> PhaseResult {
            self.count.fetch_add(1, Ordering::SeqCst);
            PhaseResult::Ok
        }
    }

    struct PublishingHandler;

    #[async_trait]
    impl PhaseHandler for PublishingHandler {
        fn name(&self) -> &str {
            "publishing_handler"
        }
        fn phases(&self) -> Vec<CognitivePhase> {
            vec![CognitivePhase::Observe]
        }
        async fn execute(
            &self,
            _phase: CognitivePhase,
            workspace: &GlobalWorkspace,
            _tick_number: u64,
        ) -> PhaseResult {
            workspace.publish_from(
                Topic::Mouse,
                EngineId::Perception,
                serde_json::json!({"x": 100, "y": 200}),
            );
            PhaseResult::Published {
                topics: vec!["sensor.mouse".into()],
            }
        }
    }

    #[tokio::test]
    async fn single_tick_runs_all_phases() {
        let count = Arc::new(AtomicUsize::new(0));
        let mut registry = PhaseRegistry::new();
        registry.register(Arc::new(CountingHandler {
            name: "all_phases".into(),
            phases: CognitivePhase::all().to_vec(),
            count: count.clone(),
        }));

        let workspace = GlobalWorkspace::new();
        let config = RuntimeConfig {
            verbose: false,
            ..Default::default()
        };
        let mut runtime = CognitiveRuntime::new(config, registry, workspace);

        let (_, duration) = runtime.tick(1).await;
        assert!(duration.as_millis() < 1000);
        assert_eq!(count.load(Ordering::SeqCst), 7); // one per phase
    }

    #[tokio::test]
    async fn heartbeat_runs_1000_ticks() {
        let count = Arc::new(AtomicUsize::new(0));
        let mut registry = PhaseRegistry::new();
        registry.register(Arc::new(CountingHandler {
            name: "counter".into(),
            phases: vec![CognitivePhase::Observe],
            count: count.clone(),
        }));

        let workspace = GlobalWorkspace::new();
        let config = RuntimeConfig::default();
        let mut runtime = CognitiveRuntime::new(config, registry, workspace);

        let start = Instant::now();
        for i in 1..=1000 {
            runtime.tick(i).await;
        }
        let elapsed = start.elapsed();

        assert_eq!(count.load(Ordering::SeqCst), 1000);
        assert!(
            elapsed.as_millis() < 10_000,
            "1000 ticks took too long: {:?}",
            elapsed
        );
    }

    #[tokio::test]
    async fn workspace_events_across_ticks() {
        let mut registry = PhaseRegistry::new();
        registry.register(Arc::new(PublishingHandler));

        let config = RuntimeConfig::default();
        let workspace = GlobalWorkspace::new();
        let mut runtime = CognitiveRuntime::new(config, registry, workspace);

        // Run 5 ticks.
        for i in 1..=5 {
            runtime.tick(i).await;
        }

        // Check ALL events in the runtime's workspace.
        let ws = runtime.workspace();
        let all = ws.history(None);

        // Should have at least: 5 sensor.mouse + 5 cognitive.tick.completed
        assert!(
            all.len() >= 10,
            "expected at least 10 events, got {}. Topics: {:?}",
            all.len(),
            all.iter().map(|e| &e.topic).collect::<Vec<_>>()
        );
    }

    #[tokio::test]
    async fn empty_registry_runs_clean() {
        let registry = PhaseRegistry::new();
        let workspace = GlobalWorkspace::new();
        let config = RuntimeConfig::default();
        let mut runtime = CognitiveRuntime::new(config, registry, workspace);

        // Should run 100 ticks without panicking.
        for i in 1..=100 {
            runtime.tick(i).await;
        }

        // 100 ticks ran, no handlers executed, no errors.
        assert_eq!(runtime.metrics().total_phases_executed, 0);
        assert_eq!(runtime.metrics().errors, 0);
    }

    #[tokio::test]
    async fn runtime_with_shutdown() {
        let registry = PhaseRegistry::new();
        let workspace = GlobalWorkspace::new();
        let config = RuntimeConfig {
            tick_interval: Duration::from_millis(10),
            ..Default::default()
        };
        let mut runtime = CognitiveRuntime::new(config, registry, workspace);

        let (shutdown_tx, shutdown_rx) = watch::channel(false);

        // Let it run for a bit, then shut down.
        let handle = tokio::spawn(async move {
            runtime.run(shutdown_rx).await;
            runtime
        });

        tokio::time::sleep(Duration::from_millis(150)).await;
        shutdown_tx.send(true).unwrap();

        let runtime = handle.await.unwrap();
        assert!(runtime.metrics().total_ticks > 0);
    }
}
