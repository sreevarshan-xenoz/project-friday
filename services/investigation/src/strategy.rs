//! `InvestigationStrategy` — the pluggable epistemic policy.
//!
//! The default strategy is deterministic
//! ([`crate::heuristic::HeuristicInvestigationStrategy`]); the model-backed
//! [`crate::llm::LlmInvestigationStrategy`] is a drop-in replacement that
//! falls back to the heuristic whenever the model fails. The engine and the
//! plan model stay decoupled from *how* requirements are derived — a strategy
//! can be swapped without touching either.
//!
//! The seam is async because a model call is genuinely async: a sync trait
//! would force a blocking hack inside the strategy layer. Deterministic
//! strategies simply return immediately when awaited.

use async_trait::async_trait;

use crate::plan::InvestigationPlan;

/// Analyses a goal and returns the information it requires.
#[async_trait]
pub trait InvestigationStrategy: Send + Sync {
    async fn investigate(&self, goal: &str) -> InvestigationPlan;
}

/// The minimal model surface an LLM-backed strategy needs.
///
/// Defined here so the strategy layer stays model-agnostic: the Inference
/// Fabric (or any OpenAI-compatible client) implements it at the wiring
/// point, and the investigation crate never depends on a model provider.
#[async_trait]
pub trait LlmStrategyClient: Send + Sync {
    /// Send one prompt and return the model's raw completion.
    async fn complete(&self, prompt: &str) -> Result<String, String>;
}

/// `Arc` passthrough so a client shared behind an `Arc` in an async runtime
/// (the natural way a daemon owns an `InferenceFabric`) implements the seam
/// without cloning or unwrapping.
#[async_trait]
impl<T: LlmStrategyClient + ?Sized> LlmStrategyClient for std::sync::Arc<T> {
    async fn complete(&self, prompt: &str) -> Result<String, String> {
        (**self).complete(prompt).await
    }
}
