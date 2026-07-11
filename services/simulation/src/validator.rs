//! Pre-flight validation of a plan before simulation/execution.

use friday_capabilities::registry::CapabilityRegistry;
use friday_planner::TaskGraph;
use serde::{Deserialize, Serialize};

/// Severity of a validation issue. `Error` means the plan cannot proceed as-is;
/// `Warning` means it can, but the user should be told.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum IssueSeverity {
    Error,
    Warning,
}

impl IssueSeverity {
    pub fn as_str(&self) -> &'static str {
        match self {
            IssueSeverity::Error => "error",
            IssueSeverity::Warning => "warning",
        }
    }
}

/// A single validation finding.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationIssue {
    pub kind: String,
    pub message: String,
    pub severity: IssueSeverity,
}

impl ValidationIssue {
    pub fn error(kind: &str, message: impl Into<String>) -> Self {
        Self {
            kind: kind.into(),
            message: message.into(),
            severity: IssueSeverity::Error,
        }
    }

    pub fn warning(kind: &str, message: impl Into<String>) -> Self {
        Self {
            kind: kind.into(),
            message: message.into(),
            severity: IssueSeverity::Warning,
        }
    }
}

/// Result of validating a plan.
#[derive(Debug, Clone)]
pub struct ValidationResult {
    pub ok: bool,
    pub issues: Vec<ValidationIssue>,
}

/// Structural validation: cycle, impossible actions, unavailable/dependency
/// failures. Precondition-level findings are added by the engine during
/// simulation (they need the `WorldState` + `Simulatable` preconditions).
pub fn validate(graph: &TaskGraph, registry: &CapabilityRegistry) -> ValidationResult {
    let mut issues = Vec::new();

    if graph.topo_order().is_err() {
        issues.push(ValidationIssue::error(
            "circular_plan",
            "task graph contains a cycle",
        ));
    }

    for task in &graph.tasks {
        let cap = &task.capability;
        let tool = cap.split('.').next().unwrap_or(cap);
        let has = !registry.ranked_providers_for(cap).is_empty()
            || !registry.ranked_providers_for(tool).is_empty();
        if !has {
            if task.depends_on.is_empty() {
                issues.push(ValidationIssue::error(
                    "impossible_action",
                    format!("no provider for capability '{cap}'"),
                ));
            } else {
                issues.push(ValidationIssue::error(
                    "dependency_failure",
                    format!("dependency capability '{cap}' is unavailable"),
                ));
            }
        }
    }

    let ok = !issues.iter().any(|i| i.severity == IssueSeverity::Error);
    ValidationResult { ok, issues }
}
