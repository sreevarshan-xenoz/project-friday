//! Metrics: automatic, per-execution measurement.
//!
//! Every execution records timing, retries, the plugin used, success, and
//! (placeholders for) resource usage. The data makes the platform answerable
//! with facts: "why is the browser slower than the terminal?" becomes a query
//! over [`Metrics::avg_execution_ms`] rather than a guess.

use std::collections::VecDeque;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// A single measured execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricSample {
    pub execution_id: Uuid,
    pub planning_ms: u128,
    pub queue_ms: u128,
    pub execution_ms: u128,
    pub retry_count: u32,
    pub plugin_used: String,
    pub success: bool,
    /// Memory used by the action, in bytes (best-effort; 0 until wired).
    pub memory_bytes: u64,
    /// CPU time consumed, in milliseconds (best-effort; 0 until wired).
    pub cpu_ms: u64,
}

impl Default for MetricSample {
    fn default() -> Self {
        Self {
            execution_id: Uuid::new_v4(),
            planning_ms: 0,
            queue_ms: 0,
            execution_ms: 0,
            retry_count: 0,
            plugin_used: String::new(),
            success: false,
            memory_bytes: 0,
            cpu_ms: 0,
        }
    }
}

/// Bounded ring of recent samples plus aggregate queries.
pub struct Metrics {
    samples: Mutex<VecDeque<MetricSample>>,
    capacity: usize,
}

impl Default for Metrics {
    fn default() -> Self {
        Self::new(4096)
    }
}

impl Metrics {
    pub fn new(capacity: usize) -> Self {
        Self {
            samples: Mutex::new(VecDeque::with_capacity(capacity)),
            capacity,
        }
    }

    pub fn record(&self, sample: MetricSample) {
        let mut guard = self.samples.lock().expect("metrics poisoned");
        if guard.len() >= self.capacity {
            guard.pop_front();
        }
        guard.push_back(sample);
    }

    pub fn recent(&self, n: usize) -> Vec<MetricSample> {
        let guard = self.samples.lock().expect("metrics poisoned");
        guard.iter().rev().take(n).rev().cloned().collect()
    }

    /// Fraction of samples that succeeded, in `[0.0, 1.0]`.
    pub fn success_rate(&self) -> f64 {
        let guard = self.samples.lock().expect("metrics poisoned");
        if guard.is_empty() {
            return 0.0;
        }
        let ok = guard.iter().filter(|s| s.success).count();
        ok as f64 / guard.len() as f64
    }

    /// Every sample recorded for a given plugin.
    pub fn by_plugin(&self, plugin: &str) -> Vec<MetricSample> {
        let guard = self.samples.lock().expect("metrics poisoned");
        guard
            .iter()
            .filter(|s| s.plugin_used == plugin)
            .cloned()
            .collect()
    }

    /// Average execution time (ms) for a plugin — the basis for "browser vs
    /// terminal" comparisons. Returns 0.0 when no samples exist.
    pub fn avg_execution_ms(&self, plugin: &str) -> f64 {
        let samples = self.by_plugin(plugin);
        if samples.is_empty() {
            return 0.0;
        }
        let total: u128 = samples.iter().map(|s| s.execution_ms).sum();
        total as f64 / samples.len() as f64
    }

    pub fn count(&self) -> usize {
        self.samples.lock().expect("metrics poisoned").len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(plugin: &str, ms: u128, success: bool) -> MetricSample {
        MetricSample {
            execution_id: Uuid::new_v4(),
            plugin_used: plugin.to_string(),
            execution_ms: ms,
            success,
            ..Default::default()
        }
    }

    #[test]
    fn success_rate_and_averages() {
        let m = Metrics::new(16);
        m.record(sample("terminal", 10, true));
        m.record(sample("terminal", 20, true));
        m.record(sample("browser", 100, false));

        assert!((m.success_rate() - 2.0 / 3.0).abs() < 1e-9);
        assert!((m.avg_execution_ms("terminal") - 15.0).abs() < 1e-9);
        assert!((m.avg_execution_ms("browser") - 100.0).abs() < 1e-9);
        assert_eq!(m.by_plugin("terminal").len(), 2);
    }
}
