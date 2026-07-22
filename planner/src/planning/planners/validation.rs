//! Deterministic `TaskGraph` validation. No matter how a plan was produced
//! (rule or LLM), it must pass these checks before the supervisor executes it:
//!
//! * **dependency cycles** — a DAG must be acyclic,
//! * **missing / unsupported capabilities** — every task's capability must
//!   resolve in the registry,
//! * **invalid inputs** — every task has a capability and a non-empty name,
//! * **budget violations** — task count within `max_steps`,
//! * **risk escalation** — high-risk tasks respect the risk policy.
//!
//! Returns `Ok(())` or `Err(PlanError::Validation(Vec<String>))` listing every
//! problem found (all-at-once, not fail-fast) so the caller can report them.

use crate::execution::capability_registry::CapabilityRegistry;
use crate::execution::lifecycle::ExecutionBudget;
use crate::planning::planners::context::RiskPolicy;
use crate::planning::planners::PlanError;
use crate::planning::TaskGraph;

/// Validate a graph against the registry, budget, and risk policy.
pub fn validate_task_graph(
    graph: &TaskGraph,
    registry: &CapabilityRegistry,
    budget: &ExecutionBudget,
    policy: &RiskPolicy,
) -> Result<(), PlanError> {
    let mut issues = Vec::new();

    // --- invalid inputs: name / capability present ---
    for t in &graph.tasks {
        if t.name.trim().is_empty() {
            issues.push(format!("task {} has an empty name", t.id));
        }
        if t.capability.trim().is_empty() {
            issues.push(format!("task {:?} ({}) has no capability", t.id, t.name));
        }
    }

    // --- missing / unsupported capabilities ---
    let known = registry.known();
    for t in &graph.tasks {
        if t.capability.trim().is_empty() {
            continue;
        }
        if !known.contains(&t.capability) {
            issues.push(format!(
                "task '{}' uses unknown/unregistered capability '{}'",
                t.name, t.capability
            ));
        }
    }

    // --- dependency integrity: referenced tasks exist ---
    let ids: std::collections::HashSet<_> = graph.tasks.iter().map(|t| t.id).collect();
    for t in &graph.tasks {
        for d in &t.depends_on {
            if !ids.contains(d) {
                issues.push(format!(
                    "task '{}' depends on missing task id {}",
                    t.name, d
                ));
            }
        }
    }

    // --- budget: task count within max_steps ---
    if let Some(max_steps) = budget.max_steps {
        if graph.tasks.len() as u32 > max_steps {
            issues.push(format!(
                "plan has {} tasks but budget allows {}",
                graph.tasks.len(),
                max_steps
            ));
        }
    }

    // --- risk escalation: high-risk tasks must be permitted by policy ---
    for t in &graph.tasks {
        let risk = registry.risk_of(&t.capability);
        if matches!(risk, crate::execution::capability_registry::Risk::High)
            && !policy.allow_high_risk
        {
            issues.push(format!(
                "task '{}' uses high-risk capability '{}' which policy forbids",
                t.name, t.capability
            ));
        }
    }

    // --- dependency cycles (DAG check) ---
    if let Err(e) = graph.topo_order() {
        issues.push(e);
    }

    if issues.is_empty() {
        Ok(())
    } else {
        Err(PlanError::Validation(issues))
    }
}
