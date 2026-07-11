//! Aggregate per-step estimates into a plan-level estimate.

use friday_capabilities::cost::CostEstimator;
use friday_capabilities::model::Capability;
use friday_capabilities::sim::{ResourceForecast, StepEstimate};
use friday_capabilities::PlanCost;

/// Plan-level aggregation of cost, resource forecast, and timeline.
#[derive(Debug, Clone)]
pub struct PlanEstimate {
    pub cost: PlanCost,
    pub forecast: ResourceForecast,
    pub timeline_ms: u64,
}

/// Combine the capabilities used, the per-step estimates, and the per-step
/// forecasts into a single plan estimate. Memory/cpu/gpu/tokens take the peak;
/// network/disk are summed; timeline is the sequential sum of latencies.
pub fn aggregate(
    used: &[Capability],
    steps: &[StepEstimate],
    forecasts: &[ResourceForecast],
) -> PlanEstimate {
    let cost = CostEstimator::estimate(used);
    let timeline_ms: u64 = steps.iter().map(|s| s.latency_ms).sum();
    let mut forecast = ResourceForecast::default();
    for f in forecasts {
        forecast = ResourceForecast {
            cpu_pct: forecast.cpu_pct.max(f.cpu_pct),
            memory_mb: forecast.memory_mb.max(f.memory_mb),
            network_bytes: forecast.network_bytes + f.network_bytes,
            disk_bytes: forecast.disk_bytes + f.disk_bytes,
            gpu_pct: forecast.gpu_pct.max(f.gpu_pct),
            tokens: forecast.tokens + f.tokens,
        };
    }
    PlanEstimate {
        cost,
        forecast,
        timeline_ms,
    }
}
