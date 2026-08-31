//! FRIDAY OS Context Collector (R5-2-5-1).
//!
//! Pure OS capability that captures per-channel context
//! (`Screen` / `Selection` / `App` / `Clipboard`) behind a testable command
//! boundary. It knows **nothing** about consent, providers, privacy tiers, or
//! egress policy — the R5-2-4 [`ContextGate`] and `EgressGate` sit downstream
//! and are the only places those decisions live.
//!
//! ```text
//! ContextChannel ──▶ ContextCollector ──▶ CapturedContext
//!                        │
//!              ┌─────────┴─────────┐
//!        FakeCommandRunner   RealCommandRunner
//!        (Windows CI/tests)   (Omarchy/Linux qual)
//! ```

pub mod collector;
pub mod command;

pub use collector::{
    CaptureError, CaptureOptions, CapturedContext, ContextCollector, ScreenRegion, TextSource,
};
pub use command::{
    CommandOutput, CommandRunner, FakeCommandRunner, RealCommandRunner, RunnerError,
};
pub use friday_intelligence::ContextChannel;
