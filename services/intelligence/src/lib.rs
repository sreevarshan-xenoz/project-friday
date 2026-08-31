pub mod compiler;
pub mod context_gate;
pub mod egress;
pub mod parser;
pub mod planner_backend;
pub mod providers;
pub mod qualification;
pub mod router;
pub mod secrets;

pub use compiler::{ModelProvider, PromptAst, PromptCompiler};
pub use context_gate::{
    compose, ContextChannel, ContextComposeError, ContextDecision, ContextGate, ProviderTarget,
};
pub use egress::{EgressAction, EgressGate, EgressMode, EgressVerdict};
pub use parser::{LlmResponse, ParsedTask, ResponseInterpreter};
pub use planner_backend::InferenceBackend;
pub use providers::{
    classify_failure_reason, opengateway_models, sanitize_candidate_error, CandidateConsideration,
    CandidateFailure, CapabilityTier, CircuitBreaker, CircuitState, CompletionChunk,
    EmbeddingResult, InferenceFabric, MockModelClient, ModelCapability, ModelClient,
    ModelDescriptor, ModelInfo, OmniRouteProvider, OmniRouteProviderV2, OpenAiProvider,
    PersistedProviderHealth, Provider, ProviderCapabilities, ProviderMetadata, ProviderV2,
    ProviderV2Adapter, RoutingDecision, RoutingMode,
};
pub use qualification::{
    routing_score, BenchmarkConfig, Budget, CandidateView, Preference, ProviderHealth,
    QualificationProfile, RequestRequirements, Requirement, QUALIFICATION_SCHEMA_VERSION,
};
pub use router::{CognitiveMode, IntelligenceRouter};
pub use secrets::{
    looks_like_secret, redact_payload, scan_payload, ScopedSecrets, Secret, SecretHit,
    SecretsVault, MIN_SCAN_LENGTH,
};
