use std::collections::HashMap;

use crate::catalog::CapabilityCatalog;
use crate::contract::{
    CapabilityRequest, CapabilityResult, CorrelationId, DecisionRecord, ExecutionPlan, PlanOutcome,
    Plugin, PluginHealth, PluginStatus, ProviderProfile, RuntimeError, ScoringWeights,
    SelectionRationale, TelemetryEvent,
};
use crate::utils::now_ms;
use crate::health::HealthMonitor;
use crate::ledger::DecisionLedger;
use crate::policy::{DefaultPolicyEngine, PolicyEngine};
use crate::registry::CapabilityRegistry;
use crate::planner::CapabilityPlanner;
use crate::telemetry::{TelemetryCollector, TelemetryObserver};

/// Plugin Runtime — owns lifecycle, invocation routing, health collection,
/// capability catalog, policy enforcement, provider profiles, and execution telemetry.
///
/// Every capability execution enters and exits through this single
/// abstraction. The Executive never knows which plugin was chosen.
/// Every significant decision emits a structured telemetry event.
pub struct PluginRuntime {
    plugins: HashMap<String, Box<dyn Plugin>>,
    registry: CapabilityRegistry,
    health: HealthMonitor,
    catalog: CapabilityCatalog,
    policy: Box<dyn PolicyEngine>,
    weights: ScoringWeights,
    telemetry: TelemetryCollector,
    profiles: HashMap<String, ProviderProfile>,
    ledger: DecisionLedger,
}

impl Default for PluginRuntime {
    fn default() -> Self {
        Self::new()
    }
}

impl PluginRuntime {
    pub fn new() -> Self {
        Self {
            plugins: HashMap::new(),
            registry: CapabilityRegistry::new(),
            health: HealthMonitor::new(),
            catalog: CapabilityCatalog::with_defaults(),
            policy: Box::new(DefaultPolicyEngine),
            weights: ScoringWeights::default(),
            telemetry: TelemetryCollector::new(),
            profiles: HashMap::new(),
            ledger: DecisionLedger::new(),
        }
    }

    /// Register a plugin, its capabilities, profile, and begin health tracking.
    pub fn register(&mut self, plugin: Box<dyn Plugin>) {
        let desc = plugin.descriptor();
        let plugin_id = desc.id.clone();
        let profile = desc.profile.clone();

        self.registry.register(desc);
        self.health.register(&plugin_id);
        self.profiles.insert(plugin_id.clone(), profile);
        self.plugins.insert(plugin_id, plugin);
    }

    /// Initialize all registered plugins.
    pub fn initialize_all(&mut self) {
        let ids: Vec<String> = self.plugins.keys().cloned().collect();
        for id in ids {
            if let Some(plugin) = self.plugins.get_mut(&id) {
                plugin.initialize();
                self.health.mark_initialized(&id);
            }
        }
    }

    /// Mark all initialized plugins as healthy.
    pub fn activate_all(&mut self) {
        let ids: Vec<String> = self.plugins.keys().cloned().collect();
        for id in ids {
            if let Some(plugin) = self.plugins.get(&id) {
                let h = plugin.health();
                if h.status == PluginStatus::Initialized || h.status == PluginStatus::Installed {
                    let old = h.status.clone();
                    self.health.mark_healthy(&id);
                    self.telemetry.emit_event(
                        CorrelationId::new(),
                        TelemetryEvent::HealthChanged {
                            plugin_id: id.clone(),
                            old_status: old,
                            new_status: PluginStatus::Healthy,
                        },
                    );
                }
            }
        }
    }

