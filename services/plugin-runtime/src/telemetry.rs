use std::collections::VecDeque;

use crate::contract::{CorrelationId, TelemetryEnvelope, TelemetryEvent};

const DEFAULT_CAPACITY: usize = 10_000;

/// An in-memory ring buffer of telemetry events.
///
/// Events are retained up to `capacity` then the oldest are dropped.
/// This is the runtime's "flight recorder" — raw history for queries,
/// dashboards, and the cognitive bridge.
pub struct TelemetryStore {
    events: VecDeque<TelemetryEnvelope>,
    capacity: usize,
}

impl TelemetryStore {
    pub fn new() -> Self {
        Self {
            events: VecDeque::with_capacity(DEFAULT_CAPACITY),
            capacity: DEFAULT_CAPACITY,
        }
    }

    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            events: VecDeque::with_capacity(capacity),
            capacity,
        }
    }

    /// Push an event. Drops oldest if at capacity.
    pub fn push(&mut self, envelope: TelemetryEnvelope) {
        if self.events.len() >= self.capacity {
            self.events.pop_front();
        }
        self.events.push_back(envelope);
    }

    /// All stored events.
    pub fn all(&self) -> &VecDeque<TelemetryEnvelope> {
        &self.events
    }

    /// All events for a given correlation ID.
    pub fn for_correlation(&self, id: &CorrelationId) -> Vec<&TelemetryEnvelope> {
        self.events
            .iter()
            .filter(|e| e.correlation_id == *id)
            .collect()
    }

    /// All events of a given variant.
    pub fn of_type(&self, kind: &str) -> Vec<&TelemetryEnvelope> {
        self.events
            .iter()
            .filter(|e| e.event.variant_name() == kind)
            .collect()
    }

    /// Number of stored events.
    pub fn count(&self) -> usize {
        self.events.len()
    }

    /// Clear all events.
    pub fn clear(&mut self) {
        self.events.clear();
    }
}

impl Default for TelemetryStore {
    fn default() -> Self {
        Self::new()
    }
}

/// Passive collector that receives telemetry events and stores them.
///
/// The collector is owned by the runtime and has no active role in
/// execution — it observes and records. Nothing depends on it.
pub struct TelemetryCollector {
    store: TelemetryStore,
    observers: Vec<Box<dyn TelemetryObserver>>,
}

impl TelemetryCollector {
    pub fn new() -> Self {
        Self {
            store: TelemetryStore::new(),
            observers: Vec::new(),
        }
    }

    /// Register an observer that receives every event.
    pub fn add_observer(&mut self, observer: Box<dyn TelemetryObserver>) {
        self.observers.push(observer);
    }

    /// Emit a telemetry event.
    pub fn emit(&mut self, envelope: TelemetryEnvelope) {
        // Notify observers first so they see the event
        for observer in &self.observers {
            observer.on_event(&envelope);
        }
        self.store.push(envelope);
    }

    /// Convenience: build and emit a simple event.
    pub fn emit_event(&mut self, correlation_id: CorrelationId, event: TelemetryEvent) {
        self.emit(TelemetryEnvelope::new(correlation_id, event));
    }

    /// Access the event store.
    pub fn store(&self) -> &TelemetryStore {
        &self.store
    }

    /// Access the event store mutably (for tests).
    pub fn store_mut(&mut self) -> &mut TelemetryStore {
        &mut self.store
    }
}

impl Default for TelemetryCollector {
    fn default() -> Self {
        Self::new()
    }
}

/// Observer that receives every telemetry event as it is emitted.
///
/// Implementations must not block execution. The bridge from telemetry
/// to the cognitive pipeline is one such observer.
pub trait TelemetryObserver: Send + Sync {
    fn on_event(&self, envelope: &TelemetryEnvelope);
}

