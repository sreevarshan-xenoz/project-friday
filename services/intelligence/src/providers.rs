use async_trait::async_trait;

/// Pluggable interface for calling large language models.
/// Isolating this ensures we can swap backends (e.g. Gemini to OpenAI)
/// without touching prompt compilers or response interpreters.
#[async_trait]
pub trait ModelClient: Send + Sync {
    async fn complete(&self, prompt: &str) -> Result<String, String>;
}

/// A mock model client used for deterministic unit/integration testing of the cognition layer.
pub struct MockModelClient {
    pub response: String,
}

#[async_trait]
impl ModelClient for MockModelClient {
    async fn complete(&self, _prompt: &str) -> Result<String, String> {
        Ok(self.response.clone())
    }
}
