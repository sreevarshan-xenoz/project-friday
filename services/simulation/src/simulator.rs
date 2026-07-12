//! The Plan Simulation Engine (M4.7): a Digital Twin of the agent.
//!
//! Given a [`TaskGraph`], an immutable [`WorldState`], the capability [`registry`],
//! and a map of `Simulatable` services, the engine virtualizes the entire plan
//! without touching the real system: it validates preconditions, simulates each
//! step against the world snapshot, accumulates side effects / resource forecasts,
//! derives a confidence score with explicit assumptions, generates fallback paths
//! for unavailable services, and emits a machine-readable [`SimulationReport`].

use std::collections::HashMap;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use friday_capabilities::model::Capability;
use friday_capabilities::registry::CapabilityRegistry;
use friday_capabilities::sim::{
    Prediction, ResourceForecast, SideEffect, SimRequest, Simulatable, StepEstimate, WorldState,
};
use friday_planner::TaskGraph;

use crate::approval::{PlanSummary, RiskLevel};
use crate::estimator::{aggregate, PlanEstimate};
use crate::optimizer::{optimize, OptimizerDiff};
use crate::validator::{validate, IssueSeverity, ValidationIssue};

/// A fallback path generated when the preferred capability is unavailable.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FallbackPath {
    pub from: String,
    pub to: String,
    pub available: bool,
}

/// Machine-readable result of simulating a plan.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimulationReport {
    pub plan_id: Uuid,
    pub intent: String,
    pub summary: PlanSummary,
    pub risk: RiskLevel,
    pub resource_forecast: ResourceForecast,
    pub timeline_ms: u64,
    pub side_effects: Vec<SideEffect>,
    pub confidence: Prediction,
    pub fallbacks: Vec<FallbackPath>,
    pub issues: Vec<ValidationIssue>,
    pub rollback_available: bool,
    pub optimized_plan: Option<TaskGraph>,
    pub optimizer_diff: Option<OptimizerDiff>,
    pub world_before: WorldState,
    pub world_after: WorldState,
    pub approval_required: bool,
}

/// The engine: owns the world snapshot, registry, and the `tool -> Simulatable`
/// map. Construct via [`SimulationEngine::new`]; run via [`simulate_plan`].
///
/// [`simulate_plan`]: SimulationEngine::simulate_plan
pub struct SimulationEngine {
    registry: Arc<CapabilityRegistry>,
    sims: HashMap<String, Arc<dyn Simulatable>>,
    world: WorldState,
}

fn split_cap(cap: &str) -> (String, String) {
    match cap.split_once('.') {
        Some((t, a)) => (t.to_string(), a.to_string()),
        None => (cap.to_string(), cap.to_string()),
    }
}

fn fallback_capability(cap: &str) -> Option<String> {
    cap.strip_prefix("browser.")
        .map(|rest| format!("windows.{rest}"))
}

impl SimulationEngine {
    pub fn new(
        registry: Arc<CapabilityRegistry>,
        sims: HashMap<String, Arc<dyn Simulatable>>,
        world: WorldState,
    ) -> Self {
        Self {
            registry,
            sims,
            world,
        }
    }

    /// Resolve the best available provider for a capability id, trying the id
    /// directly then its `tool` prefix.
    fn resolve(&self, cap_id: &str) -> Option<(String, Capability)> {
        if let Some(first) = self
            .registry
            .ranked_providers_for(cap_id)
            .into_iter()
            .next()
        {
            return Some(first);
        }
        let tool = cap_id.split('.').next().unwrap_or(cap_id);
        self.registry.ranked_providers_for(tool).into_iter().next()
    }

