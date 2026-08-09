//! [`AgentContext`] — the envelope the runtime hands to a worker's driver.
//!
//! It carries everything a worker needs to self-regulate: its own spec, a hard
//! deadline, a cancellation signal, and a shared budget tracker. Drivers call
//! [`AgentContext::check`] between steps; the runtime additionally enforces a
//! hard `timeout_at` around the whole run, so a misbehaving driver cannot
//! outlive its deadline even if it never checks.

use std::sync::Arc;
use std::time::Duration;
use tokio::time::Instant;

use serde::{Deserialize, Serialize};

use crate::budget::BudgetTracker;
use crate::cancellation::CancellationToken;
use crate::spec::AgentSpec;

/// Why a worker stopped before succeeding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AgentAbort {
    Cancelled,
    TimedOut,
    BudgetExhausted,
}

impl AgentAbort {
    pub fn as_str(&self) -> &'static str {
        match self {
            AgentAbort::Cancelled => "cancelled",
            AgentAbort::TimedOut => "timed_out",
            AgentAbort::BudgetExhausted => "budget_exhausted",
        }
    }
}

impl std::fmt::Display for AgentAbort {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// The execution envelope for one worker.
#[derive(Clone)]
pub struct AgentContext {
    pub spec: AgentSpec,
    /// Hard wall-clock deadline. `check` fails with `TimedOut` past it.
    pub deadline: Instant,
    /// Shared budget tracker — record a step via [`AgentContext::record_step`].
    pub budget: Arc<BudgetTracker>,
    /// Shared cancellation signal (the handle can cancel the worker).
    pub cancellation: CancellationToken,
}

impl AgentContext {
    /// `Ok(())` if the worker may continue; `Err(reason)` if it must stop.
    ///
    /// Checks, in order: cancellation, deadline, budget. A worker that checks
    /// before every step cannot be cancelled, overrun, or overspend silently.
    pub fn check(&self) -> Result<(), AgentAbort> {
        if self.cancellation.is_cancelled() {
            return Err(AgentAbort::Cancelled);
        }
        if tokio::time::Instant::now() >= self.deadline {
            return Err(AgentAbort::TimedOut);
        }
        if self.budget.exceeded() {
            return Err(AgentAbort::BudgetExhausted);
        }
        Ok(())
    }

    /// Record one step against the budget.
    pub fn record_step(&self) {
        self.budget.record_step(0.0);
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancellation.is_cancelled()
    }

    /// Remaining time until the deadline (never negative).
    pub fn time_remaining(&self) -> Duration {
        self.deadline.saturating_duration_since(Instant::now())
    }

    /// The worker's private memory scope (allowed keys/prefixes). Empty means
    /// the worker has no private memory.
    pub fn memory_scope(&self) -> &[String] {
        &self.spec.memory_scope
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx(ms: u64, max_steps: u32) -> AgentContext {
        AgentContext {
            spec: AgentSpec::new("t", "task")
                .with_time_limit(ms)
                .with_max_steps(max_steps),
            deadline: Instant::now() + Duration::from_millis(ms),
            budget: Arc::new(BudgetTracker::new(crate::budget::BudgetLimits {
                max_steps,
                max_time_ms: ms,
                max_cost: 0.0,
            })),
            cancellation: CancellationToken::new(),
        }
    }

    #[test]
    fn check_reports_cancellation() {
        let c = ctx(60_000, 0);
        c.cancellation.cancel();
        assert_eq!(c.check(), Err(AgentAbort::Cancelled));
    }

    #[test]
    fn check_reports_timeout_after_deadline() {
        let c = ctx(0, 0); // deadline already passed
        assert_eq!(c.check(), Err(AgentAbort::TimedOut));
    }

    #[test]
    fn check_reports_budget_exhaustion() {
        let c = ctx(60_000, 2);
        c.record_step();
        c.record_step();
        assert_eq!(c.check(), Err(AgentAbort::BudgetExhausted));
    }
}
