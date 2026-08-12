//! Result merge — the "planner merges everything" step. Workers report
//! independently (each through its own mailbox); the orchestrator folds them
//! into one [`MergedReport`] the planner can act on. Only the planner decides
//! what becomes permanent; merging is pure aggregation, it never mutates
//! memory or state.

use crate::spec::{AgentResult, AgentState};

/// Aggregated view of a completed fan-out.
#[derive(Debug, Clone, Default)]
pub struct MergedReport {
    /// Every worker result, in spawn order.
    pub agents: Vec<AgentResult>,
    pub succeeded: usize,
    pub failed: usize,
    pub cancelled: usize,
    pub total_steps: u32,
    pub total_duration_ms: u64,
}

impl MergedReport {
    /// `true` when at least one worker ran and **every** worker reached
    /// [`AgentState::Succeeded`]. `Succeeded` is the only success: a worker
    /// that failed, was cancelled, timed out, or hit a budget wall makes this
    /// `false`.
    pub fn all_succeeded(&self) -> bool {
        !self.agents.is_empty() && self.agents.iter().all(|a| a.state.is_success())
    }

    /// One human/planner-readable line per worker.
    pub fn summary_lines(&self) -> Vec<String> {
        self.agents
            .iter()
            .map(|r| {
                format!(
                    "[{}] {} — {} ({} step(s), {} ms{})",
                    r.state,
                    r.agent_id,
                    r.summary,
                    r.steps.len(),
                    r.duration_ms,
                    r.error
                        .as_ref()
                        .map(|e| format!(", error: {e}"))
                        .unwrap_or_default(),
                )
            })
            .collect()
    }
}

/// Fold worker results into one report.
pub fn merge(results: Vec<AgentResult>) -> MergedReport {
    let mut report = MergedReport {
        agents: results,
        ..Default::default()
    };
    for agent in &report.agents {
        match agent.state {
            AgentState::Succeeded => report.succeeded += 1,
            AgentState::Failed => report.failed += 1,
            AgentState::Cancelled => report.cancelled += 1,
            _ => {}
        }
        report.total_steps += agent.steps.len() as u32;
        report.total_duration_ms += agent.duration_ms;
    }
    report
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::spec::AgentStep;

    #[test]
    fn merge_aggregates_success_and_failure() {
        let ok = AgentResult::succeeded("a", "did a", vec![AgentStep::ok(1, "git.status", 5)]);
        let ok2 = AgentResult::succeeded("b", "did b", vec![AgentStep::ok(1, "docs.search", 3)]);
        let bad = AgentResult::failed("c", "could not", vec![], "no usable skill");

        let report = merge(vec![ok, ok2, bad]);
        assert_eq!(report.succeeded, 2);
        assert_eq!(report.failed, 1);
        assert_eq!(report.total_steps, 2);
        assert!(!report.all_succeeded());
        assert_eq!(report.summary_lines().len(), 3);

        let all_ok = merge(vec![AgentResult::succeeded("a", "ok", vec![])]);
        assert!(all_ok.all_succeeded());
    }

    #[test]
    fn aborted_workers_block_all_succeeded() {
        use crate::context::AgentAbort;
        let timed_out = AgentResult::aborted("slow", AgentAbort::TimedOut, "late", vec![], 5000);
        let report = merge(vec![AgentResult::succeeded("a", "ok", vec![]), timed_out]);
        assert!(!report.all_succeeded(), "a timed-out worker is not success");

        let budget =
            AgentResult::aborted("stepper", AgentAbort::BudgetExhausted, "budget", vec![], 0);
        assert!(!merge(vec![budget]).all_succeeded());
    }
}
