use crate::phase::CognitivePhase;
use crate::workspace::GlobalWorkspace;
use async_trait::async_trait;
use std::pin::Pin;
use std::future::Future;

/// The result of a phase handler executing.
#[derive(Debug, Clone)]
pub enum PhaseResult {
    /// Phase completed successfully.
    Ok,
    /// Phase completed and published events to the workspace.
    Published { topics: Vec<String> },
    /// Phase wants to skip the remaining phases in this tick.
    SkipRemaining,
    /// Phase encountered an error but the tick can continue.
    Error { message: String },
}

/// A boxed async future that returns PhaseResult.
type BoxFuture = Pin<Box<dyn Future<Output = PhaseResult> + Send>>;

/// Closure type for FnHandler.
type HandlerFn = dyn Fn(CognitivePhase, GlobalWorkspace, u64) -> BoxFuture + Send + Sync;

/// Trait that every engine implements to participate in the cognitive loop.
///
/// An engine registers for one or more phases. When the runtime reaches
/// that phase, it calls `execute` on every handler registered for it.
///
/// The handler reads from and writes to the Global Workspace.
/// It does NOT call other engines directly. Everything goes through the workspace.
#[async_trait]
pub trait PhaseHandler: Send + Sync {
    /// Human-readable name for this handler (for logging/metrics).
    fn name(&self) -> &str;

    /// Which phases this handler wants to participate in.
    fn phases(&self) -> Vec<CognitivePhase>;

    /// Execute this handler for the given phase.
    async fn execute(
        &self,
        phase: CognitivePhase,
        workspace: &GlobalWorkspace,
        tick_number: u64,
    ) -> PhaseResult;
}

/// Wrapper that makes any async closure into a PhaseHandler.
pub struct FnHandler {
    name: String,
    phases: Vec<CognitivePhase>,
    func: Box<HandlerFn>,
}

impl FnHandler {
    pub fn new<F>(
        name: impl Into<String>,
        phases: Vec<CognitivePhase>,
        func: F,
    ) -> Self
    where
        F: Fn(CognitivePhase, GlobalWorkspace, u64) -> BoxFuture + Send + Sync + 'static,
    {
        Self {
            name: name.into(),
            phases,
            func: Box::new(func),
        }
    }
}

#[async_trait]
impl PhaseHandler for FnHandler {
    fn name(&self) -> &str {
        &self.name
    }

    fn phases(&self) -> Vec<CognitivePhase> {
        self.phases.clone()
    }

    async fn execute(
        &self,
        phase: CognitivePhase,
        workspace: &GlobalWorkspace,
        tick_number: u64,
    ) -> PhaseResult {
        (self.func)(phase, workspace.clone(), tick_number).await
    }
}
