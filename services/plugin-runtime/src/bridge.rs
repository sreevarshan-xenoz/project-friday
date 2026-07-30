use std::collections::HashMap;

use crate::contract::{CorrelationId, TelemetryEnvelope, TelemetryEvent};
use crate::telemetry::TelemetryObserver;

/// A structured observation produced by the bridge.
///
/// These are the "cognitive translation" of runtime events. They are
/// workspace-compatible but do not depend on the Core 1.0 workspace
/// types. A thin integration layer (outside the frozen core) forwards
/// these into the workspace.
#[derive(Debug, Clone)]
pub struct RuntimeObservation {
    pub kind: ObservationKind,
    pub correlation_id: CorrelationId,
    pub summary: String,
    pub detail: HashMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ObservationKind {
    CapabilityExecuted,
    CapabilityFailed,
    ProviderChosen,
    PolicyApplied,
    RetryHappened,
    HealthDegraded,
    Other,
}

/// Bridges telemetry events into cognitive observations.
///
/// This observer lives in the runtime and transforms telemetry events
/// into structured observations that the cognitive pipeline can consume
/// without the core ever knowing about plugins.
pub struct CognitiveBridge {
    observations: Vec<RuntimeObservation>,
}

impl Default for CognitiveBridge {
    fn default() -> Self {
        Self::new()
    }
}

impl CognitiveBridge {
    pub fn new() -> Self {
        Self {
            observations: Vec::new(),
        }
    }

    /// Translate a telemetry envelope into observations.
    pub fn translate(&self, envelope: &TelemetryEnvelope) -> Vec<RuntimeObservation> {
        match &envelope.event {
            TelemetryEvent::ProviderResolved { plugin_id, confidence, .. } => {
                vec![RuntimeObservation {
                    kind: ObservationKind::ProviderChosen,
                    correlation_id: envelope.correlation_id.clone(),
                    summary: format!("resolved to {} with confidence {:.2}", plugin_id, confidence),
                    detail: map_of![
                        "plugin_id" => plugin_id.clone(),
                        "confidence" => format!("{:.3}", confidence)
                    ],
                }]
            }
            TelemetryEvent::PluginCompleted { plugin_id, duration_ms, success } => {
                let kind = if *success {
                    ObservationKind::CapabilityExecuted
                } else {
                    ObservationKind::CapabilityFailed
                };
                vec![RuntimeObservation {
                    kind,
                    correlation_id: envelope.correlation_id.clone(),
                    summary: format!("{} completed in {}ms (success: {})", plugin_id, duration_ms, success),
                    detail: map_of![
                        "plugin_id" => plugin_id.clone(),
                        "duration_ms" => duration_ms.to_string(),
                        "success" => success.to_string()
                    ],
                }]
            }
            TelemetryEvent::PluginFailed { plugin_id, error, .. } => {
                vec![RuntimeObservation {
                    kind: ObservationKind::CapabilityFailed,
                    correlation_id: envelope.correlation_id.clone(),
                    summary: format!("{} failed: {}", plugin_id, error),
                    detail: map_of![
                        "plugin_id" => plugin_id.clone(),
                        "error" => error.clone()
                    ],
                }]
            }
            TelemetryEvent::PolicyEvaluated { capability, candidates_before, candidates_after } => {
                vec![RuntimeObservation {
                    kind: ObservationKind::PolicyApplied,
                    correlation_id: envelope.correlation_id.clone(),
                    summary: format!("policy filtered {} → {} candidates for {}", candidates_before, candidates_after, capability.as_str()),
                    detail: map_of![
                        "capability" => capability.as_str().to_string(),
                        "before" => candidates_before.to_string(),
                        "after" => candidates_after.to_string()
                    ],
                }]
            }
            TelemetryEvent::RetryAttempted { plugin_id, attempt, .. } => {
                vec![RuntimeObservation {
                    kind: ObservationKind::RetryHappened,
                    correlation_id: envelope.correlation_id.clone(),
                    summary: format!("retry #{} for {}", attempt, plugin_id),
                    detail: map_of![
                        "plugin_id" => plugin_id.clone(),
                        "attempt" => attempt.to_string()
                    ],
                }]
            }
            TelemetryEvent::HealthChanged { plugin_id, old_status, new_status } => {
                let kind = if *new_status == crate::contract::PluginStatus::Failed {
                    ObservationKind::HealthDegraded
                } else {
                    ObservationKind::Other
                };
                vec![RuntimeObservation {
                    kind,
                    correlation_id: envelope.correlation_id.clone(),
                    summary: format!("{} health: {:?} → {:?}", plugin_id, old_status, new_status),
                    detail: map_of![
                        "plugin_id" => plugin_id.clone(),
                        "old_status" => format!("{:?}", old_status),
                        "new_status" => format!("{:?}", new_status)
                    ],
                }]
            }
            _ => vec![],
        }
    }

