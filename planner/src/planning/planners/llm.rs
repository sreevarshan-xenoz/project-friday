//! The model-driven planner. It turns an [`Intent`] + [`PlanContext`] into a
//! `TaskGraph` by asking a [`ReasoningBackend`] for a *structured* plan and then
//! validating it. It never lets the model emit an arbitrary graph: the output
//! is parsed into an intermediate shape, rebuilt with fresh UUIDs and
//! index→id dependency resolution, and run through the same deterministic
//! [`validate_task_graph`] the rule planner would face.

use async_trait::async_trait;
use serde::Deserialize;
use std::sync::Arc;

use crate::execution::capability_registry::CapabilityRegistry;
use crate::execution::lifecycle::ExecutionBudget;
use crate::experience::GoalOutcome;
use crate::intent::Intent;
use crate::planning::planners::backend::{PlanError, ReasoningBackend};
use crate::planning::planners::context::{PlanContext, RiskPolicy};
use crate::planning::planners::planner_trait::Planner;
use crate::planning::planners::router::PlannerKind;
use crate::planning::planners::validation::validate_task_graph;
use crate::planning::TaskGraph;

/// One task as the model is asked to emit it. Dependencies are by *index* into
/// the `tasks` array (stable across serialization); the planner maps them to
/// real UUIDs afterwards.
#[derive(Debug, Clone, Deserialize)]
struct LlmTask {
    name: String,
    capability: String,
    #[serde(default)]
    params: serde_json::Value,
    #[serde(default)]
    depends_on_indices: Vec<usize>,
    #[serde(default)]
    produces: Option<String>,
}

/// The required structured output shape.
#[derive(Debug, Clone, Deserialize)]
struct LlmPlan {
    #[serde(default)]
    tasks: Vec<LlmTask>,
    /// Declared artifacts (informational; reflected in telemetry, not executed).
    #[serde(default)]
    #[allow(dead_code)]
    artifacts: Vec<String>,
    /// Model-estimated cost (informational; recorded for budgeting later).
    #[serde(default)]
    #[allow(dead_code)]
    estimated_cost: f64,
}

/// Model-driven planner. Holds the backend plus the grounding it needs to
/// validate (registry, default budget, policy). The backend is injected — the
/// planner knows nothing about providers.
pub struct LlmPlanner {
    backend: Box<dyn ReasoningBackend>,
    registry: Arc<CapabilityRegistry>,
    budget: ExecutionBudget,
    policy: RiskPolicy,
}

impl LlmPlanner {
    pub fn new(
        backend: Box<dyn ReasoningBackend>,
        registry: Arc<CapabilityRegistry>,
        budget: ExecutionBudget,
        policy: RiskPolicy,
    ) -> Self {
        Self {
            backend,
            registry,
            budget,
            policy,
        }
    }

    /// Format experience evidence for the prompt. Returns an empty string
    /// when no experience is available, keeping the prompt clean.
    fn format_experience(ctx: &PlanContext) -> String {
        let view = ctx.evidence();
        if !view.is_available() {
            return String::new();
        }

        let mut sections: Vec<String> = Vec::new();

        // Previous attempt at this exact goal.
        if let Some(summary) = view.previous_outcome_summary() {
            sections.push(format!("- {summary}"));
        }

        // Successful workflows from similar goals.
        if let Some(exp) = ctx.experience.as_ref() {
            let workflows: Vec<String> = exp
                .evidence
                .successful_workflows
                .iter()
                .filter(|w| w.outcome == GoalOutcome::Success)
                .map(|w| {
                    format!("  - [{}]", w.capabilities.join(", "))
                })
                .collect();
            if !workflows.is_empty() {
                sections.push("Successful past workflows:".into());
                sections.extend(workflows);
            }

            // Reflections from similar goals (limit to 3 most relevant).
            let reflections: Vec<String> = exp
                .evidence
                .reflections
                .iter()
                .take(3)
                .map(|r| format!("  - \"{}\"", r.insight))
                .collect();
            if !reflections.is_empty() {
                sections.push("Lessons from similar goals:".into());
                sections.extend(reflections);
            }
        }

        if sections.is_empty() {
            return String::new();
        }

        format!(
            "\nEXPERIENCE EVIDENCE (from past executions — use as guidance, not requirement):\n{}\n",
            sections.join("\n")
        )
    }

