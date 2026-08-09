//! [`AgentHandle`] — the orchestrator's mailbox for one worker.
//!
//! A handle exposes the worker's live state (via a watch channel), a one-shot
//! for the final result, and a cancellation signal. `await_result` consumes
//! the handle and returns the worker's [`crate::spec::AgentResult`].

use tokio::sync::{oneshot, watch};

use crate::cancellation::CancellationToken;
use crate::spec::{AgentResult, AgentState};

/// The orchestrator's view of one running worker.
pub struct AgentHandle {
    /// Worker id (mirrors `AgentSpec::id`).
    pub id: String,
    state: watch::Receiver<AgentState>,
    result: oneshot::Receiver<AgentResult>,
    cancellation: CancellationToken,
}

impl AgentHandle {
    pub(crate) fn new(
        id: String,
        state: watch::Receiver<AgentState>,
        result: oneshot::Receiver<AgentResult>,
        cancellation: CancellationToken,
    ) -> Self {
        Self {
            id,
            state,
            result,
            cancellation,
        }
    }

    /// The worker's current lifecycle state.
    pub fn state(&self) -> AgentState {
        *self.state.borrow()
    }

    /// Ask the worker to stop. Cooperative: the worker stops at its next
    /// [`crate::context::AgentContext::check`]; the runtime also hard-kills the
    /// driver at the deadline.
    pub fn cancel(&self) {
        self.cancellation.cancel();
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancellation.is_cancelled()
    }

    /// Wait for the worker's final result. Falls back to a `Failed` result if
    /// the worker task was dropped without sending (should not happen).
    pub async fn await_result(self) -> AgentResult {
        match self.result.await {
            Ok(result) => result,
            Err(_) => AgentResult::failed(
                self.id,
                "worker task ended without a result",
                vec![],
                "runtime dropped the worker",
            ),
        }
    }
}
