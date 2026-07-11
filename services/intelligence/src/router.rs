use serde::{Deserialize, Serialize};

/// Cognitive System modes mapping to System 1 (fast/reactive) and System 2 (slow/reflective).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CognitiveMode {
    /// Fast, low-latency, rule-driven or direct capability execution.
    System1,
    /// Slow, plannable, simulated, and verified multi-step reasoning.
    System2,
}

pub struct IntelligenceRouter;

impl IntelligenceRouter {
    /// Route the goal to either System 1 or System 2.
    pub fn route(goal: &str) -> CognitiveMode {
        let clean = goal.trim().to_lowercase();
        
        // System 1 greetings, status checks, and simple one-step commands
        let simple_phrases = [
            "hello", "hi", "hey", "who are you", "what is your name", 
            "what time is it", "current date", "ping", "help"
        ];
        let simple_actions = [
            "open notepad", "open chrome", "open calculator", "open calc",
            "git status", "list files", "clear clipboard", "get clipboard"
        ];

        // Match exact or prefix matching for chitchat/status checks
        if simple_phrases.iter().any(|&p| clean == p || clean.starts_with(p)) {
            return CognitiveMode::System1;
        }

        // Match exact simple command mappings
        if simple_actions.iter().any(|&a| clean == a) {
            return CognitiveMode::System1;
        }

        // Multi-step complex tasks default to System 2 (reasoning, planning, simulation)
        CognitiveMode::System2
    }
}
