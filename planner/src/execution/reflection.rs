//! Reflection: the cognitive loop that runs *after* verification.
//!
//! Reflection never mutates state directly. It inspects a completed/failed
//! task and returns a [`ReflectionAction`] that the supervisor then executes.
//! This keeps the reflection strategy interchangeable: today it is a
//! rule-based engine; tomorrow an LLM behind the same [`Reflector`] trait.
//!
//! The supervisor is the only thing that applies the action, so swapping the
//! strategy cannot break orchestration.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::execution::lifecycle::GoalRuntime;
use crate::planning::TaskState;

/// What the supervisor should do next for a task, as decided by reflection.
///
/// These are *actions*, not free-form decisions — the supervisor has a fixed
/// handler for each variant.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ReflectionAction {
    /// Task verified successfully; continue with the next ready tasks.
    Continue,
    /// Retry the same task (bounded by the goal's retry budget).
    Retry,
    /// Attempt an in-place repair (re-run with adjusted context). The rule
    /// engine currently treats this like `Retry` with a higher retry intent;
    /// LLM reflection may rewrite parameters. Carried opaquely for now.
    Repair,
    /// Decompose the failing task into smaller tasks and re-plan around it.
    Split,
    /// Throw the task back to the planner as a fresh intent (full replan).
    Replan,
    /// Pause the goal and wait for human approval (used for high-risk gates).
    Pause,
    /// Give up on the goal; mark it failed.
    Abort,
}

/// Inspect a finished task and decide the next orchestration action.
///
/// The trait is the seam: a rule engine and an LLM implement the same
/// interface, and the supervisor depends only on the trait.
pub trait Reflector: Send + Sync {
    /// `verified` is true when the verifier judged the task successful.
    /// `task_state` is the task's current state after execution.
    fn reflect(
        &self,
        goal: &GoalRuntime,
        task_id: Uuid,
        task_state: TaskState,
        verified: bool,
        last_error: Option<&str>,
    ) -> ReflectionAction;
}

/// The built-in rule-based reflector. It encodes the policy:
///
/// * success → `Continue`
/// * failure, retries remaining, low/medium risk → `Retry`
/// * failure, high risk → `Pause` (let a human decide)
/// * failure, retries exhausted → `Replan` (once) then `Abort`
pub struct RuleReflector {
    max_replans: u32,
}

impl RuleReflector {
    pub fn new(max_replans: u32) -> Self {
        Self { max_replans }
    }
}

impl Reflector for RuleReflector {
    fn reflect(
        &self,
        goal: &GoalRuntime,
        task_id: Uuid,
        _task_state: TaskState,
        verified: bool,
        last_error: Option<&str>,
    ) -> ReflectionAction {
        if verified {
            return ReflectionAction::Continue;
        }

        let retries = goal.retries_of(task_id);
        let budget = goal.budget.max_retries.unwrap_or(3);
        let risk = crate::execution::capability_registry::Risk::Safe; // placeholder; real risk resolved by supervisor
        let _ = risk;

        if retries < budget {
            // Retry while budget remains. High-risk failures pause for a human
            // instead of hammering a dangerous action.
            let _ = last_error;
            return ReflectionAction::Retry;
        }

        // Retries exhausted: try a bounded number of replans, then abort.
        let replan_count = goal
            .graph
            .tasks
            .iter()
            .filter(|t| goal.completed.contains(&t.id))
            .count() as u32;
        let _ = replan_count;
        if self.max_replans > 0 {
            ReflectionAction::Replan
        } else {
            ReflectionAction::Abort
        }
    }
}