impl TelemetryEvent {
    /// A short, stable name for this event variant (for filtering).
    pub fn variant_name(&self) -> &'static str {
        match self {
            Self::CapabilityRequested(_) => "capability.requested",
            Self::ProviderResolved { .. } => "provider.resolved",
            Self::PluginInvoked { .. } => "plugin.invoked",
            Self::PluginCompleted { .. } => "plugin.completed",
            Self::PluginFailed { .. } => "plugin.failed",
            Self::HealthChanged { .. } => "health.changed",
            Self::PolicyEvaluated { .. } => "policy.evaluated",
            Self::ResolverScored { .. } => "resolver.scored",
            Self::RetryAttempted { .. } => "retry.attempted",
            Self::RuntimeWarning(_) => "runtime.warning",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contract::{Capability, TelemetryEvent};

    #[test]
    fn store_retains_events() {
        let mut collector = TelemetryCollector::new();
        let cid = CorrelationId::new();
        collector.emit_event(cid.clone(), TelemetryEvent::RuntimeWarning("test".into()));
        assert_eq!(collector.store().count(), 1);
    }

    #[test]
    fn store_evicts_oldest_at_capacity() {
        let mut store = TelemetryStore::with_capacity(3);
        let cid = CorrelationId::new();
        for i in 0..5 {
            store.push(TelemetryEnvelope::new(
                cid.clone(),
                TelemetryEvent::RuntimeWarning(format!("event {}", i)),
            ));
        }
        assert_eq!(store.count(), 3);
        // Oldest (0, 1) should be gone
        let all: Vec<&TelemetryEnvelope> = store.all().iter().collect();
        for env in all {
            if let TelemetryEvent::RuntimeWarning(msg) = &env.event {
                assert!(!msg.contains("event 0"));
                assert!(!msg.contains("event 1"));
            }
        }
    }

    #[test]
    fn for_correlation_filters_correctly() {
        let mut store = TelemetryStore::new();
        let cid_a = CorrelationId::from_string("a");
        let cid_b = CorrelationId::from_string("b");
        store.push(TelemetryEnvelope::new(
            cid_a.clone(),
            TelemetryEvent::RuntimeWarning("a1".into()),
        ));
        store.push(TelemetryEnvelope::new(
            cid_b.clone(),
            TelemetryEvent::RuntimeWarning("b1".into()),
        ));
        store.push(TelemetryEnvelope::new(
            cid_a.clone(),
            TelemetryEvent::RuntimeWarning("a2".into()),
        ));

        assert_eq!(store.for_correlation(&cid_a).len(), 2);
        assert_eq!(store.for_correlation(&cid_b).len(), 1);
    }

    #[test]
    fn of_type_filters_by_variant() {
        let mut collector = TelemetryCollector::new();
        let cid = CorrelationId::new();
        collector.emit_event(
            cid.clone(),
            TelemetryEvent::CapabilityRequested(Capability::ReadFile),
        );
        collector.emit_event(cid.clone(), TelemetryEvent::RuntimeWarning("hello".into()));
        assert_eq!(collector.store().of_type("capability.requested").len(), 1);
        assert_eq!(collector.store().of_type("runtime.warning").len(), 1);
    }

    #[test]
    fn variant_name_matches_event_kind() {
        let e = TelemetryEvent::CapabilityRequested(Capability::ReadFile);
        assert_eq!(e.variant_name(), "capability.requested");
        let e = TelemetryEvent::ProviderResolved {
            plugin_id: "p".into(),
            confidence: 0.9,
            candidate_count: 3,
        };
        assert_eq!(e.variant_name(), "provider.resolved");
        let e = TelemetryEvent::PluginInvoked {
            plugin_id: "p".into(),
            capability: Capability::ReadFile,
        };
        assert_eq!(e.variant_name(), "plugin.invoked");
    }

    #[test]
    fn observer_receives_events() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::sync::Arc;

        let counter = Arc::new(AtomicUsize::new(0));
        let c = counter.clone();
        struct CountingObserver(Arc<AtomicUsize>);
        impl TelemetryObserver for CountingObserver {
            fn on_event(&self, _: &TelemetryEnvelope) {
                self.0.fetch_add(1, Ordering::SeqCst);
            }
        }

        let mut collector = TelemetryCollector::new();
        collector.add_observer(Box::new(CountingObserver(c)));
        let cid = CorrelationId::new();
        collector.emit_event(cid.clone(), TelemetryEvent::RuntimeWarning("test".into()));
        collector.emit_event(
            cid,
            TelemetryEvent::CapabilityRequested(Capability::ReadFile),
        );
        assert_eq!(counter.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn correlation_id_generates_unique_values() {
        let a = CorrelationId::new();
        let b = CorrelationId::new();
        assert_ne!(a, b);
    }

    #[test]
    fn correlation_id_display() {
        let id = CorrelationId::from_string("hello-123");
        assert_eq!(format!("{}", id), "hello-123");
    }
}
