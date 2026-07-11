/// Policy controlling recovery behaviour. Keeps the "retry or replan?"
/// decision data-driven rather than hard-coded in the engine.
#[derive(Debug, Clone, Copy)]
pub struct RetryPolicy {
    pub max_retries: u32,
    pub max_replans: u32,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            max_retries: 2,
            max_replans: 1,
        }
    }
}

impl RetryPolicy {
    /// Decide whether a failed task should be retried. Once retries are
    /// exhausted the verdict is `GiveUp`; the caller separately decides whether
    /// to trigger a replan using its own replan budget.
    pub fn decide(&self, retries_so_far: u32) -> super::result::Verdict {
        if retries_so_far < self.max_retries {
            super::result::Verdict::Retry
        } else {
            super::result::Verdict::GiveUp
        }
    }
}
