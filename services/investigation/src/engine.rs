//! `InvestigationEngine` — the strategy-layer entry point.
//!
//! Owns **epistemic strategy only**: given a goal, produce the information
//! requirements and the traceable view. It executes nothing, routes nothing,
//! and holds no state beyond its strategy.

use crate::heuristic::HeuristicInvestigationStrategy;
use crate::llm::LlmInvestigationStrategy;
use crate::plan::{InvestigationGraph, InvestigationPlan};
use crate::strategy::{InvestigationStrategy, LlmStrategyClient};

/// The strategy layer: goal → [`InvestigationPlan`] → [`InvestigationGraph`].
pub struct InvestigationEngine {
    strategy: Box<dyn InvestigationStrategy>,
}

impl InvestigationEngine {
    /// Wrap a custom strategy (heuristic, rule, LLM). Any strategy works —
    /// the engine only calls [`InvestigationStrategy::investigate`].
    pub fn new(strategy: Box<dyn InvestigationStrategy>) -> Self {
        Self { strategy }
    }

    /// The deterministic default strategy.
    pub fn heuristic() -> Self {
        Self::new(Box::new(HeuristicInvestigationStrategy::default()))
    }

    /// The model-backed strategy: the LLM derives the requirements and the
    /// heuristic is the fallback whenever the model fails. Drop-in for
    /// [`InvestigationEngine::heuristic`].
    pub fn llm(client: Box<dyn LlmStrategyClient>) -> Self {
        Self::new(Box::new(LlmInvestigationStrategy::new(client)))
    }

    /// What information does this goal require before execution?
    pub async fn investigate(&self, goal: &str) -> InvestigationPlan {
        self.strategy.investigate(goal).await
    }

    /// The "watch FRIDAY think" view: goal → branches → phases.
    pub async fn graph(&self, goal: &str) -> InvestigationGraph {
        InvestigationGraph::new(goal, &self.investigate(goal).await)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::channel::InvestigationChannel;
    use crate::plan::InformationRequirement;
    use crate::strategy::LlmStrategyClient;
    use async_trait::async_trait;

    #[tokio::test]
    async fn engine_runs_the_default_strategy() {
        let engine = InvestigationEngine::heuristic();
        let plan = engine
            .investigate("Search the web for a guide on async Rust")
            .await;
        assert!(plan.needs_investigation());
        assert!(plan.channels().contains(&InvestigationChannel::Web));
    }

    #[tokio::test]
    async fn graph_is_the_traceable_view() {
        let engine = InvestigationEngine::heuristic();
        let g = engine
            .graph("Look up the git branches and modified files")
            .await;
        assert_eq!(g.goal, "Look up the git branches and modified files");
        assert!(g.phases.contains(&"investigate".to_string()));
        assert!(g
            .requirements
            .iter()
            .any(|r| r.channel == InvestigationChannel::Git));
    }

    #[tokio::test]
    async fn strategy_is_pluggable() {
        struct FixedStrategy;
        #[async_trait]
        impl InvestigationStrategy for FixedStrategy {
            async fn investigate(&self, _goal: &str) -> InvestigationPlan {
                InvestigationPlan {
                    requirements: vec![InformationRequirement {
                        channel: InvestigationChannel::Memory,
                        rationale: "fixed policy".into(),
                        confidence: 1.0,
                    }],
                }
            }
        }
        let engine = InvestigationEngine::new(Box::new(FixedStrategy));
        let plan = engine.investigate("anything").await;
        assert_eq!(plan.channels(), vec![InvestigationChannel::Memory]);
        assert_eq!(plan.requirements[0].rationale, "fixed policy");
    }

    /// The `llm()` constructor is a drop-in for `heuristic()`: same engine,
    /// same call, model-derived plan.
    #[tokio::test]
    async fn llm_constructor_is_a_drop_in_for_heuristic() {
        struct ScriptedClient;
        #[async_trait]
        impl LlmStrategyClient for ScriptedClient {
            async fn complete(&self, _prompt: &str) -> Result<String, String> {
                Ok(r#"{"requirements":[{"channel":"git","rationale":"repo status","confidence":0.85}]}"#
                    .to_string())
            }
        }
        let engine = InvestigationEngine::llm(Box::new(ScriptedClient));
        let plan = engine.investigate("Show me the repo status").await;
        assert_eq!(plan.channels(), vec![InvestigationChannel::Git]);
        assert_eq!(plan.requirements[0].confidence, 0.85);

        // The same engine's graph path works too.
        let g = engine.graph("Show me the repo status").await;
        assert_eq!(g.requirements.len(), 1);
        assert!(g.phases.contains(&"investigate".to_string()));
    }
}
