//! The reasoning backend abstraction. A planner only needs "give me text, get
//! back text" — never the provider's internals. This keeps the planner free of
//! any hard dependency on `friday-intelligence` (which, in turn, depends on this
//! crate), so there is no cycle and a planner can be driven by a mock in tests
//! or by `InferenceFabric` in production behind the `intelligence` feature.

use async_trait::async_trait;
use friday_core::trace::ModelUsage;

/// The result of a model completion, carrying both the raw text response and
/// optional usage metadata. The `usage` field is `None` when the backend
/// cannot provide token/cost information (e.g. local models, mocks).
///
/// ```text
/// CompletionResult
///   ├── content: String       raw model response text
///   └── usage: Option<ModelUsage>
///         ├── model / provider identity
///         ├── tokens_in / tokens_out
///         ├── cached_tokens / reasoning_tokens (optional)
///         ├── cost_usd (optional; computed by the backend layer)
///         └── finish_reason (optional)
/// ```
#[derive(Debug, Clone)]
pub struct CompletionResult {
    pub content: String,
    pub usage: Option<ModelUsage>,
}

impl CompletionResult {
    /// A result with content only (no usage data).
    pub fn text(content: impl Into<String>) -> Self {
        Self {
            content: content.into(),
            usage: None,
        }
    }

    /// A result with content and usage metadata.
    pub fn with_usage(content: impl Into<String>, usage: ModelUsage) -> Self {
        Self {
            content: content.into(),
            usage: Some(usage),
        }
    }
}

/// A minimal chat-completion backend. Implemented by a mock in tests and by an
/// adapter over `InferenceFabric` (see `crate::intelligence_backend`).
#[async_trait]
pub trait ReasoningBackend: Send + Sync {
    /// Run a single prompt and return the model's raw text response.
    async fn complete(&self, prompt: &str) -> Result<String, String>;

    /// Run a single prompt and return a [`CompletionResult`] with optional
    /// usage metadata. The default implementation delegates to [`complete`](Self::complete)
    /// and returns `None` for usage — backends that can provide token counts
    /// should override this method.
    async fn complete_with_usage(&self, prompt: &str) -> Result<CompletionResult, String> {
        let content = self.complete(prompt).await?;
        Ok(CompletionResult::text(content))
    }
}

/// Errors a planner can return. Validation failures carry the list of problems
/// so the caller (or a future human-in-the-loop UI) can show precisely what is
/// wrong before the supervisor ever sees the graph.
#[derive(Debug, Clone)]
pub enum PlanError {
    /// The reasoning backend failed (network/provider error).
    Backend(String),
    /// The model returned text that could not be parsed into a plan.
    Parse(String),
    /// The produced graph failed deterministic validation.
    Validation(Vec<String>),
}

impl std::fmt::Display for PlanError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PlanError::Backend(e) => write!(f, "backend error: {e}"),
            PlanError::Parse(e) => write!(f, "parse error: {e}"),
            PlanError::Validation(issues) => {
                write!(f, "validation failed:\n- {}", issues.join("\n- "))
            }
        }
    }
}

impl std::error::Error for PlanError {}

/// A deterministic backend for tests: returns a fixed response, optionally
/// varying by a substring of the prompt. Lets the `LlmPlanner` be exercised
/// end-to-end without a network or API key.
pub struct MockBackend {
    /// The exact response text to return. May contain `{goal}` which is replaced
    /// by the goal string from the prompt (best-effort). For most tests a static
    /// JSON plan is sufficient.
    pub response: String,
}

impl MockBackend {
    pub fn new(response: impl Into<String>) -> Self {
        Self {
            response: response.into(),
        }
    }
}

#[async_trait]
impl ReasoningBackend for MockBackend {
    async fn complete(&self, _prompt: &str) -> Result<String, String> {
        Ok(self.response.clone())
    }
}
