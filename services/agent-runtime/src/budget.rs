//! Execution budget for a worker: step/cost limits plus a thread-safe tracker
//! the driver consults on every step. The hard *time* cap is enforced by the
//! runtime's deadline (`timeout_at`); this budget governs *volume* — how much
//! work a worker may do before it must stop and report back.

use std::sync::Mutex;

use serde::{Deserialize, Serialize};

/// The limits a worker must stay within.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct BudgetLimits {
    /// Maximum steps. 0 = unlimited.
    pub max_steps: u32,
    /// Advisory time budget (the deadline enforces the hard cap).
    pub max_time_ms: u64,
    /// Maximum cost in arbitrary units. 0 = unlimited.
    pub max_cost: f64,
}

impl Default for BudgetLimits {
    fn default() -> Self {
        Self {
            max_steps: 0,
            max_time_ms: 0,
            max_cost: 0.0,
        }
    }
}

#[derive(Debug, Default)]
struct Spent {
    steps: u32,
    cost: f64,
}

/// Interior-mutable step/cost tracker shared between the runtime and the
/// driver via [`crate::context::AgentContext`].
#[derive(Debug)]
pub struct BudgetTracker {
    limits: BudgetLimits,
    spent: Mutex<Spent>,
}

impl BudgetTracker {
    pub fn new(limits: BudgetLimits) -> Self {
        Self {
            limits,
            spent: Mutex::new(Spent::default()),
        }
    }

    /// Record one step taken by the worker (with optional cost).
    pub fn record_step(&self, cost: f64) {
        let mut spent = self.spent.lock().expect("budget poisoned");
        spent.steps += 1;
        spent.cost += cost;
    }

    pub fn steps(&self) -> u32 {
        self.spent.lock().expect("budget poisoned").steps
    }

    pub fn cost(&self) -> f64 {
        self.spent.lock().expect("budget poisoned").cost
    }

    /// `true` when the worker has consumed its step/cost budget and must stop.
    pub fn exceeded(&self) -> bool {
        let spent = self.spent.lock().expect("budget poisoned");
        (self.limits.max_steps > 0 && spent.steps >= self.limits.max_steps)
            || (self.limits.max_cost > 0.0 && spent.cost >= self.limits.max_cost)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tracks_steps_and_detects_exhaustion() {
        let budget = BudgetTracker::new(BudgetLimits {
            max_steps: 3,
            max_time_ms: 0,
            max_cost: 0.0,
        });
        assert!(!budget.exceeded());
        budget.record_step(0.0);
        budget.record_step(0.5);
        assert_eq!(budget.steps(), 2);
        assert_eq!(budget.cost(), 0.5);
        assert!(!budget.exceeded());
        budget.record_step(0.0);
        assert!(budget.exceeded());
    }

    #[test]
    fn zero_limits_are_unlimited() {
        let budget = BudgetTracker::new(BudgetLimits::default());
        for _ in 0..100 {
            budget.record_step(0.0);
        }
        assert!(!budget.exceeded());
    }
}
