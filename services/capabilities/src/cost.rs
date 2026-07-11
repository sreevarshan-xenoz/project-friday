//! Cost Estimator (M4.6.5 companion).
//!
//! Before a plan runs, estimate its cost from the capabilities it would invoke:
//! time, memory, expected success, risk, token cost, and network usage. This
//! lets FRIDAY answer "this will take ~2 minutes and needs approval" — and lets
//! Dry Run (M4.7) reason about the *current system state*, not just static
//! feasibility.

use serde::{Deserialize, Serialize};

use crate::model::Capability;

/// Aggregate cost of executing a plan (a sequence of capability steps).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PlanCost {
    /// Estimated wall-clock time in milliseconds (sequential sum of latencies).
    pub estimated_ms: u64,
    /// Estimated peak memory in MB (heuristic: 16 MB per step).
    pub estimated_memory_mb: u64,
    /// Combined probability the whole plan succeeds (product of reliabilities).
    pub expected_success: f64,
    /// Risk of failure, 0..1 (1 - expected_success).
    pub risk: f64,
    /// Estimated LLM/context token cost (heuristic: 100 tokens per step).
    pub token_cost: u64,
    /// Estimated network bytes (only steps requiring `network`).
    pub network_usage: u64,
}

/// Combines capability metadata into a plan-level estimate.
pub struct CostEstimator;

impl CostEstimator {
    pub fn estimate(steps: &[Capability]) -> PlanCost {
        if steps.is_empty() {
            return PlanCost::default();
        }
        let estimated_ms = steps.iter().map(|c| c.latency_ms).sum();
        let estimated_memory_mb = steps.len() as u64 * 16;
        let expected_success = steps
            .iter()
            .map(|c| c.reliability.clamp(0.0, 1.0))
            .product::<f64>()
            .clamp(0.0, 1.0);
        let risk = (1.0 - expected_success).clamp(0.0, 1.0);
        let token_cost = steps.len() as u64 * 100;
        let network_usage = steps
            .iter()
            .filter(|c| c.resource_requirements.iter().any(|r| r == "network"))
            .map(|_| 1_000_000u64)
            .sum();
        PlanCost {
            estimated_ms,
            estimated_memory_mb,
            expected_success,
            risk,
            token_cost,
            network_usage,
        }
    }
}
