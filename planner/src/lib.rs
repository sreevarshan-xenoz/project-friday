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
    IntentEngine, PlanningEngine, PlanningTelemetry, count_validation_issues, planning_engine_with,
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

use crate::execution::{CapabilityRegistry, GoalStore, GoalSupervisor, PolicyAuthority};
use crate::planning::planners::backend::ReasoningBackend;
use crate::planning::planners::context::{RiskPolicy, WorkspaceContext};
use crate::execution::lifecycle::ExecutionBudget;

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
    let registry = Arc::new(CapabilityRegistry::new());
    crate::execution::native_caps::register_native_builtins(&registry);

    friday
        .register_plugin(Box::new(IntentEngine::new()))
        .await?;
    let planning = planning_engine_with(
        registry.clone(),
        backend,
        ExecutionBudget::default(),
        RiskPolicy::default(),
        WorkspaceContext::default(),
    );
    friday.register_plugin(Box::new(planning)).await?;
    friday
        .register_plugin(Box::new(VerificationEngine::new()))
        .await?;
    let store = GoalStore::open(&goal_store_path(friday))
        .map_err(|e| friday_core::FridayError::Plugin("supervisor".to_string(), format!("goal store: {e}")))?;
    friday
        .register_plugin(Box::new(GoalSupervisor::new(
            registry.clone(),
            Arc::new(AllowAllPolicy),
            Arc::new(store),
        )))
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
    let store = GoalStore::open(&goal_store_path(friday))
        .map_err(|e| friday_core::FridayError::Plugin("supervisor".to_string(), format!("goal store: {e}")))?;
    friday
        .register_plugin(Box::new(GoalSupervisor::new(registry, policy, Arc::new(store))))
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
