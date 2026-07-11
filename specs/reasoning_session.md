# Reasoning Session Specification (M5.7)

**Status:** 🚧 Planned. **Spec-first:** authoritative spec for FRIDAY's LLM session manager.

The Reasoning Session is the stateful coordinator that manages interactive model calls, token budgets, error recovery (self-correction loops), and provider fallbacks. This keeps model clients stateless.

```
                  Reasoning Session
                          │
          ┌───────────────┼───────────────┐
          ▼               ▼               ▼
    [Token Budget]  [Self-Correction] [Provider Failover]
```

## Session Lifecycle

When the Cognitive Engine receives a goal, it spawns a stateful `ReasoningSession`:

1. **Context Loading**: Fetches the context and compiles the prompt.
2. **Execution**: Routes the prompt to the primary provider (e.g., Gemini).
3. **Validation**: Passes the response to the Response Interpreter.
4. **Self-Correction (Retry Loop)**: If validation fails (e.g., JSON parse error or ontology violation), the session appends the error message back to the prompt, increases the retry count, and tries again.
5. **Failover**: If the primary provider times out or fails (HTTP 5xx), the session switches to the fallback provider (e.g., Claude or local Ollama) and logs the failure event to the telemetry channel.

## Struct Definitions

```rust
pub struct TokenUsage {
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub estimated_cost_usd: f64,
}

pub struct ReasoningSession {
    pub session_id: Uuid,
    pub history: Vec<SessionMessage>,
    pub token_budget: TokenUsage,
    pub max_retries: usize,
    pub active_provider: ModelProvider,
}

impl ReasoningSession {
    pub fn new(max_retries: usize) -> Self;

    /// Runs the request loop with automatic self-correction and provider failovers
    pub async fn execute_goal(
        &mut self,
        goal: &str,
        context_builder: &ContextBuilder,
        compiler: &PromptCompiler,
        interpreter: &ResponseInterpreter,
        providers: &HashMap<ModelProvider, Box<dyn ModelClient>>,
    ) -> Result<TaskGraph, String>;
}
```

## Self-Correction Loop Details
If the LLM returns an invalid JSON response or unknown capability:
```
LLM response: "Here is your plan: { ... }" (Fails JSON stripping)
Interpreter Error: "JSON syntax error..."
Reasoning Session prompt to LLM:
    "Error: The previous response failed validation with: 'JSON syntax error'. 
     Please correct this and return only raw JSON conforming to the schema."
```
Limits retries to 3 times before failing or attempting a provider failover.
