//! Host system probe (M6.2a).
//!
//! Publishes **real** host metrics (CPU / RAM / Disk) onto the FRIDAY event bus so
//! the mobile dashboard shows genuine numbers on day one — not a simulated panel.
//! Runs on a fixed interval; the bus bridge forwards these as `telemetry.system`
//! events to connected phones.

use std::time::Duration;

use friday_core::event::{Event, EventBus};
use sysinfo::{Disks, System};

/// Interval between telemetry samples.
const SAMPLE_INTERVAL: Duration = Duration::from_secs(2);

/// A single host telemetry sample (real measurements from `sysinfo`).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SystemSample {
    pub cpu_percent: f32,
    pub memory_used_mb: u64,
    pub memory_total_mb: u64,
    pub disk_used_mb: u64,
    pub disk_total_mb: u64,
}

/// Periodically samples host resources and publishes them on the bus.
pub struct SystemProbe {
    bus: EventBus,
}

impl SystemProbe {
    pub fn new(bus: EventBus) -> Self {
        Self { bus }
    }

    /// Sample once (used by tests and by [`Self::run`]).
    pub fn sample() -> SystemSample {
        let mut sys = System::new_all();
        sys.refresh_all();

        let cpu = sys
            .cpus()
            .iter()
            .map(|c| c.cpu_usage())
            .fold(0.0_f32, |a, b| a + b)
            / sys.cpus().len().max(1) as f32;

        let memory_used_mb = sys.used_memory() / 1024 / 1024;
        let memory_total_mb = sys.total_memory() / 1024 / 1024;

        let disks = Disks::new_with_refreshed_list();
        let (mut used, mut total) = (0u64, 0u64);
        for d in &disks {
            total += d.total_space();
            used += d.total_space().saturating_sub(d.available_space());
        }
        let disk_used_mb = used / 1024 / 1024;
        let disk_total_mb = total / 1024 / 1024;

        SystemSample {
            cpu_percent: cpu,
            memory_used_mb,
            memory_total_mb,
            disk_used_mb,
            disk_total_mb,
        }
    }

    /// Run forever, publishing a sample every [`SAMPLE_INTERVAL`].
    pub async fn run(self) {
        let mut ticker = tokio::time::interval(SAMPLE_INTERVAL);
        tracing::info!("system probe started");
        loop {
            ticker.tick().await;
            let s = Self::sample();
            let payload = serde_json::to_value(&s).expect("system sample serializes");
            let _ = self
                .bus
                .publish(Event::new("telemetry.system", "system-probe", payload))
                .await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sample_is_real() {
        let s = SystemProbe::sample();
        // Memory is a hard, measurable fact on the host; it must be non-zero.
        assert!(s.memory_total_mb > 0, "host must report total memory");
        assert!(s.memory_used_mb > 0, "host must report used memory");
        assert!(s.disk_total_mb > 0, "host must report disk space");
        assert!((0.0..=100.0).contains(&s.cpu_percent));
    }
}
