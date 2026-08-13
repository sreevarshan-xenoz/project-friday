//! [`SkillAgentDriver`] — the bridge between the Agent Runtime and the Skill
//! Engine: a worker goal is planned by the skill engine *scoped to the
//! agent's allowed skills*, then executed through the same kernel every other
//! action flows through.
//!
//! **The planner orchestrates, the model never owns the architecture.** The
//! driver is fully deterministic — it uses the skill engine's ranking, not an
//! LLM, so a worker's behavior is reproducible and testable. A worker can only
//! ever see the skills in its `AgentSpec::allowed_skills` (plus their
//! dependencies), and it only executes capabilities the scoped policy
//! auto-approves (readonly skill covering a safe capability) or that the
//! session has approved. High-risk capabilities are gated exactly like the
//! supervisor gates them — a worker never executes high-risk work.
//!
//! This driver lives in the action-runtime crate (not friday-context) because
//! `friday-context` is downstream of `friday-actions` via `friday-mcp`; the
//! host (`e2e`, a console) owns the skills/approvals registries and passes
//! them in.

use std::collections::HashMap;
use std::sync::Arc;

use chrono::Utc;
use friday_agent_runtime::{AgentContext, AgentDriver, AgentResult, AgentStep};
use friday_planner::execution::capability_registry::CapabilityRegistry as PlannerCapabilityRegistry;
use friday_planner::execution::Risk;
use friday_skills::{PermissionState, RiskResolver, SkillApprovals, SkillPolicy, SkillRegistry};
use serde_json::Value;
use uuid::Uuid;

use crate::kernel::context::{CancellationToken, ExecutionContext};
use crate::request::ActionRequest;
use crate::runtime::AgentKernel;

/// Maps the planner registry's [`Risk`] classification to the Skill Engine's
/// safe/unsafe question (`Risk::Safe` → safe) — the same mapping friday-context
/// applies, without importing it (dependency direction).
struct PlannerRiskResolver(Arc<PlannerCapabilityRegistry>);

impl RiskResolver for PlannerRiskResolver {
    fn is_safe(&self, capability: &str) -> bool {
        self.0.risk_of(capability) == Risk::Safe
    }
}

/// Resolves capability ids against the planner registry — lets the driver
/// re-resolve every skill's status live (a capability bound or removed at
/// runtime is reflected before the worker plans, instead of a load-time
/// snapshot).
struct PlannerRegistryResolver(Arc<PlannerCapabilityRegistry>);

impl friday_skills::CapabilityResolver for PlannerRegistryResolver {
    fn exists(&self, capability: &str) -> bool {
        self.0.resolve(capability).is_some()
    }
}

/// Splits a canonical capability id (`web.fetch`) into `(tool, action)`.
fn split_capability(capability: &str) -> (String, String) {
    match capability.split_once('.') {
        Some((tool, action)) => (tool.to_string(), action.to_string()),
        None => (capability.to_string(), String::new()),
    }
}

/// A deterministic, skill-scoped worker driver.
pub struct SkillAgentDriver {
    registry: Arc<SkillRegistry>,
    approvals: Arc<SkillApprovals>,
    planner_registry: Arc<PlannerCapabilityRegistry>,
    kernel: Arc<AgentKernel>,
}

impl SkillAgentDriver {
    pub fn new(
        registry: Arc<SkillRegistry>,
        approvals: Arc<SkillApprovals>,
        planner_registry: Arc<PlannerCapabilityRegistry>,
        kernel: Arc<AgentKernel>,
    ) -> Self {
        Self {
            registry,
            approvals,
            planner_registry,
            kernel,
        }
    }
}

