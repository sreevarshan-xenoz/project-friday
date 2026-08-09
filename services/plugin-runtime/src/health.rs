use std::collections::HashMap;

use crate::contract::{ExecutionTelemetry, PluginHealth, PluginStatus};

/// Owns health metrics for all registered plugins.
///
/// Plugins report telemetry per execution. The monitor accumulates
/// rolling metrics that the resolver and planner can consume.
pub struct HealthMonitor {
    metrics: HashMap<String, PluginHealth>,
}

impl Default for HealthMonitor {
    fn default() -> Self {
        Self::new()
    }
}

impl HealthMonitor {
    pub fn new() -> Self {
        Self {
            metrics: HashMap::new(),
        }
    }

    /// Register a new plugin for health tracking.
    pub fn register(&mut self, plugin_id: &str) {
        self.metrics
            .entry(plugin_id.to_string())
            .or_insert_with(|| PluginHealth {
                plugin_id: plugin_id.to_string(),
                status: PluginStatus::Installed,
                availability: 1.0,
                reliability: 1.0,
                average_latency_ms: 0.0,
                success_rate: 1.0,
                total_executions: 0,
                failure_count: 0,
                last_error: None,
                last_execution_ms: None,
            });
    }

    /// Mark a plugin as initialized and healthy.
    pub fn mark_initialized(&mut self, plugin_id: &str) {
        if let Some(h) = self.metrics.get_mut(plugin_id) {
            h.status = PluginStatus::Initialized;
        }
    }

    /// Mark a plugin as healthy (ready to accept work).
    pub fn mark_healthy(&mut self, plugin_id: &str) {
        if let Some(h) = self.metrics.get_mut(plugin_id) {
            h.status = PluginStatus::Healthy;
        }
    }

    /// Mark a plugin as busy.
    pub fn mark_busy(&mut self, plugin_id: &str) {
        if let Some(h) = self.metrics.get_mut(plugin_id) {
            h.status = PluginStatus::Busy;
        }
    }

    /// Mark a plugin as failed.
    pub fn mark_failed(&mut self, plugin_id: &str, error: &str) {
        if let Some(h) = self.metrics.get_mut(plugin_id) {
            h.status = PluginStatus::Failed;
            h.last_error = Some(error.to_string());
        }
    }

    /// Record telemetry from a completed execution and update rolling metrics.
    pub fn record_execution(&mut self, plugin_id: &str, telem: &ExecutionTelemetry) {
        let now_ms = crate::utils::now_ms();

        if let Some(h) = self.metrics.get_mut(plugin_id) {
            let latency = telem.duration_ms() as f64;

            // Exponential moving average for latency (α = 0.3)
            if h.total_executions == 0 {
                h.average_latency_ms = latency;
            } else {
                h.average_latency_ms = 0.7 * h.average_latency_ms + 0.3 * latency;
            }

            h.total_executions += 1;
            h.last_execution_ms = Some(now_ms);

            if telem.success {
                let n = h.total_executions as f64;
                h.success_rate = ((n - 1.0) * h.success_rate + 1.0) / n;
                h.reliability = h.success_rate;
                h.availability = 0.9 * h.availability + 0.1 * h.success_rate;
            } else {
                h.failure_count += 1;
                let n = h.total_executions as f64;
                h.success_rate = ((n - 1.0) * h.success_rate + 0.0) / n;
                h.reliability = h.success_rate;
                h.availability = 0.9 * h.availability + 0.1 * h.success_rate;
            }

            h.status = PluginStatus::Healthy;
        }
    }

    /// Record an execution failure with error details.
    pub fn record_failure(&mut self, plugin_id: &str, error: &str) {
        let now_ms = crate::utils::now_ms();
        if let Some(h) = self.metrics.get_mut(plugin_id) {
            h.failure_count += 1;
            h.total_executions += 1;
            let n = h.total_executions as f64;
            h.success_rate = ((n - 1.0) * h.success_rate) / n;
            h.reliability = h.success_rate;
            h.availability = 0.9 * h.availability + 0.1 * 0.0;
            h.last_error = Some(error.to_string());
            h.last_execution_ms = Some(now_ms);
            h.status = PluginStatus::Failed;
        }
    }

