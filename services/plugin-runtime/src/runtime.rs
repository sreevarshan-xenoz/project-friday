use std::collections::HashMap;

use crate::catalog::CapabilityCatalog;
use crate::contract::{
    Capability, CapabilityRequest, CapabilityResult, CorrelationId, DecisionRecord, ExecutionPlan,
    PlanOutcome, Plugin, PluginHealth, PluginStatus, ProviderProfile, RuntimeError, ScoringWeights,
    SelectionContext, SelectionRationale, TelemetryEvent,
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

        // ── Execute with fallback support ──────────────────────────
        // The runtime never generates plans. It only executes plans
        // that already exist in the immutable fallback chain.
        // This preserves the invariant: only the planner creates plans.

        let primary_result = self.execute(&plan, request)?;

        let (final_result, executed_plan) = self.resolve_with_fallback(
            &plan,
            primary_result,
            &capability,
            &context,
            &parameters,
        );

        // Emit telemetry for the final result
        if final_result.success {
            self.telemetry.emit_event(
                cid.clone(),
                TelemetryEvent::PluginCompleted {
                    plugin_id: executed_plan.operator_id.clone(),
                    duration_ms: final_result.telemetry.duration_ms(),
                    success: true,
                },
            );
        } else {
            let error = final_result
                .metadata
                .get("error")
                .cloned()
                .unwrap_or_else(|| "unknown error".into());
            self.telemetry.emit_event(
                cid,
                TelemetryEvent::PluginFailed {
                    plugin_id: executed_plan.operator_id.clone(),
                    error,
                    duration_ms: final_result.telemetry.duration_ms(),
                },
            );
        }

        // Every execution produces exactly one decision record for the
        // primary plan. Fallback attempts are also recorded (above).
        let outcome = PlanOutcome {
            latency_ms: final_result.telemetry.duration_ms(),
            cost: 0.0,
            success: final_result.success,
            error: final_result.metadata.get("error").cloned(),
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
            plan: executed_plan.clone(),
            outcome,
            reflection: None,
        };

        self.ledger.append(decision);

        let rationale = SelectionRationale {
            capability: executed_plan.capability,
            candidates: plan.rationale.candidates,
            selected_plugin_id: plan.rationale.selected_operator_id,
            composite_score: plan.rationale.composite_score,
            explanation: plan.rationale.explanation,
            correlation_id: plan.rationale.correlation_id,
        };

        Ok((final_result, rationale))
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

    /// Try fallback plans when the primary execution fails.
    ///
    /// Iterates through the plan's immutable fallback chain, executing each
    /// until one succeeds or the chain is exhausted. The runtime never
    /// generates new plans — only existing plans from the chain are used.
    ///
    /// Each fallback attempt is recorded as a DecisionRecord in the ledger
    /// (for failures; the caller records the final decision separately).
    pub(crate) fn resolve_with_fallback(
        &mut self,
        plan: &ExecutionPlan,
        primary_result: CapabilityResult,
        capability: &Capability,
        context: &SelectionContext,
        parameters: &HashMap<String, String>,
    ) -> (CapabilityResult, ExecutionPlan) {
        if primary_result.success || plan.fallback_chain.is_empty() {
            return (primary_result, plan.clone());
        }

        let mut final_result = primary_result;
        let mut executed_plan = plan.clone();

        for fallback in &plan.fallback_chain {
            let fb_req = CapabilityRequest {
                capability: capability.clone(),
                parameters: parameters.clone(),
                permissions: Vec::new(),
                correlation_id: CorrelationId::new(),
                context: context.clone(),
            };

            match self.execute(fallback, fb_req) {
                Ok(fb_result) if fb_result.success => {
                    final_result = fb_result;
                    executed_plan = fallback.clone();
                    break;
                }
                Ok(fb_result) => {
                    // Fallback failed — record it and try next
                    self.ledger.append(DecisionRecord {
                        id: CorrelationId::new(),
                        timestamp_ms: now_ms(),
                        capability: capability.clone(),
                        context: context.clone(),
                        parameters: parameters.clone(),
                        plan: fallback.clone(),
                        outcome: PlanOutcome {
                            latency_ms: fb_result.telemetry.duration_ms(),
                            cost: 0.0,
                            success: false,
                            error: fb_result.metadata.get("error").cloned(),
                            completed_steps: 0,
                            cancelled: false,
                            timeout: false,
                            retries: 0,
                        },
                        reflection: None,
                    });
                }
                Err(_) => {
                    // Plugin unavailable — skip this fallback
                    continue;
                }
            }
        }

        (final_result, executed_plan)
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
        Capability, CapabilityRequest, CapabilityResult, ExecutionTelemetry,
        PlanningRationale, Plugin, PluginDescriptor, PluginHealth, PluginStatus, ProviderProfile,
        SelectionContext,
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

    // ── Fallback tests ────────────────────────────────────────────

    fn make_fallback_plan(
        operator_id: &str,
        fallbacks: Vec<ExecutionPlan>,
    ) -> ExecutionPlan {
        let cid = CorrelationId::new();
        ExecutionPlan {
            plan_id: cid.clone(),
            version: 1,
            capability: Capability::ReadFile,
            operator_id: operator_id.into(),
            estimated_latency_ms: 10,
            estimated_cost: 0.1,
            estimated_success_probability: 0.9,
            estimated_risk: 0.1,
            confidence: 0.9,
            fallback_chain: fallbacks,
            rationale: PlanningRationale {
                candidates: vec![],
                selected_operator_id: operator_id.into(),
                composite_score: 0.9,
                explanation: "test".into(),
                correlation_id: cid,
                evaluator_evidence: vec![],
            },
        }
    }

    fn fallback_runtime() -> PluginRuntime {
        let mut runtime = PluginRuntime::new();
        runtime.register(Box::new(MockPlugin::new(
            "primary-fail",
            vec![Capability::ReadFile],
            false,
        )));
        runtime.register(Box::new(MockPlugin::new(
            "fallback-ok",
            vec![Capability::ReadFile],
            true,
        )));
        runtime.register(Box::new(MockPlugin::new(
            "fallback-also-fail",
            vec![Capability::ReadFile],
            false,
        )));
        runtime.initialize_all();
        runtime.activate_all();
        runtime
    }

    #[test]
    fn fallback_primary_success_no_fallback_needed() {
        let mut runtime = PluginRuntime::new();
        runtime.register(Box::new(MockPlugin::new(
            "always-ok",
            vec![Capability::ReadFile],
            true,
        )));
        runtime.initialize_all();
        runtime.activate_all();

        let plan = make_fallback_plan("always-ok", vec![]);
        let req = CapabilityRequest::new(Capability::ReadFile);
        let result = runtime.execute(&plan, req).unwrap();

        let (final_result, _) = runtime.resolve_with_fallback(
            &plan, result,
            &Capability::ReadFile,
            &SelectionContext::default(),
            &HashMap::new(),
        );

        assert!(final_result.success);
        assert_eq!(final_result.output, "executed read_file via always-ok");
        assert_eq!(runtime.ledger().len(), 0);
    }

    #[test]
    fn fallback_primary_fails_no_chain_returns_primary() {
        let mut runtime = fallback_runtime();

        let plan = make_fallback_plan("primary-fail", vec![]);
        let req = CapabilityRequest::new(Capability::ReadFile);
        let result = runtime.execute(&plan, req).unwrap();

        let (final_result, executed) = runtime.resolve_with_fallback(
            &plan, result,
            &Capability::ReadFile,
            &SelectionContext::default(),
            &HashMap::new(),
        );

        assert!(!final_result.success);
        assert_eq!(executed.operator_id, "primary-fail");
        assert_eq!(runtime.ledger().len(), 0);
    }

    #[test]
    fn fallback_succeeds_when_primary_fails() {
        let mut runtime = fallback_runtime();

        let fallback = make_fallback_plan("fallback-ok", vec![]);
        let plan = make_fallback_plan("primary-fail", vec![fallback]);

        let req = CapabilityRequest::new(Capability::ReadFile);
        let result = runtime.execute(&plan, req).unwrap();

        let (final_result, executed) = runtime.resolve_with_fallback(
            &plan, result,
            &Capability::ReadFile,
            &SelectionContext::default(),
            &HashMap::new(),
        );

        assert!(final_result.success);
        assert_eq!(executed.operator_id, "fallback-ok");
        assert_eq!(final_result.output, "executed read_file via fallback-ok");
        assert_eq!(runtime.ledger().len(), 0);
    }

    #[test]
    fn fallback_all_fail_returns_primary_result() {
        let mut runtime = fallback_runtime();

        let fb1 = make_fallback_plan("fallback-also-fail", vec![]);
        let plan = make_fallback_plan("primary-fail", vec![fb1]);

        let req = CapabilityRequest::new(Capability::ReadFile);
        let result = runtime.execute(&plan, req).unwrap();

        let (final_result, executed) = runtime.resolve_with_fallback(
            &plan, result,
            &Capability::ReadFile,
            &SelectionContext::default(),
            &HashMap::new(),
        );

        assert!(!final_result.success);
        assert_eq!(executed.operator_id, "primary-fail");
        // Failed fallback recorded in ledger
        assert_eq!(runtime.ledger().len(), 1);
    }

    #[test]
    fn fallback_returns_fast_on_first_success() {
        let mut runtime = fallback_runtime();

        // Primary fails, first fallback succeeds, second fallback never tried
        let fb2 = make_fallback_plan("fallback-also-fail", vec![]);
        let fb1 = make_fallback_plan("fallback-ok", vec![fb2]);
        let plan = make_fallback_plan("primary-fail", vec![fb1]);

        let req = CapabilityRequest::new(Capability::ReadFile);
        let result = runtime.execute(&plan, req).unwrap();

        let (final_result, executed) = runtime.resolve_with_fallback(
            &plan, result,
            &Capability::ReadFile,
            &SelectionContext::default(),
            &HashMap::new(),
        );

        assert!(final_result.success);
        assert_eq!(executed.operator_id, "fallback-ok");
        assert_eq!(runtime.ledger().len(), 0);
    }

    #[test]
    fn fallback_records_failed_attempts_skips_successful() {
        let mut runtime = fallback_runtime();

        // Primary fails, first fallback fails (recorded), second succeeds (not recorded)
        let fb_fail = make_fallback_plan("fallback-also-fail", vec![]);
        let fb_ok = make_fallback_plan("fallback-ok", vec![]);
        let plan = make_fallback_plan("primary-fail", vec![fb_fail, fb_ok]);

        let req = CapabilityRequest::new(Capability::ReadFile);
        let result = runtime.execute(&plan, req).unwrap();

        let (final_result, executed) = runtime.resolve_with_fallback(
            &plan, result,
            &Capability::ReadFile,
            &SelectionContext::default(),
            &HashMap::new(),
        );

        assert!(final_result.success);
        assert_eq!(executed.operator_id, "fallback-ok");
        // Only the failed fallback (fb_fail) is recorded in ledger
        assert_eq!(runtime.ledger().len(), 1);
    }
}
