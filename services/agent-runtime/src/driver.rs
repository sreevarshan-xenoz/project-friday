//! [`AgentDriver`] — the seam between the runtime and a worker's actual work.
//!
//! The runtime owns lifecycle (spawn, cancellation, timeout, budget, mailbox);
//! a driver owns *what the worker does*. This keeps the orchestration crate
//! independent of the planner/skills/actions crates: `friday-context` supplies
//! a `SkillAgentDriver` that runs a goal through the skill engine scoped to
//! the agent's allowed skills, while tests use the closure-backed
//! [`FnDriver`] / [`AsyncFnDriver`].

use async_trait::async_trait;

use crate::context::AgentContext;
use crate::spec::AgentResult;

#[async_trait]
pub trait AgentDriver: Send + Sync {
    /// Execute one worker. The driver should call
    /// [`AgentContext::check`] between steps and return early with the
    /// corresponding abort state when it fails. The runtime wraps the whole
    /// call in a hard timeout regardless.
    async fn run(&self, ctx: AgentContext) -> AgentResult;
}

/// Wrap a synchronous closure as a driver — the ergonomic mock surface for
/// tests with quick, non-blocking workers.
pub struct FnDriver<F> {
    f: F,
}

impl<F> FnDriver<F>
where
    F: Fn(AgentContext) -> AgentResult + Send + Sync,
{
    pub fn new(f: F) -> Self {
        Self { f }
    }
}

#[async_trait]
impl<F> AgentDriver for FnDriver<F>
where
    F: Fn(AgentContext) -> AgentResult + Send + Sync,
{
    async fn run(&self, ctx: AgentContext) -> AgentResult {
        (self.f)(ctx)
    }
}

/// Wrap an async closure as a driver — the mock surface for workers that must
/// actually await (e.g. `tokio::time::sleep` for timeout/concurrency tests).
pub struct AsyncFnDriver<F> {
    f: F,
}

impl<F> AsyncFnDriver<F>
where
    F: Fn(AgentContext) -> Pin<Box<dyn Future<Output = AgentResult> + Send>> + Send + Sync,
{
    pub fn new(f: F) -> Self {
        Self { f }
    }
}

use std::future::Future;
use std::pin::Pin;

#[async_trait]
impl<F> AgentDriver for AsyncFnDriver<F>
where
    F: Fn(AgentContext) -> Pin<Box<dyn Future<Output = AgentResult> + Send>> + Send + Sync,
{
    async fn run(&self, ctx: AgentContext) -> AgentResult {
        (self.f)(ctx).await
    }
}
