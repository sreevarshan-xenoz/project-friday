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
use crate::planning::planners::planner_trait::{PlanResult, Planner};
use crate::planning::planners::router::PlannerKind;
use crate::planning::planners::validation::validate_task_graph;
use crate::planning::TaskGraph;
use friday_core::trace::{CognitiveTraceContext, ResultType, SpanId, TraceCollector};
use friday_investigation::InvestigationResult;

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

    /// Format the skill-aware context for the prompt. Returns an empty string
    /// when no skill source was wired or nothing ranked, keeping the prompt
    /// clean. Renders the pure-data [`SkillContext`](friday_skills::SkillContext)
    /// (no prompts, no scores — ranks are encoded by ordering) and marks the
    /// recommended skill so the model plans *through* the skill when it fits.
    ///
    /// **Trust surface:** skill manifest text (name/description/actions) reaches
    /// the planning model, JSON-escaped but still readable — the same trust
    /// class as capability descriptions, and skills are third-party-installable
    /// via Skill Forge. A skill author controls text the planner will read;
    /// documented, not fixed (consistent with the house pattern).
    fn format_skills(ctx: &PlanContext) -> String {
        let skills = match &ctx.skills {
            Some(s) if !s.relevant_skills.is_empty() => s,
            _ => return String::new(),
        };
        let json = serde_json::to_string_pretty(skills).unwrap_or_else(|_| "{}".to_string());
        format!(
            "\nRELEVANT SKILLS (ranked for this goal; `active_skill` is the \
             recommended skill — plan through it when it fits, using the \
             capabilities it declares):\n{json}\n"
        )
    }

    /// Format the investigation outcome for the prompt. Returns an empty
    /// string when no investigation seam was wired, investigation was
    /// skipped/failed, or the goal is self-contained — keeping the prompt
    /// clean. Renders the pure-data [`InvestigationResult`] (channels with
    /// rationale + confidence, no prompts) so the model knows which evidence
    /// channels to consult before executing when the goal genuinely needs
    /// them.
    ///
    /// **Trust surface:** the rationale text (from the heuristic's keyword
    /// scan or the LLM strategy's output) reaches the planning model,
    /// JSON-escaped but still readable — the same trust class as capability
    /// descriptions. The rationale is derived, not user-supplied.
    fn format_investigation(ctx: &PlanContext) -> String {
        let plan = match &ctx.investigation {
            Some(InvestigationResult::Complete(plan)) => plan,
            _ => return String::new(), // None / Skipped / Failed — no evidence section
        };
        if !plan.needs_investigation() {
            return String::new();
        }
        let rows: Vec<String> = plan
            .requirements
            .iter()
            .map(|r| {
                format!(
                    "- {} ({:.2}): {}",
                    r.channel.label(),
                    r.confidence,
                    r.rationale
                )
            })
            .collect();
        format!(
            "\nEVIDENCE REQUIRED (consult these channels before executing; \
             confidence = evidence strength):\n{}\n",
            rows.join("\n")
        )
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
                .map(|w| format!("  - [{}]", w.capabilities.join(", ")))
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
        let skills = Self::format_skills(ctx);
        let investigation = Self::format_investigation(ctx);
        let experience = Self::format_experience(ctx);
        format!(
            "You are the planning module of FRIDAY, an autonomous execution agent.\n\
             Produce a Directed Acyclic Graph of tasks that accomplishes the user's goal.\n\
             Each task MUST use a capability from the provided list. Dependencies are by \
             zero-based index into the tasks array.\n\n\
             GOAL:\n{goal}\n\n\
             {skills}\
             {investigation}\
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
            skills = skills,
            investigation = investigation,
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
        tracer: &dyn TraceCollector,
        trace_ctx: &CognitiveTraceContext,
    ) -> Result<PlanResult, PlanError> {
        // ── Model call ────────────────────────────────────────────────
        let model_span = SpanId::default();
        let model_start = std::time::Instant::now();
        let prompt = self.build_prompt(intent, ctx);
        let completion = self
            .backend
            .complete_with_usage(&prompt)
            .await
            .map_err(PlanError::Backend)?;
        let model_elapsed_ms = model_start.elapsed().as_millis() as u64;
        tracing::debug!(
            raw_len = completion.content.len(),
            "llm: raw response received"
        );

        // Emit model call span with real usage data.
        let mut model_trace = trace_ctx
            .trace(model_span, "planner", "llm_call")
            .with_result(ResultType::Success)
            .with_timing(
                chrono::Utc::now() - chrono::Duration::milliseconds(model_elapsed_ms as i64),
                model_elapsed_ms,
            );
        if let Some(ref usage) = completion.usage {
            model_trace = model_trace
                .with_model_usage(usage.clone())
                .with_result(ResultType::Success);
        }
        tracer.record(model_trace);

        // ── Parse & validate ──────────────────────────────────────────
        let json_str = Self::extract_json(&completion.content)?;
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

        Ok(PlanResult {
            graph,
            usage: completion.usage,
            llm_duration_ms: Some(model_elapsed_ms),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::intent::parse;
    use crate::planning::planners::backend::MockBackend;
    use crate::planning::planners::context::WorkspaceContext;
    use friday_investigation::{InformationRequirement, InvestigationChannel, InvestigationPlan};
    use friday_skills::{SkillContext, SkillSummary};

    fn planner() -> LlmPlanner {
        LlmPlanner::new(
            Box::new(MockBackend::new("{}")),
            Arc::new(CapabilityRegistry::new()),
            ExecutionBudget::default(),
            RiskPolicy::default(),
        )
    }

    fn ctx_with_skills() -> PlanContext {
        PlanContext {
            intent_raw: "summarize the repo".to_string(),
            capabilities: vec![],
            budget: ExecutionBudget::default(),
            workspace: WorkspaceContext::default(),
            risk_policy: RiskPolicy::default(),
            experience: None,
            skills: Some(SkillContext {
                relevant_skills: vec![SkillSummary {
                    id: "git".to_string(),
                    name: "Git".to_string(),
                    version: "1.0.0".to_string(),
                    description: "inspect repositories".to_string(),
                    category: "developer".to_string(),
                    actions: vec!["show status".to_string()],
                    capabilities: vec!["git.status".to_string()],
                    permission_hints: vec!["git.status".to_string()],
                    readonly: true,
                }],
                active_skill: Some("git".to_string()),
            }),
            investigation: None,
        }
    }

    fn ctx_with_investigation() -> PlanContext {
        let mut ctx = ctx_with_skills();
        ctx.investigation = Some(InvestigationResult::Complete(InvestigationPlan {
            requirements: vec![
                InformationRequirement {
                    channel: InvestigationChannel::Web,
                    rationale: "goal mentions 'search the web'".into(),
                    confidence: 0.9,
                },
                InformationRequirement {
                    channel: InvestigationChannel::Git,
                    rationale: "goal mentions 'git'".into(),
                    confidence: 0.8,
                },
            ],
        }));
        ctx
    }

    #[test]
    fn prompt_renders_relevant_skills_section() {
        let prompt = planner().build_prompt(&parse("summarize the repo"), &ctx_with_skills());
        assert!(prompt.contains("RELEVANT SKILLS"));
        assert!(prompt.contains("\"git\""));
        assert!(prompt.contains("active_skill"));
        // Skills land before the capability list (plan through the skill).
        assert!(
            prompt.find("RELEVANT SKILLS").unwrap()
                < prompt.find("AVAILABLE CAPABILITIES").unwrap()
        );
    }

    #[test]
    fn prompt_renders_evidence_required_section() {
        let prompt = planner().build_prompt(
            &parse("search the web and check git"),
            &ctx_with_investigation(),
        );
        assert!(prompt.contains("EVIDENCE REQUIRED"));
        assert!(prompt.contains("web (0.90)"));
        assert!(prompt.contains("git (0.80)"));
        assert!(prompt.contains("goal mentions 'search the web'"));
        // Evidence lands between the skill ranking and the capability list.
        assert!(
            prompt.find("EVIDENCE REQUIRED").unwrap() > prompt.find("RELEVANT SKILLS").unwrap()
        );
        assert!(
            prompt.find("EVIDENCE REQUIRED").unwrap()
                < prompt.find("AVAILABLE CAPABILITIES").unwrap()
        );
    }

    #[test]
    fn prompt_omits_evidence_section_when_absent() {
        let ctx = ctx_with_skills(); // investigation: None
        let prompt = planner().build_prompt(&parse("summarize the repo"), &ctx);
        assert!(!prompt.contains("EVIDENCE REQUIRED"));
    }

    #[test]
    fn prompt_omits_evidence_section_when_self_contained() {
        // An empty plan = goal is self-contained — nothing to consult.
        let mut ctx = ctx_with_skills();
        ctx.investigation = Some(InvestigationResult::Complete(InvestigationPlan::default()));
        let prompt = planner().build_prompt(&parse("open notepad"), &ctx);
        assert!(!prompt.contains("EVIDENCE REQUIRED"));
    }

    #[test]
    fn prompt_omits_evidence_section_when_skipped_or_failed() {
        for result in [
            InvestigationResult::Skipped,
            InvestigationResult::Failed {
                reason: "strategy unavailable".into(),
            },
        ] {
            let mut ctx = ctx_with_skills();
            ctx.investigation = Some(result);
            let prompt = planner().build_prompt(&parse("summarize the repo"), &ctx);
            assert!(!prompt.contains("EVIDENCE REQUIRED"));
        }
    }

    #[test]
    fn prompt_omits_skills_section_when_absent() {
        let mut ctx = ctx_with_skills();
        ctx.skills = None;
        let prompt = planner().build_prompt(&parse("summarize the repo"), &ctx);
        assert!(!prompt.contains("RELEVANT SKILLS"));
    }

    #[test]
    fn prompt_omits_skills_section_when_nothing_ranked() {
        let mut ctx = ctx_with_skills();
        ctx.skills = Some(SkillContext::default());
        let prompt = planner().build_prompt(&parse("summarize the repo"), &ctx);
        assert!(!prompt.contains("RELEVANT SKILLS"));
    }
}
