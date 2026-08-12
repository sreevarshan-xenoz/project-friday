//! # FRIDAY Planner
//!
//! The "brain" of FRIDAY, split into four independent engines that communicate
//! only through the event bus (see [`events`] for the vocabulary):
//!
//! * **Intent Engine** — understands the request (`goal.raw` → `intent.created`).
//! * **Planning Engine** — decomposes intent into a task graph
//!   (`intent.created` → `plan.created`).
//! * **Execution Engine** — routes tasks to plugins and tracks lifecycle
//!   (`plan.created` → `task.started` / `exec.action` → `task.completed`).
//! * **Verification Engine** — confirms success and drives retry/replan
//!   (`task.completed`/`task.failed` → `verification.*`).
//!
//! Plans are modelled as a DAG ([`planning::TaskGraph`]) so independent tasks
//! can later run in parallel, and every task has an explicit
//! [`planning::TaskState`] lifecycle for debuggability.

pub mod engines;
pub mod events;
pub mod execution;
pub mod experience;
pub mod intent;
pub mod planning;
pub mod verification;

pub use engines::{
    count_validation_issues, planning_engine_with, IntentEngine, PlanningEngine, PlanningTelemetry,
};
pub use events::{
    EXEC_ACTION, EXEC_RESULT, GOAL_RAW, INTENT_CREATED, PLAN_CREATED, TASK_COMPLETED, TASK_FAILED,
    TASK_RETRY, TASK_STARTED, VERIFICATION_COMPLETED, VERIFICATION_FAILED, VERIFICATION_STARTED,
};
pub use intent::{parse, Intent, IntentType};
pub use planning::{levels, plan, Task, TaskGraph, TaskState};
pub use verification::{RetryPolicy, Verdict, VerificationEngine, VerificationResult};

use std::path::PathBuf;
use std::sync::Arc;
use uuid::Uuid;

use friday_core::Friday;
use friday_core::Result;

use crate::execution::lifecycle::ExecutionBudget;
use crate::execution::{
    CapabilityRegistry, GoalCompletionSink, GoalStore, GoalSupervisor, PolicyAuthority,
};
use crate::planning::planners::backend::ReasoningBackend;
use crate::planning::planners::context::{RiskPolicy, WorkspaceContext};
use friday_investigation::InvestigationEngine;
use friday_skills::SkillContextSource;

/// Resolve the goal-store path from config (`goal_store_path`) or default to a
/// per-instance temporary database. The default is unique per `Friday` instance
/// (a random file under the system temp dir) so concurrent tests / runs never
/// collide on a shared lock; a real deployment sets `goal_store_path` to a
/// stable location for durable `friday resume`.
fn goal_store_path(friday: &Friday) -> PathBuf {
    if let Some(v) = friday
        .config()
        .get("goal_store_path")
        .and_then(|v| v.as_str())
    {
        return PathBuf::from(v);
    }
    let dir = std::env::temp_dir().join("friday-goals");
    PathBuf::from(&dir).join(format!("goals-{}.redb", Uuid::new_v4()))
}

fn timeline_store_path(friday: &Friday) -> PathBuf {
    if let Some(v) = friday
        .config()
        .get("timeline_store_path")
        .and_then(|v| v.as_str())
    {
        return PathBuf::from(v);
    }
    let dir = std::env::temp_dir().join("friday-timeline");
    PathBuf::from(&dir).join(format!("timeline-{}.redb", Uuid::new_v4()))
}

/// Register all planner engines with a runtime. Order does not matter; they are
/// wired together purely by the events they emit and consume.
///
/// The planning engine and supervisor share one `CapabilityRegistry` (native
/// capabilities pre-registered), so LLM-produced plans validate against the same
/// real capabilities the supervisor resolves. The plan is rule-only by default;
/// pass an LLM backend via [`register_engines_with_llm`] to enable the
/// [`crate::planning::planners::LlmPlanner`].
///
/// The [`GoalSupervisor`] is registered last so it observes the
/// `plan.created` events produced by the planning engine and owns the goal
/// lifecycle from there on. Returns the shared `CapabilityRegistry` so external
/// capability sources (e.g. MCP servers via `friday-mcp`) can register into the
/// exact instance the planner/supervisor resolve against.
pub async fn register_engines(friday: &mut Friday) -> Result<Arc<CapabilityRegistry>> {
    register_engines_with_llm(friday, None).await
}

/// Like [`register_engines`] but optionally wires an LLM [`ReasoningBackend`]
/// (e.g. an adapter over `InferenceFabric`) into the planning engine. When
/// `backend` is `None` the engine is rule-only and fully deterministic. Returns
/// the shared `CapabilityRegistry`.
pub async fn register_engines_with_llm(
    friday: &mut Friday,
    backend: Option<Box<dyn ReasoningBackend>>,
) -> Result<Arc<CapabilityRegistry>> {
    register_engines_with_policy(friday, backend, |_| PolicyBinding {
        policy: Arc::new(AllowAllPolicy),
        skills: None,
        investigation: None,
        completion_sink: None,
    })
    .await
}

