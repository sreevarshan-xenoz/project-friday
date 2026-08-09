//! FRIDAY Agent Runtime — the sub-agent orchestrator (`specs/agent-runtime.md`
//! Part II: Agent Spawning & Orchestration).
//!
//! The runtime owns *how* worker agents run — spawning, lifecycle,
//! cancellation, deadlines, budgets, bounded concurrency, and result merge —
//! never *what* they do. A worker's behavior is supplied as an
//! [`AgentDriver`]; `friday-context` implements the skill-scoped driver that
//! runs a goal through the skill engine restricted to the agent's allowed
//! skills.
//!
//! ```text
//! Planner (orchestrator)
//!     │  spawn(AgentSpec, driver)
//!     ▼
//! AgentRuntime ──▶ AgentHandle (mailbox: state + result + cancel)
//!     │                │
//!     ▼                ▼
//! worker task ──result─▶ await_all ──▶ merge ──▶ MergedReport
//! ```

pub mod budget;
pub mod cancellation;
pub mod context;
pub mod driver;
pub mod handle;
pub mod merge;
pub mod runtime;
pub mod scheduling;
pub mod spec;

pub use budget::{BudgetLimits, BudgetTracker};
pub use cancellation::CancellationToken;
pub use context::{AgentAbort, AgentContext};
pub use driver::{AgentDriver, AsyncFnDriver, FnDriver};
pub use handle::AgentHandle;
pub use merge::{merge, MergedReport};
pub use runtime::{AgentRuntime, DEFAULT_MAX_CONCURRENCY, DEFAULT_TIME_LIMIT};
pub use scheduling::{DecomposeHint, Decomposer, HeuristicDecomposer};
pub use spec::{AgentResult, AgentSpec, AgentState, AgentStep, ModelCapability};
