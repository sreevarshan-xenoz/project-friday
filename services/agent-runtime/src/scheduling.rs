//! Scheduling — the "spawn because the planner has evidence, not because the
//! prompt said 'think step by step'" rule.
//!
//! A [`Decomposer`] estimates whether a goal splits into independent subtasks.
//! The planner consults it *before* spawning: a goal that does not decompose
//! is handled by one worker, never fan-out. v1 ships a rule-based
//! [`HeuristicDecomposer`]; a later LLM-backed decomposer plugs into the same
//! trait.

/// The planner's evidence about how a goal decomposes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DecomposeHint {
    /// Estimated number of independent subtasks (>= 1).
    pub estimated_subtasks: usize,
    /// `true` when the subtasks are independent enough to run in parallel.
    pub independent: bool,
}

impl DecomposeHint {
    /// `true` when the evidence says fan-out is warranted (2+ independent
    /// subtasks).
    pub fn warrants_fanout(&self) -> bool {
        self.independent && self.estimated_subtasks >= 2
    }
}

/// Decides whether a goal should be decomposed into parallel workers.
pub trait Decomposer: Send + Sync {
    /// `candidate_skills` is how many skills ranked for the goal — a proxy for
    /// how many different domains the work touches.
    fn decompose(&self, goal: &str, candidate_skills: usize) -> DecomposeHint;
}

/// Rule-based v1: counts conjunctive connectives and explicit items in the
/// goal, and weighs in the number of candidate skills. Deterministic and
/// cheap; deliberately conservative (an ambiguous goal stays a single worker).
pub struct HeuristicDecomposer {
    max_subtasks: usize,
}

impl Default for HeuristicDecomposer {
    fn default() -> Self {
        Self::new(8)
    }
}

impl HeuristicDecomposer {
    pub fn new(max_subtasks: usize) -> Self {
        Self {
            max_subtasks: max_subtasks.max(1),
        }
    }
}

impl Decomposer for HeuristicDecomposer {
    fn decompose(&self, goal: &str, candidate_skills: usize) -> DecomposeHint {
        let lower = goal.to_lowercase();
        let mut subtasks = 1usize;
        for connective in [" and ", " then ", " also ", " plus ", " ; ", ",", " or "] {
            subtasks += lower.matches(connective).count();
        }
        // Explicit numbering ("1. x 2. y") signals enumerated subtasks.
        for item in ["1.", "2.", "3.", "4.", "5."] {
            if lower.contains(item) {
                subtasks = subtasks.max(2);
            }
        }
        // A goal touching multiple skill domains suggests parallel work.
        if candidate_skills >= 3 {
            subtasks += 1;
        }
        let estimated_subtasks = subtasks.min(self.max_subtasks);
        DecomposeHint {
            estimated_subtasks,
            independent: estimated_subtasks >= 2,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_goal_does_not_warrant_fanout() {
        let d = HeuristicDecomposer::default();
        let hint = d.decompose("commit the changes", 1);
        assert_eq!(hint.estimated_subtasks, 1);
        assert!(!hint.warrants_fanout());
    }

    #[test]
    fn conjunctive_goal_warrants_fanout() {
        let d = HeuristicDecomposer::default();
        let hint = d.decompose(
            "check the git status and list the files and read the readme",
            1,
        );
        assert!(hint.estimated_subtasks >= 3);
        assert!(hint.warrants_fanout());
    }

    #[test]
    fn many_candidate_skills_add_evidence() {
        let d = HeuristicDecomposer::default();
        let hint = d.decompose("set up the server", 4);
        assert!(hint.warrants_fanout());
    }

    #[test]
    fn clamps_to_max_subtasks() {
        let d = HeuristicDecomposer::new(3);
        let hint = d.decompose("a and b and c and d and e and f", 0);
        assert_eq!(hint.estimated_subtasks, 3);
    }
}
