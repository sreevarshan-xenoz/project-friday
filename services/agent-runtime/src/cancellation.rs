//! A minimal cancellation token for worker agents.
//!
//! Mirrors the semantics of the action-runtime's token without importing it
//! (this crate stays dependency-light): a shared flag plus a `Notify` so a
//! worker can `await` cancellation. Cloning yields a token observing the same
//! state, so an orchestrator cancelling one handle cancels every worker that
//! shares it.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use tokio::sync::Notify;

/// A shared cancellation signal.
#[derive(Clone, Default)]
pub struct CancellationToken {
    inner: Arc<TokenInner>,
}

struct TokenInner {
    cancelled: AtomicBool,
    notify: Notify,
}

impl Default for TokenInner {
    fn default() -> Self {
        Self {
            cancelled: AtomicBool::new(false),
            notify: Notify::new(),
        }
    }
}

impl std::fmt::Debug for CancellationToken {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CancellationToken")
            .field("cancelled", &self.is_cancelled())
            .finish()
    }
}

impl CancellationToken {
    pub fn new() -> Self {
        Self::default()
    }

    /// Mark the token cancelled and wake every waiter.
    pub fn cancel(&self) {
        self.inner.cancelled.store(true, Ordering::SeqCst);
        self.inner.notify.notify_waiters();
    }

    pub fn is_cancelled(&self) -> bool {
        self.inner.cancelled.load(Ordering::SeqCst)
    }

    /// Resolve as soon as the token is cancelled.
    pub async fn cancelled(&self) {
        if self.is_cancelled() {
            return;
        }
        self.inner.notify.notified().await;
    }
}