/// Everything the policy factory hands back to the boot path: the supervisor's
/// [`PolicyAuthority`] plus — optionally — a skill source whose ranked skills
/// enrich every `PlanContext` the LLM planner sees. The policy factory runs
/// once, against the freshly built [`CapabilityRegistry`], so both the policy
/// and the skill context are constructed from the *same* registry the planner
/// and supervisor resolve against.
pub struct PolicyBinding {
    /// The supervisor's permission authority (skill-aware or allow-all).
    pub policy: Arc<dyn PolicyAuthority>,
    /// Skill source wired into the planning engine. `None` disables the
    /// skill-aware `PlanContext` enrichment (rule planner is unaffected).
    pub skills: Option<Arc<dyn SkillContextSource>>,
    /// Investigation engine wired into the planning engine. When present,
    /// every LLM-routed goal's `PlanContext` carries the channels it requires
    /// evidence from — the EVIDENCE REQUIRED section in the LLM planner's
    /// prompt. `None` disables the enrichment (rule planner is unaffected).
    pub investigation: Option<InvestigationEngine>,
    /// Optional consumer of fully verified goal completions.
    pub completion_sink: Option<Arc<dyn GoalCompletionSink>>,
}

/// Like [`register_engines_with_llm`] but the supervisor's [`PolicyAuthority`]
/// (and an optional skill context source) is provided by `policy`, a factory
/// invoked with the freshly built [`CapabilityRegistry`] (so the policy can be
/// constructed from the *same* registry the planner and supervisor resolve
/// against — e.g. a skill-aware policy that gates on resolved capabilities).
///
/// The factory returns a [`PolicyBinding`]; its `skills` field is attached to
/// the planning engine so every goal's `PlanContext` carries the skills ranked
/// against it (see [`PlanningEngine::with_skill_context`]).
///
/// **Ordering:** the factory runs **once, up front** — before any plugin is
/// registered — and its returned `policy` is reused for the supervisor. It
/// always runs, even when a later boot step (e.g. the goal store) fails;
/// callers must not rely on lazy invocation.
///
/// **API note:** the closure's return type is [`PolicyBinding`], not a bare
/// `Arc<dyn PolicyAuthority>` — hosts that want only policy return
/// `skills: None`.
///
/// This is the hook for swapping the default allow-all policy without
/// changing the shared boot path: every caller of
/// [`register_engines_with_llm`] keeps working, and hosts that want policy
/// (skill approvals, workspace rules) build it from the returned registry.
pub async fn register_engines_with_policy<F>(
    friday: &mut Friday,
    backend: Option<Box<dyn ReasoningBackend>>,
    policy: F,
) -> Result<Arc<CapabilityRegistry>>
where
    F: FnOnce(Arc<CapabilityRegistry>) -> PolicyBinding + Send,
{
    let registry = Arc::new(CapabilityRegistry::new());
    crate::execution::native_caps::register_native_builtins(&registry);

    // Run the policy factory up front: it needs the registry, and the skill
    // source it hands back must reach the planning engine before registration.
    let PolicyBinding {
        policy,
        skills,
        investigation,
        completion_sink,
    } = policy(registry.clone());

    friday
        .register_plugin(Box::new(IntentEngine::new()))
        .await?;
    let mut planning = planning_engine_with(
        registry.clone(),
        backend,
        ExecutionBudget::default(),
        RiskPolicy::default(),
        WorkspaceContext::default(),
    );
    if let Some(skills) = skills {
        planning = planning.with_skill_context(skills);
    }
    if let Some(investigation) = investigation {
        planning = planning.with_investigation_engine(investigation);
    }

    // Wire the experience provider if the timeline store is available.
    match crate::experience::TimelineStore::open(&timeline_store_path(friday)) {
        Ok(timeline) => {
            let entries = timeline.all_entries().unwrap_or_default();
            let workflows = timeline.load_workflows().unwrap_or_default();
            let reflections = timeline.load_reflections().unwrap_or_default();
            let mut service = crate::experience::ExperienceService::new();
            service.rebuild(&entries, &workflows, &reflections);
            planning = planning.with_experience(Arc::new(service));
            tracing::info!(
                entries = entries.len(),
                workflows = workflows.len(),
                "experience provider wired"
            );
        }
        Err(e) => {
            tracing::warn!(error = %e, "timeline store unavailable; experience disabled");
        }
    }

    friday.register_plugin(Box::new(planning)).await?;
    friday
        .register_plugin(Box::new(VerificationEngine::new()))
        .await?;
    let store = GoalStore::open(&goal_store_path(friday)).map_err(|e| {
        friday_core::FridayError::Plugin("supervisor".to_string(), format!("goal store: {e}"))
    })?;
    friday
        .register_plugin(Box::new(
            GoalSupervisor::new(registry.clone(), policy, Arc::new(store))
                .with_completion_sink_opt(completion_sink),
        ))
        .await?;
    Ok(registry)
}

/// Register the supervisor with an explicit policy authority and capability
/// registry (used when native capabilities / MCP servers are provided by the
/// host). The registry should already have its capabilities registered.
pub async fn register_supervisor(
    friday: &mut Friday,
    registry: Arc<CapabilityRegistry>,
    policy: Arc<dyn PolicyAuthority>,
) -> Result<()> {
    let store = GoalStore::open(&goal_store_path(friday)).map_err(|e| {
        friday_core::FridayError::Plugin("supervisor".to_string(), format!("goal store: {e}"))
    })?;
    friday
        .register_plugin(Box::new(GoalSupervisor::new(
            registry,
            policy,
            Arc::new(store),
        )))
        .await?;
    Ok(())
}

/// Default policy: everything permitted without a human gate except what the
/// capability's [`Risk`] metadata marks high (handled by the supervisor
/// directly). Swap this for a workspace/project policy engine later.
struct AllowAllPolicy;

impl PolicyAuthority for AllowAllPolicy {
    fn allows(&self, _tool: &str, _action: &str) -> bool {
        true
    }
}