    /// Plan and execute a capability request.
    ///
    /// Two-phase: the planner produces an execution plan, then the runtime
    /// follows it. The plan is never modified during execution.
    pub fn plan_and_execute(
        &mut self,
        request: CapabilityRequest,
    ) -> Result<(CapabilityResult, SelectionRationale), RuntimeError> {
        let cid = request.correlation_id.clone();

        self.telemetry.emit_event(
            cid.clone(),
            TelemetryEvent::CapabilityRequested(request.capability.clone()),
        );

        let policy_candidates_before = self.registry.plugins_for(&request.capability).len();

        let planner = CapabilityPlanner::new(
            &self.registry,
            &self.catalog,
            &*self.policy,
            &self.weights,
            &self.profiles,
        );
        let health_snapshots = self.health.all_health();
        let plan = planner.plan(
            &request.capability,
            &health_snapshots,
            &cid,
            &request.context,
        )?;

        self.telemetry.emit_event(
            cid.clone(),
            TelemetryEvent::PolicyEvaluated {
                capability: request.capability.clone(),
                candidates_before: policy_candidates_before,
                candidates_after: plan.rationale.candidates.len(),
            },
        );

        self.telemetry.emit_event(
            cid.clone(),
            TelemetryEvent::ResolverScored {
                capability: request.capability.clone(),
                candidates: plan.rationale.candidates.clone(),
                selected: plan.operator_id.clone(),
            },
        );

        self.telemetry.emit_event(
            cid.clone(),
            TelemetryEvent::ProviderResolved {
                plugin_id: plan.operator_id.clone(),
                confidence: plan.confidence,
                candidate_count: plan.rationale.candidates.len(),
            },
        );

        // Capture request data before it's consumed by execute.
        let capability = request.capability.clone();
        let context = request.context.clone();
        let parameters = request.parameters.clone();

        let result = self.execute(&plan, request)?;

        if result.success {
            self.telemetry.emit_event(
                cid.clone(),
                TelemetryEvent::PluginCompleted {
                    plugin_id: plan.operator_id.clone(),
                    duration_ms: result.telemetry.duration_ms(),
                    success: true,
                },
            );
        } else {
            let error = result
                .metadata
                .get("error")
                .cloned()
                .unwrap_or_else(|| "unknown error".into());
            self.telemetry.emit_event(
                cid,
                TelemetryEvent::PluginFailed {
                    plugin_id: plan.operator_id.clone(),
                    error,
                    duration_ms: result.telemetry.duration_ms(),
                },
            );
        }

        // Every execution produces exactly one decision record.
        // This ensures no execution is ever forgotten.
        let outcome = PlanOutcome {
            latency_ms: result.telemetry.duration_ms(),
            cost: 0.0,
            success: result.success,
            error: result.metadata.get("error").cloned(),
            completed_steps: 1,
            cancelled: false,
            timeout: false,
            retries: 0,
        };

        let decision = DecisionRecord {
            id: CorrelationId::new(),
            timestamp_ms: now_ms(),
            capability,
            context,
            parameters,
            plan: plan.clone(),
            outcome,
            reflection: None,
        };

        self.ledger.append(decision);

        let rationale = SelectionRationale {
            capability: plan.capability,
            candidates: plan.rationale.candidates,
            selected_plugin_id: plan.rationale.selected_operator_id,
            composite_score: plan.rationale.composite_score,
            explanation: plan.rationale.explanation,
            correlation_id: plan.rationale.correlation_id,
        };

        Ok((result, rationale))
    }

    /// Execute a capability on a specific resolved plugin.
    pub fn execute_with(
        &mut self,
        resolved: &crate::contract::ResolvedCapability,
        request: CapabilityRequest,
    ) -> Result<CapabilityResult, RuntimeError> {
        let plugin = self.plugins.get(&resolved.plugin_id).ok_or_else(|| {
            RuntimeError::PluginNotFound(resolved.plugin_id.clone())
        })?;

        let h = plugin.health();
        if h.status != PluginStatus::Healthy
            && h.status != PluginStatus::Initialized
            && h.status != PluginStatus::Busy
        {
            return Err(RuntimeError::PluginNotReady(resolved.plugin_id.clone()));
        }

        self.telemetry.emit_event(
            request.correlation_id.clone(),
            TelemetryEvent::PluginInvoked {
                plugin_id: resolved.plugin_id.clone(),
                capability: request.capability.clone(),
            },
        );

        let result = plugin.execute(request);
        self.health.record_execution(&resolved.plugin_id, &result.telemetry);

        Ok(result)
    }

