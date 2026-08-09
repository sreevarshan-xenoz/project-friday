//! FRIDAY Investigation Engine — the **strategy layer** between a goal and
//! the planner.
//!
//! The planner decides *how to execute*. This engine decides **what
//! information is required before execution** — the difference between an
//! assistant (reacts to the goal) and a cognitive architecture (checks what
//! it does and does not know first).
//!
//! ```text
//! Goal
//!   │
//!   ▼
//! InvestigationEngine  (what do I need to know?)
//!   │   strategy: Heuristic (deterministic) | LLM (model-backed, heuristic fallback)
//!   ▼
//! InvestigationPlan    (channels + rationale + confidence)
//!   │
//!   ▼
//! Execution graph      ("watch FRIDAY think": investigate → plan → execute)
//!   │
//!   ▼
//! Planner → Runtime
//! ```
//!
//! Core-abstraction rule (per the architectural-gravity discipline): the
//! engine owns **epistemic strategy** — nothing else. It produces data
//! ([`InvestigationPlan`]) and a traceable view ([`InvestigationGraph`]); it
//! does not execute, route, or hold state. The planner's `PlanningContext`
//! consumes the plan (seam documented in `specs/investigation-engine.md` §8).
//!
//! The strategy seam is async (a model call is genuinely async). The default
//! is deterministic ([`HeuristicInvestigationStrategy`]); the drop-in
//! replacement is [`LlmInvestigationStrategy`] — the model derives the
//! requirements and the heuristic catches any model failure.
//!
//! ```rust,no_run
//! use friday_investigation::InvestigationEngine;
//!
//! #[tokio::main]
//! async fn main() {
//!     let engine = InvestigationEngine::heuristic();
//!     let plan = engine
//!         .investigate("Search the web for the latest Rust async docs")
//!         .await;
//!     assert!(plan.needs_investigation());
//! }
//! ```

pub mod channel;
pub mod engine;
pub mod heuristic;
pub mod llm;
pub mod plan;
pub mod strategy;

pub use channel::InvestigationChannel;
pub use engine::InvestigationEngine;
pub use heuristic::HeuristicInvestigationStrategy;
pub use llm::LlmInvestigationStrategy;
pub use plan::{
    InformationRequirement, InvestigationGraph, InvestigationPlan, InvestigationResult,
};
pub use strategy::{InvestigationStrategy, LlmStrategyClient};
