use async_trait::async_trait;
use friday_cognitive::WorkspaceEvent;

#[cfg(target_os = "windows")]
pub mod windows_focus;

/// Platform-neutral sensor trait.
///
/// Sensors observe the environment and produce workspace events.
/// The cognitive system never needs to know which platform it's running on.
///
/// # Implementors
///
/// - `MockSensor` — deterministic events for tests
/// - `WindowsFocusSensor` — Win32 active window tracking (feature-gated)
#[async_trait]
pub trait Sensor: Send {
    /// Human-readable name, e.g. "windows_focus", "clipboard".
    fn name(&self) -> &str;

    /// Topic prefix this sensor produces, e.g. "sensor.window".
    fn topic_prefix(&self) -> &str;

    /// Poll the environment and return any new events.
    /// Empty vec means nothing changed since last poll.
    async fn poll(&mut self) -> Vec<WorkspaceEvent>;
}

/// Polls a set of sensors and publishes changes to a workspace.
pub struct SensorLoop {
    sensors: Vec<Box<dyn Sensor>>,
}

impl Default for SensorLoop {
    fn default() -> Self {
        Self::new()
    }
}

impl SensorLoop {
    pub fn new() -> Self {
        Self {
            sensors: Vec::new(),
        }
    }

    pub fn add(&mut self, sensor: Box<dyn Sensor>) {
        self.sensors.push(sensor);
    }

    /// Poll all sensors and publish events to the workspace.
    /// Returns total events published this cycle.
    pub async fn poll_all(
        &mut self,
        workspace: &friday_cognitive::workspace::GlobalWorkspace,
    ) -> usize {
        let mut total = 0;
        for sensor in &mut self.sensors {
            let events = sensor.poll().await;
            for event in events {
                workspace.publish_event(event);
                total += 1;
            }
        }
        total
    }

    pub fn sensor_count(&self) -> usize {
        self.sensors.len()
    }
}

// ── MockSensor (for tests) ──────────────────────────────────────────

/// Deterministic sensor for testing. Returns a fixed sequence of events,
/// then stops.
pub struct MockSensor {
    name: String,
    events: Vec<WorkspaceEvent>,
    index: usize,
}

impl MockSensor {
    pub fn new(name: &str, events: Vec<WorkspaceEvent>) -> Self {
        Self {
            name: name.to_string(),
            events,
            index: 0,
        }
    }
}

#[async_trait]
impl Sensor for MockSensor {
    fn name(&self) -> &str {
        &self.name
    }

    fn topic_prefix(&self) -> &str {
        "sensor.mock"
    }

    async fn poll(&mut self) -> Vec<WorkspaceEvent> {
        if self.index < self.events.len() {
            let event = self.events[self.index].clone();
            self.index += 1;
            vec![event]
        } else {
            Vec::new()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use friday_cognitive::workspace::Topic;

    #[tokio::test]
    async fn mock_sensor_returns_events_then_stops() {
        let events = vec![
            WorkspaceEvent::new(Topic::WindowFocusChanged, serde_json::json!({"title": "a"})),
            WorkspaceEvent::new(Topic::WindowFocusChanged, serde_json::json!({"title": "b"})),
        ];
        let mut sensor = MockSensor::new("test", events);

        let batch1 = sensor.poll().await;
        assert_eq!(batch1.len(), 1);
        assert_eq!(batch1[0].payload["title"], "a");

        let batch2 = sensor.poll().await;
        assert_eq!(batch2.len(), 1);
        assert_eq!(batch2[0].payload["title"], "b");

        let batch3 = sensor.poll().await;
        assert!(batch3.is_empty());
    }

    #[tokio::test]
    async fn sensor_loop_polls_all_sensors() {
        let events_a = vec![WorkspaceEvent::new(
            Topic::WindowFocusChanged,
            serde_json::json!({"from": "a"}),
        )];
        let events_b = vec![WorkspaceEvent::new(
            Topic::Mouse,
            serde_json::json!({"from": "b"}),
        )];

        let mut loop_ = SensorLoop::new();
        loop_.add(Box::new(MockSensor::new("sensor_a", events_a)));
        loop_.add(Box::new(MockSensor::new("sensor_b", events_b)));

        assert_eq!(loop_.sensor_count(), 2);

        let workspace = friday_cognitive::workspace::GlobalWorkspace::new();
        let count = loop_.poll_all(&workspace).await;
        assert_eq!(count, 2);

        let history = workspace.history(None);
        assert_eq!(history.len(), 2);
    }
}