    /// Execute a plan — runs the operator chosen by the planner.
    ///
    /// Extracts the operator from the plan and delegates to the
    /// underlying plugin. The plan is never modified.
    pub fn execute(
        &mut self,
        plan: &ExecutionPlan,
        request: CapabilityRequest,
    ) -> Result<CapabilityResult, RuntimeError> {
        let plugin = self.plugins.get(&plan.operator_id).ok_or_else(|| {
            RuntimeError::PluginNotFound(plan.operator_id.clone())
        })?;

        let h = plugin.health();
        if h.status != PluginStatus::Healthy
            && h.status != PluginStatus::Initialized
            && h.status != PluginStatus::Busy
        {
            return Err(RuntimeError::PluginNotReady(plan.operator_id.clone()));
        }

        self.telemetry.emit_event(
            request.correlation_id.clone(),
            TelemetryEvent::PluginInvoked {
                plugin_id: plan.operator_id.clone(),
                capability: request.capability.clone(),
            },
        );

        let result = plugin.execute(request);
        self.health.record_execution(&plan.operator_id, &result.telemetry);

        Ok(result)
    }

    /// Check if a specific capability can be handled.
    pub fn can_handle(&self, capability: &crate::contract::Capability) -> bool {
        !self.registry.plugins_for(capability).is_empty()
    }

    /// Get health snapshots for all registered plugins.
    pub fn health_snapshots(&self) -> Vec<PluginHealth> {
        self.health.all_health().into_iter().cloned().collect()
    }

    /// Get health for a specific plugin.
    pub fn health_of(&self, plugin_id: &str) -> Option<PluginHealth> {
        self.health.health_of(plugin_id).cloned()
    }

    /// Get the capability registry (read-only for introspection).
    pub fn registry(&self) -> &CapabilityRegistry {
        &self.registry
    }

    /// Get the capability catalog.
    pub fn catalog(&self) -> &CapabilityCatalog {
        &self.catalog
    }

    /// Get the provider profile for a plugin.
    pub fn profile_of(&self, plugin_id: &str) -> Option<&ProviderProfile> {
        self.profiles.get(plugin_id)
    }

    /// Set a custom policy engine.
    pub fn set_policy(&mut self, policy: Box<dyn PolicyEngine>) {
        self.policy = policy;
    }

    /// Set custom scoring weights.
    pub fn set_weights(&mut self, weights: ScoringWeights) {
        self.weights = weights;
    }

    /// Register a telemetry observer.
    pub fn add_observer(&mut self, observer: Box<dyn TelemetryObserver>) {
        self.telemetry.add_observer(observer);
    }

    /// Access the telemetry collector.
    pub fn telemetry(&self) -> &TelemetryCollector {
        &self.telemetry
    }

    /// Access the decision ledger (read-only).
    ///
    /// Every execution produces exactly one decision record.
    /// The ledger is an append-only journal — records are never
    /// modified or removed after creation.
    pub fn ledger(&self) -> &DecisionLedger {
        &self.ledger
    }

    /// Shutdown all plugins gracefully.
    pub fn shutdown(&mut self) {
        for plugin in self.plugins.values_mut() {
            plugin.shutdown();
        }
    }