    /// Get the current health snapshot for a plugin.
    pub fn health_of(&self, plugin_id: &str) -> Option<&PluginHealth> {
        self.metrics.get(plugin_id)
    }

    /// Get health snapshots for all registered plugins.
    pub fn all_health(&self) -> Vec<&PluginHealth> {
        self.metrics.values().collect()
    }

    /// Get health snapshots for all plugins that support a given capability.
    pub fn healthy_for(&self, plugin_ids: &[String]) -> Vec<&PluginHealth> {
        plugin_ids
            .iter()
            .filter_map(|id| {
                self.metrics.get(id).and_then(|h| {
                    if h.status == PluginStatus::Healthy || h.status == PluginStatus::Initialized {
                        Some(h)
                    } else {
                        None
                    }
                })
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contract::{CorrelationId, ExecutionTelemetry};

    fn cid() -> CorrelationId {
        CorrelationId::from_string("test-health")
    }

    #[test]
    fn monitor_tracks_initial_health() {
        let mut m = HealthMonitor::new();
        m.register("fs");
        let h = m.health_of("fs").unwrap();
        assert_eq!(h.status, PluginStatus::Installed);
        assert_eq!(h.total_executions, 0);
    }

    #[test]
    fn monitor_updates_after_success() {
        let mut m = HealthMonitor::new();
        m.register("fs");
        m.mark_healthy("fs");

        let telem = ExecutionTelemetry {
            started_at_ms: 1000,
            finished_at_ms: 1500,
            plugin_id: "fs".to_string(),
            success: true,
            correlation_id: cid(),
        };
        m.record_execution("fs", &telem);

        let h = m.health_of("fs").unwrap();
        assert_eq!(h.total_executions, 1);
        assert_eq!(h.failure_count, 0);
        assert!((h.average_latency_ms - 500.0).abs() < 0.1);
        assert!((h.success_rate - 1.0).abs() < 0.01);
    }

    #[test]
    fn monitor_updates_after_failure() {
        let mut m = HealthMonitor::new();
        m.register("fs");
        m.mark_healthy("fs");

        let telem = ExecutionTelemetry {
            started_at_ms: 2000,
            finished_at_ms: 2500,
            plugin_id: "fs".to_string(),
            success: false,
            correlation_id: cid(),
        };
        m.record_execution("fs", &telem);

        let h = m.health_of("fs").unwrap();
        assert_eq!(h.total_executions, 1);
        assert_eq!(h.failure_count, 1);
        assert!((h.success_rate - 0.0).abs() < 0.01);
    }

    #[test]
    fn healthy_for_filters_by_status() {
        let mut m = HealthMonitor::new();
        m.register("fs");
        m.register("net");
        m.mark_healthy("fs");
        // net stays at Installed — not healthy

        let healthy = m.healthy_for(&["fs".to_string(), "net".to_string()]);
        assert_eq!(healthy.len(), 1);
        assert_eq!(healthy[0].plugin_id, "fs");
    }

    #[test]
    fn multiple_executions_rolling_average() {
        let mut m = HealthMonitor::new();
        m.register("fs");
        m.mark_healthy("fs");

        for i in 0..3 {
            let telem = ExecutionTelemetry {
                started_at_ms: i * 1000,
                finished_at_ms: i * 1000 + 200,
                plugin_id: "fs".to_string(),
                success: true,
                correlation_id: cid(),
            };
            m.record_execution("fs", &telem);
        }

        let h = m.health_of("fs").unwrap();
        assert_eq!(h.total_executions, 3);
        assert!((h.success_rate - 1.0).abs() < 0.01);
    }

    #[test]
    fn record_failure_sets_status_and_error() {
        let mut m = HealthMonitor::new();
        m.register("fs");
        m.mark_healthy("fs");
        m.record_failure("fs", "disk full");

        let h = m.health_of("fs").unwrap();
        assert_eq!(h.status, PluginStatus::Failed);
        assert_eq!(h.last_error.as_deref(), Some("disk full"));
        assert_eq!(h.failure_count, 1);
    }
}