    /// Simulate a whole plan. Returns `Err` only on a structurally invalid graph
    /// (cycle); other problems surface as [`ValidationIssue`]s in the report.
    pub fn simulate_plan(&self, graph: &TaskGraph) -> Result<SimulationReport, String> {
        let topo = graph.topo_order()?;
        let mut world = self.world.clone();
        let mut effects: Vec<SideEffect> = Vec::new();
        let mut steps: Vec<StepEstimate> = Vec::new();
        let mut forecasts: Vec<ResourceForecast> = Vec::new();
        let mut used_caps: Vec<Capability> = Vec::new();
        let mut issues: Vec<ValidationIssue> = Vec::new();
        let mut fallbacks: Vec<FallbackPath> = Vec::new();
        let mut rollback_available = false;
        let mut unknown_effects: usize = 0;

        for id in &topo {
            let task = graph.task(*id).expect("task present in topo order");
            let cap_id = task.capability.clone();

            // Preferred provider.
            let provider = self.resolve(&cap_id);

            // If unavailable, try a fallback path (e.g. browser → windows).
            let resolved_cap_id = if provider.is_some() {
                cap_id.clone()
            } else if let Some(fb) = fallback_capability(&cap_id) {
                let fb_available = self.resolve(&fb).is_some();
                fallbacks.push(FallbackPath {
                    from: cap_id.clone(),
                    to: fb.clone(),
                    available: fb_available,
                });
                fb
            } else {
                cap_id.clone()
            };

            let (rtool, raction) = split_cap(&resolved_cap_id);
            let sim = self.sims.get(&rtool).cloned();
            let chosen_cap = provider.as_ref().map(|(_, c)| c.clone());
            let cap = chosen_cap.or_else(|| self.resolve(&resolved_cap_id).map(|(_, c)| c));

            if let (Some(cap), Some(sim)) = (cap, sim) {
                let req = SimRequest::new(rtool, raction, task.params.clone());
                for p in sim.preconditions(&req, &world) {
                    if !p.satisfied {
                        issues.push(ValidationIssue::warning(
                            &p.kind,
                            format!("precondition not met: {}", p.description),
                        ));
                    }
                }
                let res = sim.simulate(&req, &world);
                effects.extend(res.effects.clone());
                steps.push(res.estimate);
                forecasts.push(res.forecast);
                used_caps.push(cap);
                if res.rollback.is_some() {
                    rollback_available = true;
                }
                world = world.apply_effects(&res.effects);
            } else {
                issues.push(ValidationIssue::error(
                    "unavailable_service",
                    format!("no simulation path for capability '{cap_id}'"),
                ));
                unknown_effects += 1;
            }
        }

        // Structural validation findings.
        for i in validate(graph, &self.registry).issues {
            issues.push(i);
        }

        let plan_est: PlanEstimate = aggregate(&used_caps, &steps, &forecasts);
        let risk_level = RiskLevel::from_risk(plan_est.cost.risk);

        // Confidence with explicit, auditable assumptions.
        let mut assumptions = vec![
            format!("git available: {}", self.registry.available("git")),
            format!("windows available: {}", self.registry.available("windows")),
            format!("disk writable: {}", self.world.filesystem.writable),
        ];
        let unsatisfied = issues
            .iter()
            .filter(|i| i.severity == IssueSeverity::Warning)
            .count();
        let mut confidence = plan_est.cost.expected_success * 0.9f64.powi(unsatisfied as i32);
        for fb in &fallbacks {
            if !fb.available {
                assumptions.push(format!("fallback '{}' also unavailable", fb.to));
                confidence *= 0.7;
            }
        }
        confidence = confidence.clamp(0.0, 1.0);
        let uncertainty = (1.0 - confidence + unknown_effects as f64 * 0.05).clamp(0.0, 1.0);
        let prediction = Prediction {
            confidence,
            uncertainty,
            assumptions,
        };

        let (optimized_plan, optimizer_diff) = optimize(graph, &self.registry);

        // Plan summary counts (from the side-effect graph).
        let flat: Vec<&SideEffect> = effects.iter().flat_map(|e| e.flatten()).collect();
        let windows = flat.iter().filter(|e| e.kind == "window.launch").count();
        let browsers = flat.iter().filter(|e| e.kind == "browser.tab").count();
        let files_affected = flat.iter().filter(|e| e.kind.starts_with("file.")).count();
        let destructive = flat
            .iter()
            .any(|e| matches!(e.kind.as_str(), "file.delete" | "dir.delete"));

        let summary = PlanSummary {
            actions: graph.tasks.len(),
            windows,
            browsers,
            files_affected,
            estimated_seconds: (plan_est.timeline_ms as f64 / 1000.0).ceil() as u64,
            risk: risk_level,
        };

        let approval_required = matches!(risk_level, RiskLevel::High)
            || destructive
            || unknown_effects > 0
            || issues.iter().any(|i| i.severity == IssueSeverity::Warning);

        Ok(SimulationReport {
            plan_id: graph.plan_id,
            intent: graph.intent.clone(),
            summary,
            risk: risk_level,
            resource_forecast: plan_est.forecast,
            timeline_ms: plan_est.timeline_ms,
            side_effects: effects,
            confidence: prediction,
            fallbacks,
            issues,
            rollback_available,
            optimized_plan: Some(optimized_plan),
            optimizer_diff: Some(optimizer_diff),
            world_before: self.world.clone(),
            world_after: world,
            approval_required,
        })
    }
}
