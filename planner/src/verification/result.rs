use serde::{Deserialize, Serialize};

/// Outcome of verifying a completed task. `Passed` means the observable state
/// matched the expectation; `Failed` means it did not (or the executor reported
/// an error).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum VerificationResult {
    Passed,
    Failed,
}

impl VerificationResult {
    pub fn as_str(&self) -> &'static str {
        match self {
            VerificationResult::Passed => "passed",
            VerificationResult::Failed => "failed",
        }
    }
}

/// The verdict the Verification Engine reaches for a task.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    /// Task succeeded; emit `verification.completed`.
    Passed,
    /// Task can be retried; emit `task.retry` (handled by the Execution Engine).
    Retry,
    /// Task cannot be recovered; emit `verification.failed` and optionally
    /// trigger a single replan via `intent.created`.
    GiveUp,
}
