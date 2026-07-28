use crate::recorder::EventRecorder;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use tokio::sync::broadcast;
use uuid::Uuid;

// ── Strongly Typed Identifiers ──────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Topic {
    WindowFocusChanged,
    WindowCurrent,
    ClipboardChanged,
    CpuUsage,
    Mouse,
    Keyboard,
    TickCompleted,
    PhaseStarted,
    PhaseCompleted,
    MemoryStats,
    MemoryStored,
    MemoryConsolidated,
    EngineStarted,
    EngineStopped,
    EngineError,
    MissionStarted,
    MissionCompleted,
    MissionFailed,
    GoalCreated,
    ToolExecuted,
    OutcomeObserved,
    Custom(String),
}

impl<'de> serde::Deserialize<'de> for Topic {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        use serde::de;

        struct TopicVisitor;

        impl<'de> de::Visitor<'de> for TopicVisitor {
            type Value = Topic;

            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                formatter.write_str("a topic string like \"tick.completed\" or \"TickCompleted\"")
            }

            fn visit_str<E>(self, value: &str) -> Result<Topic, E>
            where
                E: de::Error,
            {
                // Try PascalCase enum variant first, then fall back to snake_case string.
                Ok(match value {
                    "WindowFocusChanged" | "sensor.window.focus_changed" => Topic::WindowFocusChanged,
                    "WindowCurrent" | "sensor.window.current" => Topic::WindowCurrent,
                    "ClipboardChanged" | "sensor.clipboard.changed" => Topic::ClipboardChanged,
                    "CpuUsage" | "sensor.cpu.usage" => Topic::CpuUsage,
                    "Mouse" | "sensor.mouse" => Topic::Mouse,
                    "Keyboard" | "sensor.keyboard" => Topic::Keyboard,
                    "TickCompleted" | "cognitive.tick.completed" => Topic::TickCompleted,
                    "PhaseStarted" | "cognitive.phase.started" => Topic::PhaseStarted,
                    "PhaseCompleted" | "cognitive.phase.completed" => Topic::PhaseCompleted,
                    "MemoryStats" | "memory.stats" => Topic::MemoryStats,
                    "MemoryStored" | "memory.stored" => Topic::MemoryStored,
                    "MemoryConsolidated" | "memory.consolidated" => Topic::MemoryConsolidated,
                    "EngineStarted" | "engine.started" => Topic::EngineStarted,
                    "EngineStopped" | "engine.stopped" => Topic::EngineStopped,
                    "EngineError" | "engine.error" => Topic::EngineError,
                    "MissionStarted" | "mission.started" => Topic::MissionStarted,
                    "MissionCompleted" | "mission.completed" => Topic::MissionCompleted,
                    "MissionFailed" | "mission.failed" => Topic::MissionFailed,
                    "GoalCreated" | "goal.created" => Topic::GoalCreated,
                    "ToolExecuted" | "tool.executed" => Topic::ToolExecuted,
                    "OutcomeObserved" | "outcome.observed" => Topic::OutcomeObserved,
                    other => Topic::Custom(other.to_string()),
                })
            }
        }

        deserializer.deserialize_str(TopicVisitor)
    }
}

impl serde::Serialize for Topic {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(self.as_str().as_str())
    }
}

impl Topic {
    pub fn as_str(&self) -> String {
        match self {
            Self::WindowFocusChanged => "sensor.window.focus_changed".into(),
            Self::WindowCurrent => "sensor.window.current".into(),
            Self::ClipboardChanged => "sensor.clipboard.changed".into(),
            Self::CpuUsage => "sensor.cpu.usage".into(),
            Self::Mouse => "sensor.mouse".into(),
            Self::Keyboard => "sensor.keyboard".into(),
            Self::TickCompleted => "cognitive.tick.completed".into(),
            Self::PhaseStarted => "cognitive.phase.started".into(),
            Self::PhaseCompleted => "cognitive.phase.completed".into(),
            Self::MemoryStats => "memory.stats".into(),
            Self::MemoryStored => "memory.stored".into(),
            Self::MemoryConsolidated => "memory.consolidated".into(),
            Self::EngineStarted => "engine.started".into(),
            Self::EngineStopped => "engine.stopped".into(),
            Self::EngineError => "engine.error".into(),
            Self::MissionStarted => "mission.started".into(),
            Self::MissionCompleted => "mission.completed".into(),
            Self::MissionFailed => "mission.failed".into(),
            Self::GoalCreated => "goal.created".into(),
            Self::ToolExecuted => "tool.executed".into(),
            Self::OutcomeObserved => "outcome.observed".into(),
            Self::Custom(s) => s.clone(),
        }
    }

