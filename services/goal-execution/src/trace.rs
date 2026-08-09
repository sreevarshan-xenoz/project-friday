use serde::{Deserialize, Serialize};

use super::WorldState;

// ── Cognitive Trace ─────────────────────────────────────────────────

/// The kind of a trace node.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TraceNodeKind {
    /// The Investigation Engine's decision: what the goal required knowing
    /// before execution (channels, evidence confidence, self-contained?).
    Investigation,
    Planning,
    Evaluation,
    Execution,
    Verification,
    Reflection,
    Learning,
    WorldStateChange,
}

/// A single node in a cognitive trace — an immutable record of one decision.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraceNode {
    pub id: String,
    pub kind: TraceNodeKind,
    pub parent_id: Option<String>,
    pub input: serde_json::Value,
    pub output: serde_json::Value,
    pub world_state_snapshot: Option<WorldState>,
    pub confidence: Option<f32>,
    pub policies_used: Vec<String>,
    pub started_at_ms: u64,
    pub finished_at_ms: u64,
}

impl TraceNode {
    pub fn new(kind: TraceNodeKind, input: serde_json::Value, output: serde_json::Value) -> Self {
        let now = chrono::Utc::now().timestamp_millis() as u64;
        Self {
            id: format!("node_{}", uuid::Uuid::new_v4()),
            kind,
            parent_id: None,
            input,
            output,
            world_state_snapshot: None,
            confidence: None,
            policies_used: Vec::new(),
            started_at_ms: now,
            finished_at_ms: now,
        }
    }

    pub fn with_parent(mut self, parent_id: impl Into<String>) -> Self {
        self.parent_id = Some(parent_id.into());
        self
    }

    pub fn with_world_state(mut self, state: WorldState) -> Self {
        self.world_state_snapshot = Some(state);
        self
    }

    pub fn with_confidence(mut self, confidence: f32) -> Self {
        self.confidence = Some(confidence.clamp(0.0, 1.0));
        self
    }

    pub fn with_policies(mut self, policies: Vec<String>) -> Self {
        self.policies_used = policies;
        self
    }

    pub fn with_timing(mut self, started_at_ms: u64, finished_at_ms: u64) -> Self {
        self.started_at_ms = started_at_ms;
        self.finished_at_ms = finished_at_ms;
        self
    }

    pub fn duration_ms(&self) -> u64 {
        self.finished_at_ms.saturating_sub(self.started_at_ms)
    }
}

/// A complete cognitive trace — the full audit trail for a goal execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CognitiveTrace {
    pub goal_id: String,
    pub nodes: Vec<TraceNode>,
}

impl CognitiveTrace {
    pub fn new(goal_id: impl Into<String>) -> Self {
        Self {
            goal_id: goal_id.into(),
            nodes: Vec::new(),
        }
    }

    pub fn push(&mut self, node: TraceNode) {
        self.nodes.push(node);
    }

    pub fn nodes_of_kind(&self, kind: &TraceNodeKind) -> Vec<&TraceNode> {
        self.nodes.iter().filter(|n| n.kind == *kind).collect()
    }

    pub fn total_duration_ms(&self) -> u64 {
        self.nodes.iter().map(|n| n.duration_ms()).sum()
    }

    /// Format the trace as a human-readable tree.
    pub fn display(&self) -> String {
        let mut lines = Vec::new();
        lines.push(format!("Goal: {}", self.goal_id));
        for (i, node) in self.nodes.iter().enumerate() {
            let prefix = if i == self.nodes.len() - 1 {
                "└── "
            } else {
                "├── "
            };
            let confidence_str = node
                .confidence
                .map(|c| format!(" [confidence: {:.2}]", c))
                .unwrap_or_default();
            let duration_str = format!(" ({}ms)", node.duration_ms());
            lines.push(format!(
                "{}{:?}{}{}",
                prefix, node.kind, confidence_str, duration_str
            ));
        }
        lines.join("\n")
    }
}

// ── Trace Recorder ─────────────────────────────────────────────────