    fn build_prompt(&self, intent: &Intent, ctx: &PlanContext) -> String {
        let caps = serde_json::to_string_pretty(&ctx.capabilities_json())
            .unwrap_or_else(|_| "[]".to_string());
        let budget = serde_json::json!({
            "max_steps": self.budget.max_steps,
            "max_retries": self.budget.max_retries,
            "timeout_minutes": self.budget.timeout.map(|d| d.num_minutes()),
        });
        let risk_policy = serde_json::json!({
            "allow_high_risk": self.policy.allow_high_risk,
            "require_approval_for_high": self.policy.require_approval_for_high,
        });
        let workspace = serde_json::json!({
            "cwd": ctx.workspace.cwd,
            "git_branch": ctx.workspace.git_branch,
            "project_kind": ctx.workspace.project_kind,
        });
        let experience = Self::format_experience(ctx);
        format!(
            "You are the planning module of FRIDAY, an autonomous execution agent.\n\
             Produce a Directed Acyclic Graph of tasks that accomplishes the user's goal.\n\
             Each task MUST use a capability from the provided list. Dependencies are by \
             zero-based index into the tasks array.\n\n\
             GOAL:\n{goal}\n\n\
             {experience}\
             AVAILABLE CAPABILITIES (canonical id, description, risk):\n{caps}\n\n\
             EXECUTION BUDGET:\n{budget}\n\n\
             WORKSPACE CONTEXT:\n{workspace}\n\n\
             RISK POLICY:\n{risk_policy}\n\n\
             Respond with ONLY a JSON object of this exact shape (no prose, no code fences):\n\
             {{\n  \"tasks\": [\n    {{\"name\": str, \"capability\": str, \"params\": {{}}, \
             \"depends_on_indices\": [int], \"produces\": str? }}\n  ],\n  \
             \"artifacts\": [str],\n  \"estimated_cost\": float\n}}\n",
            goal = intent.raw,
            experience = experience,
            caps = caps,
            budget = budget,
            workspace = workspace,
            risk_policy = risk_policy,
        )
    }

    /// Extract the first JSON object from a model response, tolerating code
    /// fences and surrounding prose.
    fn extract_json(text: &str) -> Result<&str, PlanError> {
        let trimmed = text.trim();
        let body = if let Some(stripped) = trimmed
            .strip_prefix("```json")
            .or_else(|| trimmed.strip_prefix("```"))
        {
            stripped
                .trim()
                .strip_suffix("```")
                .unwrap_or(stripped)
                .trim()
        } else {
            trimmed
        };
        // Find the outermost {...} span.
        let start = match body.find('{') {
            Some(i) => i,
            None => return Err(PlanError::Parse("no JSON object found in response".into())),
        };
        let mut depth = 0i32;
        let mut end = None;
        for (i, c) in body[start..].char_indices() {
            match c {
                '{' => depth += 1,
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        end = Some(start + i + 1);
                        break;
                    }
                }
                _ => {}
            }
        }
        match end {
            Some(e) => Ok(&body[start..e]),
            None => Err(PlanError::Parse("unbalanced braces in response".into())),
        }
    }

    fn build_graph(&self, plan: &LlmPlan) -> TaskGraph {
        let mut graph = TaskGraph::new("llm-plan");
        // First pass: assign fresh ids, remember index→id mapping.
        let mut ids: Vec<_> = Vec::with_capacity(plan.tasks.len());
        for t in &plan.tasks {
            let params = if t.params.is_object() {
                t.params.clone()
            } else {
                serde_json::json!({})
            };
            let id = graph.add_task(&t.name, &t.capability, params, &[]);
            ids.push(id);
        }
        // Second pass: wire dependencies by index.
        for (idx, t) in plan.tasks.iter().enumerate() {
            let my_id = ids[idx];
            let deps: Vec<_> = t
                .depends_on_indices
                .iter()
                .filter_map(|&d| ids.get(d).copied())
                .collect();
            if let Some(task) = graph.task_mut(my_id) {
                task.depends_on = deps;
                task.produces = t.produces.clone();
            }
        }
        graph
    }
}

#[async_trait]
impl Planner for LlmPlanner {
    fn kind(&self) -> PlannerKind {
        PlannerKind::Llm
    }

    async fn create_plan(
        &self,
        intent: &Intent,
        ctx: &PlanContext,
    ) -> Result<TaskGraph, PlanError> {
        let prompt = self.build_prompt(intent, ctx);
        let raw = self
            .backend
            .complete(&prompt)
            .await
            .map_err(PlanError::Backend)?;
        tracing::debug!(raw_len = raw.len(), "llm: raw response received");
        let json_str = Self::extract_json(&raw)?;
        tracing::debug!(json_len = json_str.len(), "llm: extracted JSON plan");
        let plan: LlmPlan =
            serde_json::from_str(json_str).map_err(|e| PlanError::Parse(e.to_string()))?;
        if plan.tasks.is_empty() {
            return Err(PlanError::Validation(vec![
                "model returned a plan with zero tasks".into(),
            ]));
        }
        let graph = self.build_graph(&plan);
        validate_task_graph(&graph, &self.registry, &self.budget, &self.policy)?;
        Ok(graph)
    }
}
