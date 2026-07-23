//! In-process mock of an LLM provider, used by `--mock` so the harness can qualify
//! the Inference Fabric's failure handling deterministically — without a live gateway
//! or network access. Mirrors the real `Provider` trait and can be configured to
//! succeed or to fail in each of the corpus's injected fault modes. `MockProviderV2`
//! mirrors the V2 `ProviderV2` surface (streaming, dynamic discovery, embeddings) so
//! the new capability benchmarks run offline too.

use std::pin::Pin;

use async_trait::async_trait;
use futures::stream;

use friday_intelligence::{
    CompletionChunk, EmbeddingResult, ModelDescriptor, Provider, ProviderCapabilities,
    ProviderMetadata, ProviderV2,
};
use futures::Stream;

/// The fault a mock provider should exhibit. `Ok` is the only success mode; every
/// other variant returns `Err` from `complete` (and reports unhealthy from
/// `check_health`) so the Fabric is forced to react.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FaultMode {
    Ok,
    Timeout,
    Http429,
    Http500,
    Malformed,
    Disconnect,
}

impl FaultMode {
    pub fn from_injection(inj: crate::corpus::Injection) -> Self {
        match inj {
            crate::corpus::Injection::Timeout => FaultMode::Timeout,
            crate::corpus::Injection::Http429 => FaultMode::Http429,
            crate::corpus::Injection::Http500 => FaultMode::Http500,
            crate::corpus::Injection::MalformedJson => FaultMode::Malformed,
            crate::corpus::Injection::Disconnect => FaultMode::Disconnect,
        }
    }
}

/// A `Provider` that returns a fixed response or a fixed fault on every call.
pub struct MockProvider {
    pub name: String,
    pub fault: FaultMode,
    pub ok_response: String,
    /// When healthy and prompt-aware, the mock picks a canned response that
    /// satisfies the corpus's content verifiers (JSON object / array of 50), so
    /// `--mock` exercises the entire harness pipeline without a live gateway.
    pub prompt_aware: bool,
}

impl MockProvider {
    pub fn new(name: String, fault: FaultMode) -> Self {
        Self {
            name,
            fault,
            ok_response: "pong".to_string(),
            prompt_aware: false,
        }
    }

    pub fn healthy(name: String) -> Self {
        Self {
            name,
            fault: FaultMode::Ok,
            ok_response: "{\"name\":\"friday\",\"age\":1,\"tags\":[\"ai\",\"agent\"]}".to_string(),
            prompt_aware: true,
        }
    }
}

#[async_trait]
impl Provider for MockProvider {
    fn name(&self) -> &str {
        &self.name
    }

    async fn complete(
        &self,
        _model_id: &str,
        prompt: &str,
        _require_json: bool,
    ) -> Result<String, String> {
        match self.fault {
            FaultMode::Ok => {
                if self.prompt_aware && prompt.contains("array of exactly 50") {
                    let arr: Vec<String> = (1..=50).map(|i| i.to_string()).collect();
                    Ok(format!("[{}]", arr.join(",")))
                } else {
                    Ok(self.ok_response.clone())
                }
            }
            FaultMode::Timeout => Err("timeout: request timed out".into()),
            FaultMode::Http429 => {
                Err("Provider mock returned error status 429: rate limited".into())
            }
            FaultMode::Http500 => Err("Provider mock returned error status 500: internal".into()),
            FaultMode::Malformed => {
                Err("Failed to parse response JSON: unexpected end of body".into())
            }
            FaultMode::Disconnect => Err("HTTP request failed: connection reset".into()),
        }
    }

    async fn check_health(&self) -> bool {
        self.fault == FaultMode::Ok
    }
}

/// A `ProviderV2` mirror of [`MockProvider`] so the Streaming and Dynamic Discovery
/// benchmarks run offline. Streaming splits the canned response into two chunks
/// (proving the harness reassembles incrementally); discovery returns one
/// well-shaped [`ModelDescriptor`]; embeddings return zero vectors of fixed dim.
pub struct MockProviderV2 {
    pub name: String,
    pub fault: FaultMode,
    pub ok_response: String,
    pub models: Vec<ModelDescriptor>,
    pub embedding_dim: usize,
}

impl MockProviderV2 {
    pub fn healthy(name: String) -> Self {
        Self {
            name,
            fault: FaultMode::Ok,
            ok_response: "the agent reasons then acts".to_string(),
            models: vec![ModelDescriptor {
                id: "mock-auto".to_string(),
                context: 200_000,
                supports_tools: true,
                supports_stream: true,
                supports_json: true,
                vision: true,
                pricing: None,
            }],
            embedding_dim: 4,
        }
    }

    fn metadata(&self) -> ProviderMetadata {
        ProviderMetadata {
            id: self.name.clone(),
            display_name: self.name.clone(),
            version: "v2".to_string(),
            endpoint: "http://mock".to_string(),
            vendor: self.name.clone(),
            priority: 100,
            cost_model: "free".to_string(),
        }
    }

    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities {
            completion: true,
            streaming: true,
            tool_calling: true,
            vision: true,
            embeddings: true,
            json_mode: true,
            long_context: true,
            local: false,
            session_affinity: true,
            dynamic_models: true,
        }
    }
}

#[async_trait]
impl ProviderV2 for MockProviderV2 {
    fn metadata(&self) -> ProviderMetadata {
        self.metadata()
    }

    fn capabilities(&self) -> ProviderCapabilities {
        self.capabilities()
    }

    async fn complete(
        &self,
        _model_id: &str,
        prompt: &str,
        _require_json: bool,
    ) -> Result<String, String> {
        if self.fault != FaultMode::Ok {
            return Err("mock v2 provider failed".into());
        }
        if prompt.contains("array of exactly 50") {
            let arr: Vec<String> = (1..=50).map(|i| i.to_string()).collect();
            Ok(format!("[{}]", arr.join(",")))
        } else {
            Ok(self.ok_response.clone())
        }
    }

    async fn stream(
        &self,
        _model_id: &str,
        _prompt: &str,
        _require_json: bool,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<CompletionChunk, String>> + Send>>, String> {
        if self.fault != FaultMode::Ok {
            return Err("mock v2 provider failed to stream".into());
        }
        let s = self.ok_response.clone();
        let mid = (s.len() / 2).max(1);
        let (a, b) = s.split_at(mid.min(s.len()));
        let chunks = vec![
            Ok(CompletionChunk {
                delta: a.to_string(),
                finish_reason: None,
            }),
            Ok(CompletionChunk {
                delta: b.to_string(),
                finish_reason: Some("stop".to_string()),
            }),
        ];
        Ok(Box::pin(stream::iter(chunks)))
    }

    async fn list_models(&self) -> Result<Vec<ModelDescriptor>, String> {
        if self.fault != FaultMode::Ok {
            return Err("mock v2 provider failed to list models".into());
        }
        Ok(self.models.clone())
    }

    async fn embeddings(
        &self,
        model_id: &str,
        texts: &[String],
    ) -> Result<EmbeddingResult, String> {
        if self.fault != FaultMode::Ok {
            return Err("mock v2 provider failed embeddings".into());
        }
        Ok(EmbeddingResult {
            model: model_id.to_string(),
            embeddings: texts
                .iter()
                .map(|_| vec![0.0f32; self.embedding_dim])
                .collect(),
        })
    }
}
