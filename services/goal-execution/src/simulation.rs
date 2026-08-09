use crate::common::assess_risk;
use crate::ActionRisk;
use crate::Capability;
use crate::Plan;
use crate::Risk;
use crate::WorldState;
use serde::{Deserialize, Serialize};

// ── Simulation ─────────────────────────────────────────────────────

/// The result of simulating a plan against a world state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimulationResult {
    /// Whether the simulation completed without contradictions.
    pub feasible: bool,
    /// Projected world state after all actions complete.
    pub projected_world: WorldState,
    /// Overall confidence in the simulation [0.0, 1.0].
    pub confidence: f32,
    /// Per-action risk assessments.
    pub risks: Vec<ActionRisk>,
    /// Aggregate risk level (highest individual risk).
    pub aggregate_risk: Risk,
    /// Estimated cost (normalized 0.0–1.0).
    pub expected_cost: f32,
    /// Estimated duration in milliseconds.
    pub expected_duration_ms: u64,
    /// Preconditions that failed during simulation.
    pub failed_preconditions: Vec<String>,
    /// Postconditions expected to be met.
    pub expected_postconditions: Vec<String>,
    /// Per-step simulation traces.
    pub step_results: Vec<StepResult>,
}

/// The result of simulating a single step.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StepResult {
    pub step_index: usize,
    pub action_id: String,
    pub preconditions_met: bool,
    pub failed_preconditions: Vec<String>,
    pub projected_effects: Vec<String>,
    pub risk: Risk,
}

/// A simulator — predicts plan outcomes without executing.
///
/// ```text
/// Simulator trait:
///   simulate(plan, world) -> SimulationResult
///
/// Never calls tools. Pure prediction.
/// ```
pub trait Simulator: Send + Sync {
    /// Simulate a plan against a world state.
    fn simulate(&self, plan: &Plan, world: &WorldState) -> SimulationResult;
}

/// Default simulator — uses preconditions/postconditions and capability risk.
pub struct DefaultSimulator;

impl DefaultSimulator {
    pub fn new() -> Self {
        Self
    }
}

impl Default for DefaultSimulator {
    fn default() -> Self {
        Self::new()
    }
}

impl Simulator for DefaultSimulator {
    fn simulate(&self, plan: &Plan, world: &WorldState) -> SimulationResult {
        let projected = world.clone();
        let mut all_risks = Vec::new();
        let mut failed_preconditions = Vec::new();
        let mut expected_postconditions = Vec::new();
        let mut step_results = Vec::new();
        let mut feasible = true;
        let mut total_cost: f32 = 0.0;

        for (i, action) in plan.actions.iter().enumerate() {
            let risk = assess_risk(&action.capability);
            all_risks.push(ActionRisk::new(&action.id, risk.risk, &risk.reason));
            total_cost += match risk.risk {
                Risk::Low => 0.1,
                Risk::Medium => 0.3,
                Risk::High => 0.7,
                Risk::Critical => 1.0,
            };

            // Check if capability is available
            if !projected.has_tool(&action.capability) {
                let msg = format!("tool not available: {}", action.capability);
                failed_preconditions.push(msg.clone());
                feasible = false;
                step_results.push(StepResult {
                    step_index: i,
                    action_id: action.id.clone(),
                    preconditions_met: false,
                    failed_preconditions: vec![msg],
                    projected_effects: vec![],
                    risk: risk.risk,
                });
                continue;
            }

            // Simulate the effect: infer what this action would produce
            let effects = infer_effects(&action.capability, &action.instruction);
            for effect in &effects {
                expected_postconditions.push(effect.clone());
            }

            step_results.push(StepResult {
                step_index: i,
                action_id: action.id.clone(),
                preconditions_met: true,
                failed_preconditions: vec![],
                projected_effects: effects,
                risk: risk.risk,
            });
        }

        let aggregate_risk = all_risks.iter().map(|r| r.risk).max().unwrap_or(Risk::Low);

        let confidence = if failed_preconditions.is_empty() {
            0.9
        } else {
            0.3
        };

        let cost = if plan.actions.is_empty() {
            0.0
        } else {
            (total_cost / plan.actions.len() as f32).clamp(0.0, 1.0)
        };

        SimulationResult {
            feasible,
            projected_world: projected,
            confidence,
            risks: all_risks,
            aggregate_risk,
            expected_cost: cost,
            expected_duration_ms: plan.estimated_duration_ms,
            failed_preconditions,
            expected_postconditions,
            step_results,
        }
    }
}

/// Infer the effects of an action based on its capability.
fn infer_effects(capability: &Capability, instruction: &str) -> Vec<String> {
    match capability {
        Capability::ReadFile => vec![format!("read: {}", instruction)],
        Capability::WriteFile => vec![format!("written: {}", instruction)],
        Capability::DeleteFile => vec![format!("deleted: {}", instruction)],
        Capability::ListDirectory => vec![format!("listed: {}", instruction)],
        Capability::ExecuteCommand => vec![format!("executed: {}", instruction)],
        Capability::OpenBrowser => vec![format!("opened: {}", instruction)],
        Capability::SearchWeb => vec![format!("searched: {}", instruction)],
        Capability::SearchMemory => vec![format!("queried memory: {}", instruction)],
        Capability::Custom(name) => vec![format!("custom({}): {}", name, instruction)],
    }
}
