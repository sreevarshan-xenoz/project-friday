//! Adapter that lets the planner's LLM planner use `InferenceFabric` as its
//! reasoning backend. `InferenceFabric` already implements `ModelClient`; this
//! wraps it as a `friday_planner::planning::planners::ReasoningBackend` so the
//! daemon can wire real model planning with one line:
//!
//! ```ignore
//! register_engines_with_llm(friday, Some(Box::new(InferenceBackend::from_env()))).await?;
//! ```

use std::sync::Arc;

use async_trait::async_trait;

use friday_core::Friday;
use friday_planner::planning::planners::ReasoningBackend as PlannerBackend;
use friday_planner::register_engines_with_llm;

use crate::providers::{InferenceFabric, ModelClient};

/// Thin adapter over [`InferenceFabric`] satisfying the planner's
/// `ReasoningBackend` contract.
pub struct InferenceBackend {
    fabric: Arc<InferenceFabric>,
}

impl InferenceBackend {
    pub fn new(fabric: Arc<InferenceFabric>) -> Self {
        Self { fabric }
    }

    /// Build a fabric from the environment (API keys / model registry / local
    /// Ollama) and wrap it as a planner backend.
    pub fn from_env() -> Self {
        Self::new(Arc::new(InferenceFabric::default_from_env()))
    }
}

#[async_trait]
impl PlannerBackend for InferenceBackend {
    async fn complete(&self, prompt: &str) -> Result<String, String> {
        self.fabric.complete(prompt).await
    }
}

/// Register the full planner stack (intent → planning → execution → supervisor)
/// with the real `InferenceFabric` as the LLM planner's reasoning backend. This
/// is the single wiring point that makes every reasoning operation — planning,
/// and later reflection/summarization/repair — flow through `InferenceFabric`.
///
/// If no providers are configured in the environment, the fabric's `complete`
/// returns an error and the planner transparently falls back to the rule
/// planner, so this is always safe to call.
pub async fn register_with_fabric(
    friday: &mut Friday,
) -> friday_core::Result<std::sync::Arc<friday_planner::execution::capability_registry::CapabilityRegistry>>
{
    let backend: Box<dyn PlannerBackend> = Box::new(InferenceBackend::from_env());
    register_engines_with_llm(friday, Some(backend)).await
}