    /// Get all observations collected so far.
    pub fn observations(&self) -> &[RuntimeObservation] {
        &self.observations
    }

    /// Clear collected observations.
    pub fn clear(&mut self) {
        self.observations.clear();
    }
}

impl TelemetryObserver for CognitiveBridge {
    fn on_event(&self, _envelope: &TelemetryEnvelope) {
        // Observations are collected but not forwarded here — a future
        // integration layer will push them into the workspace.
        // For now, the bridge simply translates and stores.
    }
}

/// Macro to build a HashMap from key-value pairs.
macro_rules! map_of {
    ($($key:expr => $value:expr),* $(,)?) => {{
        let mut m = std::collections::HashMap::new();
        $(
            m.insert($key.into(), $value.into());
        )*
        m
    }};
}

pub(crate) use map_of;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contract::{Capability, CorrelationId, PluginStatus, TelemetryEnvelope};

    #[test]
    fn bridge_translates_provider_resolved() {
        let bridge = CognitiveBridge::new();
        let cid = CorrelationId::new();
        let env = TelemetryEnvelope::new(cid.clone(), TelemetryEvent::ProviderResolved {
            plugin_id: "fs".into(),
            confidence: 0.95,
            candidate_count: 2,
        });
        let obs = bridge.translate(&env);
        assert_eq!(obs.len(), 1);
        assert_eq!(obs[0].kind, ObservationKind::ProviderChosen);
        assert!(obs[0].summary.contains("fs"));
    }

    #[test]
    fn bridge_translates_policy_evaluated() {
        let bridge = CognitiveBridge::new();
        let cid = CorrelationId::new();
        let env = TelemetryEnvelope::new(cid, TelemetryEvent::PolicyEvaluated {
            capability: Capability::ReadFile,
            candidates_before: 3,
            candidates_after: 1,
        });
        let obs = bridge.translate(&env);
        assert_eq!(obs.len(), 1);
        assert_eq!(obs[0].kind, ObservationKind::PolicyApplied);
    }

    #[test]
    fn bridge_translates_health_changed() {
        let bridge = CognitiveBridge::new();
        let cid = CorrelationId::new();
        let env = TelemetryEnvelope::new(cid, TelemetryEvent::HealthChanged {
            plugin_id: "fs".into(),
            old_status: PluginStatus::Healthy,
            new_status: PluginStatus::Failed,
        });
        let obs = bridge.translate(&env);
        assert_eq!(obs.len(), 1);
        assert_eq!(obs[0].kind, ObservationKind::HealthDegraded);
    }

    #[test]
    fn bridge_returns_empty_for_unknown_events() {
        let bridge = CognitiveBridge::new();
        let cid = CorrelationId::new();
        let env = TelemetryEnvelope::new(cid, TelemetryEvent::RuntimeWarning("test".into()));
        let obs = bridge.translate(&env);
        assert!(obs.is_empty());
    }
}
