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

    /// Like [`LlmStrategyClient::complete`], reporting the answer as it
    /// arrives: `progress` gets the text so far each time it grows. A client
    /// that can't stream reports the whole answer once.
    async fn complete_with_progress(
        &self,
        prompt: &str,
        progress: &(dyn for<'s> Fn(&'s str) + Send + Sync),
    ) -> Result<String, String> {
        let text = self.complete(prompt).await?;
        progress(&text);
        Ok(text)
    }

    /// A second, more careful answer (the person asked for a better model):
    /// like [`LlmStrategyClient::complete_with_progress`], but ranked for
    /// answer quality rather than speed, and not on `avoid` (the
    /// `(provider, model)` that gave the last answer) while another model
    /// can take it. A client with one model just answers again.
    async fn complete_thorough_with_progress(
        &self,
        prompt: &str,
        avoid: Option<(&str, &str)>,
        progress: &(dyn for<'s> Fn(&'s str) + Send + Sync),
    ) -> Result<String, String> {
        let _ = avoid;
        self.complete_with_progress(prompt, progress).await
    }
}

/// `Arc` passthrough so a client shared behind an `Arc` in an async runtime
/// (the natural way a daemon owns an `InferenceFabric`) implements the seam
/// without cloning or unwrapping.
#[async_trait]
impl<T: LlmStrategyClient + ?Sized> LlmStrategyClient for std::sync::Arc<T> {
    async fn complete(&self, prompt: &str) -> Result<String, String> {
        (**self).complete(prompt).await
    }
    async fn complete_with_progress(
        &self,
        prompt: &str,
        progress: &(dyn for<'s> Fn(&'s str) + Send + Sync),
    ) -> Result<String, String> {
        (**self).complete_with_progress(prompt, progress).await
    }
    async fn complete_thorough_with_progress(
        &self,
        prompt: &str,
        avoid: Option<(&str, &str)>,
        progress: &(dyn for<'s> Fn(&'s str) + Send + Sync),
    ) -> Result<String, String> {
        (**self)
            .complete_thorough_with_progress(prompt, avoid, progress)
            .await
    }
}
