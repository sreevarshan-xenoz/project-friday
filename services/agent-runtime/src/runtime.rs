//! [`AgentRuntime`] — the orchestrator. Owns spawning, lifecycle, cancellation,
//! deadlines, budgets, and result collection; never decides *what* a worker
//! does (that is the driver's job).
//!
//! ```text
//! Planner ── spawn(AgentSpec, driver) ──▶ AgentHandle (mailbox)
//!                 │                           │
//!                 ▼                           ▼
//!            worker task ──result──▶ await_result ──▶ merge ──▶ MergedReport
//! ```
//!
//! Concurrency is bounded by a semaphore: `spawn` returns immediately, up to
//! `max_concurrency` workers run at once, the rest wait in `Pending`. Each
//! worker's run is wrapped in a hard `timeout_at(deadline)` so a hung driver
//! terminates as `TimedOut` instead of blocking the pool forever.

use crate::budget::{BudgetLimits, BudgetTracker};
use crate::cancellation::CancellationToken;
use crate::context::AgentAbort;
use crate::driver::AgentDriver;
use crate::handle::AgentHandle;
use crate::merge::MergedReport;
use crate::spec::{AgentResult, AgentSpec, AgentState};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{oneshot, watch, Semaphore};
use tokio::time::Instant;

/// Default pool size.
pub const DEFAULT_MAX_CONCURRENCY: usize = 4;
/// Default per-worker time limit when the spec leaves it at 0.
pub const DEFAULT_TIME_LIMIT: Duration = Duration::from_secs(60);

/// The worker orchestrator.
pub struct AgentRuntime {
    pool: Arc<Semaphore>,
    default_time_limit: Duration,
}

impl Default for AgentRuntime {
    fn default() -> Self {
        Self::new()
    }
}

impl AgentRuntime {
    pub fn new() -> Self {
        Self {
            pool: Arc::new(Semaphore::new(DEFAULT_MAX_CONCURRENCY)),
            default_time_limit: DEFAULT_TIME_LIMIT,
        }
    }

    pub fn with_max_concurrency(mut self, n: usize) -> Self {
        self.pool = Arc::new(Semaphore::new(n.max(1)));
        self
    }

    pub fn with_default_time_limit(mut self, d: Duration) -> Self {
        self.default_time_limit = d;
        self
    }

    /// The default per-worker time limit used when a spec leaves it at 0.
    /// The Workforce records the resulting wall-clock deadline in the durable
    /// pre-dispatch checkpoint so reconciliation can reason about abandoned
    /// runs (R2-2 F2).
    pub fn default_time_limit(&self) -> Duration {
        self.default_time_limit
    }

    /// Spawn one worker. Returns immediately; the worker runs when a pool
    /// permit is free. State starts `Pending`, flips to `Running` when the
    /// permit is acquired, then to a terminal state.
    pub fn spawn(&self, spec: AgentSpec, driver: Arc<dyn AgentDriver>) -> AgentHandle {
        let id = spec.id.clone();
        let time_limit = if spec.time_limit_ms == 0 {
            self.default_time_limit
        } else {
            Duration::from_millis(spec.time_limit_ms)
        };
        let cancellation = CancellationToken::new();
        let budget = Arc::new(BudgetTracker::new(BudgetLimits {
            max_steps: spec.max_steps,
            max_time_ms: time_limit.as_millis() as u64,
            max_cost: 0.0,
        }));

        let (state_tx, state_rx) = watch::channel(AgentState::Pending);
        let (result_tx, result_rx) = oneshot::channel();

        // The worker task must not capture the originals — the handle needs
        // them below. Clone everything the task references up front.
        let task_id = id.clone();
        let task_cancellation = cancellation.clone();
        let pool = Arc::clone(&self.pool);
        let driver = Arc::clone(&driver);
        tokio::spawn(async move {
            // Bounded concurrency: wait for a free slot. Cancellation while
            // queued surfaces as `Cancelled` (the deadline starts when the
            // worker actually begins).
            let permit = match pool.acquire().await {
                Ok(p) => p,
                Err(_) => {
                    // Semaphore closed — cannot happen in this runtime.
                    return;
                }
            };
            let deadline = Instant::now() + time_limit;
            let ctx = crate::context::AgentContext {
                spec,
                deadline,
                budget: Arc::clone(&budget),
                cancellation: task_cancellation.clone(),
            };

            state_tx.send_replace(AgentState::Running);
            let started = Instant::now();
            let cancel_waiter = ctx.cancellation.clone();
            // Race note: when the driver finishes at the exact deadline (or
            // cancellation) instant, `select!` may pick the timeout/cancel
            // branch and report a terminal abort for a worker that actually
            // succeeded. Harmless in practice (the driver's own `check`
            // usually returns the abort itself); the worker's steps are still
            // lost to the abort result, so callers should treat sub-deadline
            // successes as authoritative.
            let result = tokio::select! {
                _ = cancel_waiter.cancelled() => {
                    AgentResult::aborted(
                        task_id,
                        AgentAbort::Cancelled,
                        "worker cancelled by the orchestrator",
                        vec![],
                        started.elapsed().as_millis() as u64,
                    )
                }
                res = tokio::time::timeout_at(deadline, driver.run(ctx)) => {
                    match res {
                        Ok(r) => r,
                        Err(_) => AgentResult::aborted(
                            task_id,
                            AgentAbort::TimedOut,
                            format!("worker exceeded its {time_limit:?} time limit"),
                            vec![],
                            started.elapsed().as_millis() as u64,
                        ),
                    }
                }
            };
            drop(permit);

            // Attach the wall-clock duration when the driver left it at 0.
            let mut result = result;
            if result.duration_ms == 0 {
                result.duration_ms = started.elapsed().as_millis() as u64;
            }
            state_tx.send_replace(result.state);
            let _ = result_tx.send(result);
        });

        AgentHandle::new(id, state_rx, result_rx, cancellation)
    }

    /// Spawn a batch of workers (each respecting the pool bound).
    pub fn spawn_many(
        &self,
        specs: Vec<AgentSpec>,
        driver: Arc<dyn AgentDriver>,
    ) -> Vec<AgentHandle> {
        specs
            .into_iter()
            .map(|spec| self.spawn(spec, Arc::clone(&driver)))
            .collect()
    }

    /// Collect every worker's final result (waits for all of them).
    pub async fn await_all(&self, handles: Vec<AgentHandle>) -> Vec<AgentResult> {
        futures::future::join_all(handles.into_iter().map(AgentHandle::await_result)).await
    }

    /// Ask every handle to stop (cooperative; workers stop at their next
    /// `check`).
    pub fn cancel_all(&self, handles: &[AgentHandle]) {
        for handle in handles {
            handle.cancel();
        }
    }

    /// Fold results into a single [`MergedReport`] for the planner.
    pub fn merge(&self, results: Vec<AgentResult>) -> MergedReport {
        crate::merge::merge(results)
    }
}