    /// Get a list of all registered plugin IDs.
    pub fn plugin_ids(&self) -> Vec<String> {
        self.plugins.keys().cloned().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contract::{
        Capability, CapabilityRequest, CapabilityResult, ExecutionTelemetry, Plugin,
        PluginDescriptor, PluginHealth, PluginStatus, ProviderProfile, SelectionContext,
    };
    use std::collections::HashMap;

    struct MockPlugin {
        id: String,
        execute_success: bool,
        latency_ms: u64,
    }

    impl MockPlugin {
        fn new(id: &str, _capabilities: Vec<Capability>, execute_success: bool) -> Self {
            Self {
                id: id.to_string(),
                execute_success,
                latency_ms: 10,
            }
        }
    }

    impl Plugin for MockPlugin {
        fn descriptor(&self) -> PluginDescriptor {
            PluginDescriptor {
                id: self.id.clone(),
                name: format!("Mock {}", self.id),
                version: "1.0.0".into(),
                author: "test".into(),
                capabilities: vec![Capability::ReadFile],
                permissions: vec![],
                profile: ProviderProfile::default_for(&self.id),
            }
        }

        fn initialize(&mut self) {}

        fn execute(&self, request: CapabilityRequest) -> CapabilityResult {
            let start = crate::utils::now_ms();
            let cid = request.correlation_id.clone();
            let output = if self.execute_success {
                format!("executed {} via {}", request.capability.as_str(), self.id)
            } else {
                String::new()
            };
            let mut metadata = HashMap::new();
            if !self.execute_success {
                metadata.insert("error".into(), "mock failure".into());
            }
            CapabilityResult {
                success: self.execute_success,
                output,
                metadata,
                telemetry: ExecutionTelemetry {
                    started_at_ms: start,
                    finished_at_ms: start + self.latency_ms,
                    plugin_id: self.id.clone(),
                    success: self.execute_success,
                    correlation_id: cid,
                },
            }
        }

        fn health(&self) -> PluginHealth {
            PluginHealth {
                plugin_id: self.id.clone(),
                status: PluginStatus::Healthy,
                availability: 1.0,
                reliability: 1.0,
                average_latency_ms: self.latency_ms as f64,
                success_rate: 1.0,
                total_executions: 0,
                failure_count: 0,
                last_error: None,
                last_execution_ms: None,
            }
        }

        fn shutdown(&mut self) {}
    }

    #[test]
    fn runtime_register_and_execute() {
        let mut runtime = PluginRuntime::new();
        runtime.register(Box::new(MockPlugin::new(
            "mock-fs",
            vec![Capability::ReadFile],
            true,
        )));
        runtime.initialize_all();
        runtime.activate_all();

        let (result, rationale) = runtime
            .plan_and_execute(CapabilityRequest::new(Capability::ReadFile))
            .unwrap();
        assert!(result.success);
        assert!(result.output.contains("mock-fs"));
        assert!(rationale.explanation.contains("mock-fs"));
    }

    #[test]
    fn runtime_health_updates_after_execution() {
        let mut runtime = PluginRuntime::new();
        runtime.register(Box::new(MockPlugin::new(
            "health-test",
            vec![Capability::ReadFile],
            true,
        )));
        runtime.initialize_all();
        runtime.activate_all();

        runtime
            .plan_and_execute(CapabilityRequest::new(Capability::ReadFile))
            .unwrap();

        let health = runtime.health_of("health-test").unwrap();
        assert_eq!(health.total_executions, 1);
        assert_eq!(health.failure_count, 0);
    }

    #[test]
    fn runtime_returns_error_for_unregistered_capability() {
        let mut runtime = PluginRuntime::new();
        let result =
            runtime.plan_and_execute(CapabilityRequest::new(Capability::OpenBrowser));
        match result {
            Err(RuntimeError::NoProviderFound(_)) => {}
            _ => panic!("expected NoProviderFound"),
        }
    }

    #[test]
    fn runtime_can_handle_check() {
        let mut runtime = PluginRuntime::new();
        runtime.register(Box::new(MockPlugin::new(
            "check-test",
            vec![Capability::ReadFile],
            true,
        )));

        assert!(runtime.can_handle(&Capability::ReadFile));
        assert!(!runtime.can_handle(&Capability::WriteFile));
    }

    #[test]
    fn runtime_shutdown_gracefully() {
        let mut runtime = PluginRuntime::new();
        runtime.register(Box::new(MockPlugin::new(
            "shutdown-test",
            vec![Capability::ReadFile],
            true,
        )));
        runtime.shutdown();
        assert!(runtime.health_of("shutdown-test").is_some());
    }

    #[test]
    fn runtime_records_failure_metrics() {
        let mut runtime = PluginRuntime::new();
        runtime.register(Box::new(MockPlugin::new(
            "fail-test",
            vec![Capability::ReadFile],
            false,
        )));
        runtime.initialize_all();
        runtime.activate_all();

        let (result, _) = runtime
            .plan_and_execute(CapabilityRequest::new(Capability::ReadFile))
            .unwrap();
        assert!(!result.success);

        let health = runtime.health_of("fail-test").unwrap();
        assert_eq!(health.failure_count, 1);
    }

    #[test]
    fn runtime_produces_selection_rationale() {
        let mut runtime = PluginRuntime::new();
        runtime.register(Box::new(MockPlugin::new(
            "rationale-test",
            vec![Capability::ReadFile],
            true,
        )));
        runtime.initialize_all();
        runtime.activate_all();

        let (_, rationale) = runtime
            .plan_and_execute(CapabilityRequest::new(Capability::ReadFile))
            .unwrap();

        assert_eq!(rationale.capability, Capability::ReadFile);
        assert!(!rationale.explanation.is_empty());
        assert!(rationale.composite_score > 0.0);
    }

    #[test]
    fn runtime_emits_telemetry_events() {
        let mut runtime = PluginRuntime::new();
        runtime.register(Box::new(MockPlugin::new(
            "telemetry-test",
            vec![Capability::ReadFile],
            true,
        )));
        runtime.initialize_all();
        runtime.activate_all();

        runtime
            .plan_and_execute(CapabilityRequest::new(Capability::ReadFile))
            .unwrap();

        let store = runtime.telemetry().store();
        assert!(store.count() > 0);

        // Should have at least: CapabilityRequested, PolicyEvaluated,
        // ResolverScored, ProviderResolved, PluginInvoked, PluginCompleted
        let requested = store.of_type("capability.requested");
        assert!(!requested.is_empty(), "missing capability.requested event");

        let invoked = store.of_type("plugin.invoked");
        assert!(!invoked.is_empty(), "missing plugin.invoked event");

        let completed = store.of_type("plugin.completed");
        assert!(!completed.is_empty(), "missing plugin.completed event");
    }

    #[test]
    fn runtime_respects_selection_context() {
        let mut runtime = PluginRuntime::new();
        runtime.register(Box::new(MockPlugin::new(
            "ctx-test",
            vec![Capability::ReadFile],
            true,
        )));
        runtime.initialize_all();
        runtime.activate_all();

        let ctx = SelectionContext::urgent();
        let req = CapabilityRequest::new(Capability::ReadFile)
            .with_context(ctx);
        let (result, _) = runtime.plan_and_execute(req).unwrap();
        assert!(result.success);

        // Context should also work for offline mode
        let ctx = SelectionContext::offline();
        let req = CapabilityRequest::new(Capability::ReadFile)
            .with_context(ctx);
        let (result, _) = runtime.plan_and_execute(req).unwrap();
        assert!(result.success);
    }

    #[test]
    fn runtime_stores_provider_profile() {
        let mut runtime = PluginRuntime::new();
        runtime.register(Box::new(MockPlugin::new(
            "profile-test",
            vec![Capability::ReadFile],
            true,
        )));

        let profile = runtime.profile_of("profile-test").unwrap();
        assert_eq!(profile.plugin_id, "profile-test");
        assert!(profile.deterministic);
        assert!(profile.offline_capable);
    }

    #[test]
    fn successful_execution_appends_to_ledger() {
        let mut runtime = PluginRuntime::new();
        runtime.register(Box::new(MockPlugin::new(
            "ledger-succ",
            vec![Capability::ReadFile],
            true,
        )));
        runtime.initialize_all();
        runtime.activate_all();

        assert_eq!(runtime.ledger().len(), 0);

        runtime
            .plan_and_execute(CapabilityRequest::new(Capability::ReadFile))
            .unwrap();

        assert_eq!(runtime.ledger().len(), 1);

        let record = runtime.ledger().get(0).unwrap();
        assert_eq!(record.capability, Capability::ReadFile);
        assert!(record.outcome.success);
        assert!(record.reflection.is_none());
    }

    #[test]
    fn failed_execution_appends_to_ledger() {
        let mut runtime = PluginRuntime::new();
        runtime.register(Box::new(MockPlugin::new(
            "ledger-fail",
            vec![Capability::ReadFile],
            false,
        )));
        runtime.initialize_all();
        runtime.activate_all();

        assert_eq!(runtime.ledger().len(), 0);

        runtime
            .plan_and_execute(CapabilityRequest::new(Capability::ReadFile))
            .unwrap();

        assert_eq!(runtime.ledger().len(), 1);

        let record = runtime.ledger().get(0).unwrap();
        assert!(!record.outcome.success);
        assert!(record.outcome.error.is_some());
    }

    #[test]
    fn ledger_grows_with_multiple_executions() {
        let mut runtime = PluginRuntime::new();
        runtime.register(Box::new(MockPlugin::new(
            "ledger-multi",
            vec![Capability::ReadFile],
            true,
        )));
        runtime.initialize_all();
        runtime.activate_all();

        for _ in 0..5 {
            runtime
                .plan_and_execute(CapabilityRequest::new(Capability::ReadFile))
                .unwrap();
        }

        assert_eq!(runtime.ledger().len(), 5);
    }
}