    pub fn parse(s: &str) -> Self {
        match s {
            "sensor.window.focus_changed" => Self::WindowFocusChanged,
            "sensor.window.current" => Self::WindowCurrent,
            "sensor.clipboard.changed" => Self::ClipboardChanged,
            "sensor.cpu.usage" => Self::CpuUsage,
            "sensor.mouse" => Self::Mouse,
            "sensor.keyboard" => Self::Keyboard,
            "cognitive.tick.completed" => Self::TickCompleted,
            "cognitive.phase.started" => Self::PhaseStarted,
            "cognitive.phase.completed" => Self::PhaseCompleted,
            "memory.stats" => Self::MemoryStats,
            "memory.stored" => Self::MemoryStored,
            "memory.consolidated" => Self::MemoryConsolidated,
            "engine.started" => Self::EngineStarted,
            "engine.stopped" => Self::EngineStopped,
            "engine.error" => Self::EngineError,
            "mission.started" => Self::MissionStarted,
            "mission.completed" => Self::MissionCompleted,
            "mission.failed" => Self::MissionFailed,
            "goal.created" => Self::GoalCreated,
            "tool.executed" => Self::ToolExecuted,
            "outcome.observed" => Self::OutcomeObserved,
            other => Self::Custom(other.to_string()),
        }
    }

    pub fn starts_with(&self, prefix: &str) -> bool {
        self.as_str().starts_with(prefix)
    }
}

impl std::fmt::Display for Topic {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum EngineId {
    Perception,
    Memory,
    Planner,
    Learning,
    Reasoning,
    Reflection,
    Runtime,
    ToolSelector,
    Provider,
    WorldModel,
    Security,
    Automation,
    Custom(String),
}

impl<'de> serde::Deserialize<'de> for EngineId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        use serde::de;

        struct EngineIdVisitor;

        impl<'de> de::Visitor<'de> for EngineIdVisitor {
            type Value = EngineId;

            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                formatter.write_str("an engine ID string like \"runtime\" or \"Runtime\"")
            }

            fn visit_str<E>(self, value: &str) -> Result<EngineId, E>
            where
                E: de::Error,
            {
                Ok(match value {
                    "Perception" | "perception" => EngineId::Perception,
                    "Memory" | "memory" => EngineId::Memory,
                    "Planner" | "planner" => EngineId::Planner,
                    "Learning" | "learning" => EngineId::Learning,
                    "Reasoning" | "reasoning" => EngineId::Reasoning,
                    "Reflection" | "reflection" => EngineId::Reflection,
                    "Runtime" | "runtime" => EngineId::Runtime,
                    "ToolSelector" | "tool_selector" => EngineId::ToolSelector,
                    "Provider" | "provider" => EngineId::Provider,
                    "WorldModel" | "world_model" => EngineId::WorldModel,
                    "Security" | "security" => EngineId::Security,
                    "Automation" | "automation" => EngineId::Automation,
                    other => EngineId::Custom(other.to_string()),
                })
            }
        }

        deserializer.deserialize_str(EngineIdVisitor)
    }
}