#[async_trait::async_trait]
impl AgentDriver for SkillAgentDriver {
    async fn run(&self, ctx: AgentContext) -> AgentResult {
        let spec = ctx.spec.clone();
        // A worker cancelled/timed-out while queued should not pay the plan
        // cost at all — surface the abort before any work.
        if let Err(reason) = ctx.check() {
            return AgentResult::aborted(
                spec.id.clone(),
                reason,
                "aborted before planning",
                vec![],
                0,
            );
        }

        // Re-resolve skill capability statuses against the live registry so a
        // capability bound/removed since load is reflected now.
        self.registry
            .reconcile(&PlannerRegistryResolver(Arc::clone(&self.planner_registry)));

        // 1. Scope: the agent physically cannot see skills outside its allowed
        //    set (plus their dependency closure).
        let allowed: Vec<&str> = spec.allowed_skills.iter().map(String::as_str).collect();
        let scoped = Arc::new(self.registry.subset(&allowed));
        let risk: Arc<dyn RiskResolver> =
            Arc::new(PlannerRiskResolver(Arc::clone(&self.planner_registry)));
        let policy = SkillPolicy::new(Arc::clone(&scoped), Arc::clone(&self.approvals), risk);

        // 2. The planner selects deterministically within the scope.
        let Some(selected) = scoped.select_for(&spec.goal, 8) else {
            return AgentResult::failed(
                spec.id.clone(),
                format!(
                    "no usable skill within allowed scope {:?} for goal '{}'",
                    spec.allowed_skills, spec.goal
                ),
                vec![],
                "no_selectable_skill",
            );
        };
        let chosen_id = selected.skill.id().to_string();

        let mut steps: Vec<AgentStep> = Vec::new();
        let mut idx = 1u32;
        let mut executed = 0usize;
        let mut attempted = 0usize;
        let cwd = spec.cwd.clone().unwrap_or_else(|| {
            std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."))
        });
        // One session per worker so all of its actions share a continuous
        // context (cwd, environment) exactly like a plan's actions do.
        let session_id = self.kernel.kernel().sessions.create("agent", cwd.clone());

        for capability in selected.skill.resolved_capabilities() {
            // Cooperative cancellation/timeout/budget between steps.
            if let Err(reason) = ctx.check() {
                return AgentResult::aborted(
                    spec.id.clone(),
                    reason,
                    format!(
                        "stopped after {executed}/{attempted} capability steps (chosen skill '{chosen_id}')"
                    ),
                    steps,
                    0,
                );
            }
            attempted += 1;

            // Mirror the supervisor's high-risk gate: a worker never executes
            // high-risk capabilities (terminal.exec, git.push, deletes).
            if self.planner_registry.risk_of(capability) == Risk::High {
                steps.push(AgentStep::skipped(
                    idx,
                    capability.to_string(),
                    "high-risk (gated)",
                ));
                idx += 1;
                continue;
            }

            let permission = policy.permission_for(capability);
            if !matches!(
                permission,
                PermissionState::AutoApproved | PermissionState::Approved
            ) {
                steps.push(AgentStep::skipped(
                    idx,
                    capability.to_string(),
                    "requires-approval",
                ));
                idx += 1;
                continue;
            }

            let (tool, action) = split_capability(capability);
            let params = spec.params.get(capability).cloned().unwrap_or(Value::Null);
            let req = ActionRequest::new(tool, action, params);
            let exec_ctx = ExecutionContext {
                execution_id: Uuid::new_v4(),
                session_id,
                parent_execution: None,
                user_id: format!("agent:{}", spec.id),
                cwd: cwd.clone(),
                environment: std::env::vars().collect(),
                variables: HashMap::new(),
                permissions: Vec::new(),
                trace_id: Uuid::new_v4(),
                deadline: Some(
                    Utc::now()
                        + chrono::Duration::from_std(ctx.time_remaining())
                            .unwrap_or_else(|_| chrono::Duration::seconds(30)),
                ),
                // Fresh token: per-action cancellation is handled by the
                // driver's `check` between steps plus the runtime's hard
                // timeout around the whole worker.
                cancellation_token: CancellationToken::new(),
                metadata: HashMap::new(),
                // The child's security authority flows into the kernel so the
                // central egress gate classifies every destination this worker
                // would reach. `None` keeps the legacy unscoped behavior.
                security_scope: spec.security_scope.clone(),
            };

            let started = std::time::Instant::now();
            let result = self.kernel.run_action(req, exec_ctx).await;
            ctx.record_step();
            let elapsed = started.elapsed().as_millis() as u64;
            if result.success {
                executed += 1;
                steps.push(AgentStep::ok(idx, capability.to_string(), elapsed));
            } else {
                steps.push(AgentStep::failed(
                    idx,
                    capability.to_string(),
                    result
                        .error
                        .clone()
                        .unwrap_or_else(|| "unknown error".to_string()),
                    elapsed,
                ));
            }
            idx += 1;
        }

        if executed == 0 {
            return AgentResult::failed(
                spec.id.clone(),
                format!("chose '{chosen_id}' but no capability executed ({attempted} attempted)"),
                steps,
                "no_capability_executed",
            );
        }
        AgentResult::succeeded(
            spec.id.clone(),
            format!("chose '{chosen_id}': {executed}/{attempted} capability steps succeeded"),
            steps,
        )
    }
}