/// A trace event — emitted by engines, assembled by TraceRecorder.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TraceEvent {
    /// Investigation finished — what the goal required knowing first.
    InvestigationFinished {
        goal_id: String,
        requirement_count: usize,
        /// Channel labels in evidence order (strongest first).
        channels: Vec<String>,
        /// Mean evidence confidence across the requirements.
        confidence: f32,
        /// True when the goal needs no evidence gathering.
        self_contained: bool,
    },
    PlanningStarted {
        goal_id: String,
    },
    PlanGenerated {
        goal_id: String,
        candidate_count: usize,
    },
    SimulationStarted {
        goal_id: String,
        plan_id: String,
    },
    SimulationFinished {
        goal_id: String,
        plan_id: String,
        feasible: bool,
        confidence: f32,
    },
    EvaluationStarted {
        goal_id: String,
    },
    EvaluationFinished {
        goal_id: String,
        chosen_plan_id: String,
        score: f32,
    },
    ExecutionStarted {
        goal_id: String,
        action_id: String,
    },
    ExecutionFinished {
        goal_id: String,
        action_id: String,
        success: bool,
    },
    VerificationStarted {
        goal_id: String,
        action_id: String,
    },
    VerificationFinished {
        goal_id: String,
        action_id: String,
        verified: bool,
    },
    ReflectionStarted {
        goal_id: String,
    },
    ReflectionFinished {
        goal_id: String,
        insight_count: usize,
    },
    WorldStateChanged {
        goal_id: String,
        transition_id: String,
    },
}

/// A trace recorder — assembles a CognitiveTrace from emitted events.
///
/// ```text
/// Engines emit TraceEvents
///         │
///         ▼
/// TraceRecorder
///         │
///         ▼
///   CognitiveTrace
/// ```
///
/// This decouples tracing from execution logic.
pub struct TraceRecorder {
    trace: CognitiveTrace,
    event_log: Vec<TraceEvent>,
}

impl TraceRecorder {
    pub fn new(goal_id: impl Into<String>) -> Self {
        Self {
            trace: CognitiveTrace::new(goal_id),
            event_log: Vec::new(),
        }
    }

    /// Record a trace event and update the cognitive trace.
    pub fn record(&mut self, event: TraceEvent) {
        self.event_log.push(event.clone());

        let node = match &event {
            TraceEvent::InvestigationFinished {
                requirement_count,
                channels,
                confidence,
                self_contained,
                ..
            } => TraceNode::new(
                TraceNodeKind::Investigation,
                serde_json::json!({ "self_contained": self_contained }),
                serde_json::json!({
                    "requirement_count": requirement_count,
                    "channels": channels,
                    "confidence": confidence,
                }),
            )
            .with_confidence(*confidence),
            TraceEvent::PlanningStarted { .. } => TraceNode::new(
                TraceNodeKind::Planning,
                serde_json::json!({}),
                serde_json::json!({}),
            ),
            TraceEvent::PlanGenerated {
                candidate_count, ..
            } => TraceNode::new(
                TraceNodeKind::Planning,
                serde_json::json!({}),
                serde_json::json!({ "candidate_count": candidate_count }),
            ),
            TraceEvent::SimulationStarted { plan_id, .. } => TraceNode::new(
                TraceNodeKind::Execution,
                serde_json::json!({ "plan_id": plan_id }),
                serde_json::json!({}),
            ),
            TraceEvent::SimulationFinished {
                feasible,
                confidence,
                ..
            } => TraceNode::new(
                TraceNodeKind::Execution,
                serde_json::json!({}),
                serde_json::json!({ "feasible": feasible, "confidence": confidence }),
            )
            .with_confidence(*confidence),
            TraceEvent::EvaluationFinished { score, .. } => TraceNode::new(
                TraceNodeKind::Evaluation,
                serde_json::json!({}),
                serde_json::json!({ "score": score }),
            )
            .with_confidence(*score),
            TraceEvent::ExecutionStarted { action_id, .. } => TraceNode::new(
                TraceNodeKind::Execution,
                serde_json::json!({ "action_id": action_id }),
                serde_json::json!({}),
            ),
            TraceEvent::ExecutionFinished { success, .. } => TraceNode::new(
                TraceNodeKind::Execution,
                serde_json::json!({}),
                serde_json::json!({ "success": success }),
            ),
            TraceEvent::VerificationFinished { verified, .. } => TraceNode::new(
                TraceNodeKind::Verification,
                serde_json::json!({}),
                serde_json::json!({ "verified": verified }),
            ),
            TraceEvent::ReflectionFinished { insight_count, .. } => TraceNode::new(
                TraceNodeKind::Reflection,
                serde_json::json!({}),
                serde_json::json!({ "insight_count": insight_count }),
            ),
            _ => TraceNode::new(
                TraceNodeKind::Planning,
                serde_json::json!({}),
                serde_json::json!({}),
            ),
        };

        self.trace.push(node);
    }

    /// Get the assembled cognitive trace.
    pub fn trace(&self) -> &CognitiveTrace {
        &self.trace
    }

    /// Consume the recorder and return the trace.
    pub fn into_trace(self) -> CognitiveTrace {
        self.trace
    }

    /// Get the raw event log.
    pub fn events(&self) -> &[TraceEvent] {
        &self.event_log
    }
}
