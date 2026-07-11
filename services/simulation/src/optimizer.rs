//! Deterministic plan optimization (M4.7). Pure graph rules — no LLM.
//!
//! Rules implemented:
//! 1. **Prune consecutive duplicates** — two adjacent tasks with identical
//!    capability + params collapse to one (the second is redundant).
//! 2. **Collapse redundant open→close** — an `open(T)` immediately followed by
//!    `close(T)` (same target) with nothing depending on either is removed, since
//!    it is a no-op churn of the same resource.

use friday_capabilities::registry::CapabilityRegistry;
use friday_planner::{Task, TaskGraph};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

/// A description of what the optimizer changed.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct OptimizerDiff {
    pub removed: Vec<String>,
    pub collapsed: Vec<String>,
    pub note: String,
}

fn target_of(params: &Value) -> Option<String> {
    for key in ["app", "name", "url", "path", "target"] {
        if let Some(v) = params.get(key).and_then(|v| v.as_str()) {
            return Some(v.to_string());
        }
    }
    None
}

fn is_open(cap: &str) -> bool {
    cap.ends_with(".open") || cap == "open"
}

fn is_close(cap: &str) -> bool {
    cap.ends_with(".close") || cap == "close"
}

fn depended_on(graph: &TaskGraph, id: Uuid) -> bool {
    graph.tasks.iter().any(|t| t.depends_on.contains(&id))
}

/// Produce an optimized plan and a diff describing the changes.
pub fn optimize(graph: &TaskGraph, _registry: &CapabilityRegistry) -> (TaskGraph, OptimizerDiff) {
    let order = match graph.topo_order() {
        Ok(o) => o,
        Err(_) => {
            return (
                graph.clone(),
                OptimizerDiff {
                    note: "cycle detected: optimizer skipped".into(),
                    ..Default::default()
                },
            )
        }
    };
    let tasks: Vec<&Task> = order.iter().map(|id| graph.task(*id).unwrap()).collect();

    let mut kept: Vec<Task> = Vec::new();
    let mut diff = OptimizerDiff {
        note: "deterministic graph rules applied".into(),
        ..Default::default()
    };
    let mut i = 0;
    while i < tasks.len() {
        let t = tasks[i];

        // Rule 1: prune consecutive duplicate.
        if let Some(prev) = kept.last() {
            if prev.capability == t.capability
                && prev.params == t.params
                && !depended_on(graph, t.id)
            {
                diff.removed.push(format!("pruned duplicate {}", t.capability));
                i += 1;
                continue;
            }
        }

        // Rule 2: collapse open(T) → close(T) with same target, nothing depends on them.
        if is_open(&t.capability) && i + 1 < tasks.len() {
            let nxt = tasks[i + 1];
            if is_close(&nxt.capability)
                && target_of(&t.params) == target_of(&nxt.params)
                && !depended_on(graph, t.id)
                && !depended_on(graph, nxt.id)
            {
                diff.collapsed
                    .push(format!("collapsed {} + {}", t.capability, nxt.capability));
                i += 2;
                continue;
            }
        }

        kept.push(t.clone());
        i += 1;
    }

    let mut optimized = TaskGraph::new(graph.intent.clone());
    optimized.plan_id = graph.plan_id;
    optimized.tasks = kept;
    (optimized, diff)
}