impl serde::Serialize for EngineId {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl EngineId {
    pub fn as_str(&self) -> &str {
        match self {
            Self::Perception => "perception",
            Self::Memory => "memory",
            Self::Planner => "planner",
            Self::Learning => "learning",
            Self::Reasoning => "reasoning",
            Self::Reflection => "reflection",
            Self::Runtime => "runtime",
            Self::ToolSelector => "tool_selector",
            Self::Provider => "provider",
            Self::WorldModel => "world_model",
            Self::Security => "security",
            Self::Automation => "automation",
            Self::Custom(s) => s,
        }
    }
}

impl std::fmt::Display for EngineId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

impl From<&str> for EngineId {
    fn from(s: &str) -> Self {
        match s {
            "perception" => Self::Perception,
            "memory" => Self::Memory,
            "planner" => Self::Planner,
            "learning" => Self::Learning,
            "reasoning" => Self::Reasoning,
            "reflection" => Self::Reflection,
            "runtime" => Self::Runtime,
            "tool_selector" => Self::ToolSelector,
            "provider" => Self::Provider,
            "world_model" => Self::WorldModel,
            "security" => Self::Security,
            "automation" => Self::Automation,
            other => Self::Custom(other.to_string()),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Priority {
    Low,
    Normal,
    High,
    Critical,
}

impl<'de> serde::Deserialize<'de> for Priority {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        use serde::de;

        struct PriorityVisitor;

        impl<'de> de::Visitor<'de> for PriorityVisitor {
            type Value = Priority;

            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                formatter.write_str("a priority string like \"normal\" or \"Normal\"")
            }

            fn visit_str<E>(self, value: &str) -> Result<Priority, E>
            where
                E: de::Error,
            {
                Ok(match value {
                    "Low" | "low" => Priority::Low,
                    "Normal" | "normal" => Priority::Normal,
                    "High" | "high" => Priority::High,
                    "Critical" | "critical" => Priority::Critical,
                    other => return Err(de::Error::unknown_variant(other, &["low", "normal", "high", "critical"])),
                })
            }
        }

        deserializer.deserialize_str(PriorityVisitor)
    }
}

impl serde::Serialize for Priority {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let s = match self {
            Priority::Low => "low",
            Priority::Normal => "normal",
            Priority::High => "high",
            Priority::Critical => "critical",
        };
        serializer.serialize_str(s)
    }
}

impl Default for Priority {
    fn default() -> Self {
        Self::Normal
    }
}

// ── WorkspaceEvent ──────────────────────────────────────────────────

pub const EVENT_SCHEMA_VERSION: u16 = 1;

fn default_schema_version() -> u16 {
    1
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceEvent {
    #[serde(default = "default_schema_version")]
    pub schema_version: u16,
    pub id: Uuid,
    pub timestamp_ms: u64,
    pub tick_id: Uuid,
    pub topic: Topic,
    pub source: EngineId,
    pub correlation_id: Option<Uuid>,
    pub priority: Priority,
    pub payload: serde_json::Value,
}

impl WorkspaceEvent {
    pub fn new(topic: Topic, payload: serde_json::Value) -> Self {
        Self {
            schema_version: EVENT_SCHEMA_VERSION,
            id: Uuid::new_v4(),
            timestamp_ms: current_epoch_ms(),
            tick_id: Uuid::nil(),
            topic,
            source: EngineId::Runtime,
            correlation_id: None,
            priority: Priority::Normal,
            payload,
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn with_full(
        id: Uuid,
        timestamp_ms: u64,
        tick_id: Uuid,
        topic: Topic,
        source: EngineId,
        correlation_id: Option<Uuid>,
        priority: Priority,
        payload: serde_json::Value,
    ) -> Self {
        Self {
            schema_version: EVENT_SCHEMA_VERSION,
            id, timestamp_ms, tick_id, topic, source, correlation_id, priority, payload
        }
    }

    pub fn source(mut self, source: EngineId) -> Self {
        self.source = source;
        self
    }

    pub fn tick_id(mut self, tick_id: Uuid) -> Self {
        self.tick_id = tick_id;
        self
    }

    pub fn correlation_id(mut self, id: Uuid) -> Self {
        self.correlation_id = Some(id);
        self
    }

    pub fn priority(mut self, priority: Priority) -> Self {
        self.priority = priority;
        self
    }

    pub fn topic_str(&self) -> String {
        self.topic.as_str()
    }
}

// ── GlobalWorkspace ─────────────────────────────────────────────────

#[derive(Clone)]
pub struct GlobalWorkspace {
    inner: Arc<RwLock<WorkspaceInner>>,
}

struct WorkspaceInner {
    current: HashMap<String, WorkspaceEvent>,
    history: Vec<WorkspaceEvent>,
    subscriptions: HashMap<String, Vec<String>>,
    max_history: usize,
    current_tick_id: Uuid,
    recorder: Option<EventRecorder>,
    /// Channel for broadcasting events to subscribers.
    event_tx: broadcast::Sender<WorkspaceEvent>,
}

impl GlobalWorkspace {
    pub fn new() -> Self {
        let (event_tx, _) = broadcast::channel(256);
        Self {
            inner: Arc::new(RwLock::new(WorkspaceInner {
                current: HashMap::new(),
                history: Vec::new(),
                subscriptions: HashMap::new(),
                max_history: 10_000,
                current_tick_id: Uuid::nil(),
                recorder: None,
                event_tx,
            })),
        }
    }

    pub fn with_recorder(dir: &std::path::Path) -> Result<Self, String> {
        let recorder = EventRecorder::new(dir)
            .map_err(|e| format!("Failed to create recorder at {:?}: {}", dir, e))?;
        let (event_tx, _) = broadcast::channel(256);
        Ok(Self {
            inner: Arc::new(RwLock::new(WorkspaceInner {
                current: HashMap::new(),
                history: Vec::new(),
                subscriptions: HashMap::new(),
                max_history: 10_000,
                current_tick_id: Uuid::nil(),
                recorder: Some(recorder),
                event_tx,
            })),
        })
    }

    pub fn set_current_tick(&self, tick_id: Uuid) {
        self.inner.write().unwrap().current_tick_id = tick_id;
    }

    pub fn publish(&self, topic: Topic, payload: serde_json::Value) -> Uuid {
        let tick_id = self.inner.read().unwrap().current_tick_id;
        let event = WorkspaceEvent::new(topic, payload).tick_id(tick_id);
        let id = event.id;
        self.publish_event(event);
        id
    }

    pub fn publish_from(
        &self,
        topic: Topic,
        source: EngineId,
        payload: serde_json::Value,
    ) -> Uuid {
        let tick_id = self.inner.read().unwrap().current_tick_id;
        let event = WorkspaceEvent::new(topic, payload).source(source).tick_id(tick_id);
        let id = event.id;
        self.publish_event(event);
        id
    }

    pub fn publish_event(&self, event: WorkspaceEvent) {
        let topic_str = event.topic.as_str();
        let mut inner = self.inner.write().unwrap();
        inner.current.insert(topic_str, event.clone());
        inner.history.push(event.clone());
        if inner.history.len() > inner.max_history {
            let drain = inner.history.len() - inner.max_history;
            inner.history.drain(..drain);
        }
        if let Some(ref mut recorder) = inner.recorder {
            let _ = recorder.append(&event);
        }
        // Broadcast to subscribers. Ignore error (no active receivers).
        let _ = inner.event_tx.send(event);
    }

    pub fn get(&self, topic: &str) -> Option<WorkspaceEvent> {
        self.inner.read().unwrap().current.get(topic).cloned()
    }

    pub fn get_latest(&self, prefix: &str) -> Option<WorkspaceEvent> {
        let inner = self.inner.read().unwrap();
        inner.history.iter().rev().find(|e| e.topic.starts_with(prefix)).cloned()
    }

    pub fn snapshot(&self) -> HashMap<String, WorkspaceEvent> {
        self.inner.read().unwrap().current.clone()
    }

    pub fn history(&self, topic_prefix: Option<&str>) -> Vec<WorkspaceEvent> {
        let inner = self.inner.read().unwrap();
        match topic_prefix {
            Some(prefix) => inner.history.iter().filter(|e| e.topic.starts_with(prefix)).cloned().collect(),
            None => inner.history.clone(),
        }
    }

    pub fn register_subscriber(&self, topic_prefix: &str, subscriber: &str) {
        let mut inner = self.inner.write().unwrap();
        inner.subscriptions.entry(topic_prefix.to_string()).or_default().push(subscriber.to_string());
    }

    pub fn subscribers_for(&self, topic: &str) -> Vec<String> {
        let inner = self.inner.read().unwrap();
        inner.subscriptions.iter()
            .filter(|(prefix, _)| topic.starts_with(prefix.as_str()))
            .flat_map(|(_, subs)| subs.clone())
            .collect()
    }

    /// Subscribe to the event stream. Returns a receiver that gets every
    /// event as it's published. The consumer filters by topic prefix.
    ///
    /// ```ignore
    /// let mut rx = workspace.subscribe("sensor.");
    /// while let Ok(event) = rx.recv().await {
    ///     if event.topic.starts_with("sensor.") {
    ///         // handle event
    ///     }
    /// }
    /// ```
    pub fn subscribe(&self) -> broadcast::Receiver<WorkspaceEvent> {
        self.inner.read().unwrap().event_tx.subscribe()
    }

    pub fn clear(&self) {
        let mut inner = self.inner.write().unwrap();
        inner.current.clear();
        inner.history.clear();
    }

    pub fn history_len(&self) -> usize {
        self.inner.read().unwrap().history.len()
    }

    pub fn topic_count(&self) -> usize {
        self.inner.read().unwrap().current.len()
    }
}

impl Default for GlobalWorkspace {
    fn default() -> Self {
        Self::new()
    }
}

// ── Helpers ─────────────────────────────────────────────────────────

fn current_epoch_ms() -> u64 {
    Utc::now().timestamp_millis() as u64
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn publish_and_get() {
        let ws = GlobalWorkspace::new();
        ws.publish(Topic::Mouse, json!({"x": 100, "y": 200}));

        let event = ws.get("sensor.mouse").unwrap();
        assert_eq!(event.topic, Topic::Mouse);
        assert_eq!(event.payload, json!({"x": 100, "y": 200}));
        assert!(!event.id.is_nil());
        assert!(event.timestamp_ms > 0);
    }

    #[test]
    fn get_latest_by_prefix() {
        let ws = GlobalWorkspace::new();
        ws.publish(Topic::Mouse, json!({"x": 1}));
        ws.publish(Topic::Keyboard, json!({"key": "a"}));

        let latest = ws.get_latest("sensor.").unwrap();
        assert_eq!(latest.topic, Topic::Keyboard);
    }

    #[test]
    fn snapshot_returns_all_current_topics() {
        let ws = GlobalWorkspace::new();
        ws.publish(Topic::Custom("a".into()), json!(1));
        ws.publish(Topic::Custom("b".into()), json!(2));

        let snap = ws.snapshot();
        assert_eq!(snap.len(), 2);
    }

    #[test]
    fn history_filtering() {
        let ws = GlobalWorkspace::new();
        ws.publish(Topic::Mouse, json!(1));
        ws.publish(Topic::Custom("b.y".into()), json!(2));

        let a_history = ws.history(Some("sensor.mouse"));
        assert_eq!(a_history.len(), 1);
    }

    #[test]
    fn subscribe_and_query() {
        let ws = GlobalWorkspace::new();
        ws.register_subscriber("sensor.", "perception_engine");
        ws.register_subscriber("planner.", "planning_engine");

        let subs = ws.subscribers_for("sensor.mouse");
        assert!(subs.contains(&"perception_engine".to_string()));
        assert!(!subs.contains(&"planning_engine".to_string()));
    }

    #[test]
    fn history_pruning() {
        let ws = GlobalWorkspace::new();
        ws.inner.write().unwrap().max_history = 5;

        for i in 0..10 {
            ws.publish(Topic::Custom(format!("event.{}", i)), json!(i));
        }

        assert_eq!(ws.history_len(), 5);
    }

    #[test]
    fn topic_as_str_roundtrip() {
        let topic = Topic::WindowFocusChanged;
        let s = topic.as_str();
        let parsed = Topic::parse(&s);
        assert_eq!(topic, parsed);
    }

    #[test]
    fn topic_custom_roundtrip() {
        let topic = Topic::Custom("my.custom.topic".into());
        let s = topic.as_str();
        assert_eq!(s, "my.custom.topic");
        let parsed = Topic::parse(&s);
        assert_eq!(topic, parsed);
    }

    #[test]
    fn workspace_event_has_timestamp() {
        let ws = GlobalWorkspace::new();
        let id = ws.publish(Topic::TickCompleted, json!({"tick": 1}));
        let event = ws.history(None).into_iter().find(|e| e.id == id).unwrap();
        assert!(event.timestamp_ms > 0);
    }

    #[test]
    fn publish_from_sets_source() {
        let ws = GlobalWorkspace::new();
        ws.publish_from(Topic::Mouse, EngineId::Perception, json!({"x": 1}));
        let event = ws.get("sensor.mouse").unwrap();
        assert_eq!(event.source, EngineId::Perception);
    }

    #[test]
    fn topic_display() {
        assert_eq!(Topic::TickCompleted.to_string(), "cognitive.tick.completed");
        assert_eq!(Topic::Mouse.to_string(), "sensor.mouse");
    }

    #[test]
    fn engine_id_from_str() {
        assert_eq!(EngineId::from("perception"), EngineId::Perception);
        assert_eq!(EngineId::from("memory"), EngineId::Memory);
        assert_eq!(EngineId::from("unknown"), EngineId::Custom("unknown".into()));
    }

    #[test]
    fn topic_starts_with() {
        assert!(Topic::Mouse.starts_with("sensor."));
        assert!(!Topic::TickCompleted.starts_with("sensor."));
    }

    #[test]
    fn backward_compat_old_jsonl_without_schema_version() {
        // Simulate an event written before schema_version existed.
        let old_json = r#"{"id":"00000000-0000-0000-0000-000000000001","timestamp_ms":1000,"tick_id":"00000000-0000-0000-0000-000000000002","topic":"tick.completed","source":"runtime","correlation_id":null,"priority":"normal","payload":{"tick":1}}"#;
        let event: WorkspaceEvent = serde_json::from_str(old_json).unwrap();
        assert_eq!(event.schema_version, 1, "Old events default to schema_version 1");
        assert_eq!(event.id, uuid::Uuid::from_u128(1));
    }
}
