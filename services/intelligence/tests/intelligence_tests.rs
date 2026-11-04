use friday_context::AgentContext;
use friday_intelligence::{
    CognitiveMode, IntelligenceRouter, ModelProvider, PromptCompiler, ResponseInterpreter,
};

#[test]
fn test_intelligence_router() {
    // System 1 chitchat/greeting matches
    assert_eq!(
        IntelligenceRouter::route("Hello there!"),
        CognitiveMode::System1
    );
    assert_eq!(
        IntelligenceRouter::route("what time is it"),
        CognitiveMode::System1
    );

    // System 1 direct command matches
    assert_eq!(
        IntelligenceRouter::route("open notepad"),
        CognitiveMode::System1
    );

    // System 2 complex plannable actions
    assert_eq!(
        IntelligenceRouter::route(
            "compile the main Rust application and run tests, then commit changes to git"
        ),
        CognitiveMode::System2
    );
}

#[test]
fn test_prompt_compiler_formats() {
    let context = AgentContext {
        timestamp: chrono::Utc::now(),
        goal: "compile cargo project".to_string(),
        capabilities: vec![],
        healthy_services: vec!["terminal".to_string()],
        relevant_memories: vec![],
        experiences: vec![],
        current_project: "/src".to_string(),
        active_sessions: vec![],
        active_windows: vec![],
        skills: vec![],
        active_skill: None,
    };

    let ast = PromptCompiler::compile_ast(&context);
    assert_eq!(ast.goal, "compile cargo project");
    assert!(ast.active_state.contains("terminal"));

    // Test Claude XML format compiler
    let claude_str = PromptCompiler::compile_to_string(&ast, ModelProvider::Claude);
    assert!(claude_str.contains("<system>"));
    assert!(claude_str.contains("<user_goal>"));

    // Test Ollama compact format compiler
    let ollama_str = PromptCompiler::compile_to_string(&ast, ModelProvider::Ollama);
    assert!(ollama_str.contains("INSTRUCTION:"));
    assert!(ollama_str.contains("GOAL:"));

    // Test Gemini/OpenAI markdown format compiler
    let gemini_str = PromptCompiler::compile_to_string(&ast, ModelProvider::Gemini);
    assert!(gemini_str.contains("### System Instruction"));
    assert!(gemini_str.contains("### Target Goal"));
}

#[test]
fn test_response_interpreter_validations() {
    let known_caps = vec![
        "filesystem.create_file".to_string(),
        "windows.open".to_string(),
        "windows.close".to_string(),
    ];

    // Happy Path: clean JSON with code fences
    let raw_response = r#"
    ```json
    {
        "goal": "create file",
        "intent": "create",
        "confidence": 0.95,
        "tasks": [
            {
                "name": "Create README",
                "capability": "filesystem.create_file",
                "params": {"path": "README.md"},
                "depends_on_indices": []
            }
        ]
    }
    ```
    "#;

    let parsed = ResponseInterpreter::parse_response(raw_response, &known_caps).unwrap();
    assert_eq!(parsed.intent, "create");
    assert_eq!(parsed.tasks.len(), 1);
    assert_eq!(parsed.tasks[0].capability, "filesystem.create_file");

    // Compile parsed to TaskGraph check
    let graph = ResponseInterpreter::compile_to_task_graph(&parsed);
    assert_eq!(graph.intent, "create");
    assert_eq!(graph.tasks.len(), 1);
    assert_eq!(graph.tasks[0].capability, "filesystem.create_file");

    // Ontology Violation Check (Fuzzy matches suggesting windows.open)
    let bad_response = r#"
    {
        "goal": "open notepad",
        "intent": "open",
        "confidence": 0.90,
        "tasks": [
            {
                "name": "Open",
                "capability": "windows.launch",
                "params": {"app": "notepad"},
                "depends_on_indices": []
            }
        ]
    }
    "#;

    let parse_err = ResponseInterpreter::parse_response(bad_response, &known_caps).unwrap_err();
    assert!(parse_err.contains("Ontology Violation"));
    assert!(parse_err.contains("windows.open") || parse_err.contains("windows.close"));
    // suggestion check
}

struct TestProvider {
    name: String,
    succeed: bool,
    response: String,
    /// Custom failure text returned by `complete()` when `succeed == false`.
    /// Lets a test inject a credential-bearing error to pin credential hygiene.
    failure_text: Option<String>,
    calls: std::sync::Arc<std::sync::atomic::AtomicUsize>,
}

impl TestProvider {
    fn new(name: &str, succeed: bool, response: &str) -> Self
    where
        Self: Sized,
    {
        Self {
            name: name.to_string(),
            succeed,
            response: response.to_string(),
            failure_text: None,
            calls: std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0)),
        }
    }

    fn failing_with(name: &str, error: &str) -> Self {
        Self {
            name: name.to_string(),
            succeed: false,
            response: String::new(),
            failure_text: Some(error.to_string()),
            calls: std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0)),
        }
    }
}

#[async_trait::async_trait]
impl friday_intelligence::Provider for TestProvider {
    fn name(&self) -> &str {
        &self.name
    }
    async fn complete(
        &self,
        _model_id: &str,
        _prompt: &str,
        _require_json: bool,
    ) -> Result<String, String> {
        self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        if self.succeed {
            Ok(self.response.clone())
        } else {
            Err(self
                .failure_text
                .clone()
                .unwrap_or_else(|| "Simulated failure".to_string()))
        }
    }
    async fn check_health(&self) -> bool {
        self.succeed
    }
}

#[tokio::test]
async fn test_inference_fabric_failover() {
    use friday_intelligence::{
        InferenceFabric, ModelCapability, ModelClient, ModelInfo, RoutingMode,
    };

    let mut fabric = InferenceFabric::new();
    fabric.default_mode = RoutingMode::Hybrid;

    let calls_p1 = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let calls_p2 = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));

    // ollama: fails, latency=100ms
    fabric.register_provider(Box::new(TestProvider {
        name: "ollama".to_string(),
        succeed: false,
        response: "".to_string(),
        failure_text: None,
        calls: calls_p1.clone(),
    }));
    fabric.register_model(ModelInfo {
        id: "local-llama".to_string(),
        family: "llama".to_string(),
        provider: "ollama".to_string(),
        latency_ms: 100,
        reliability: 1.0,
        capabilities: vec![ModelCapability::Local],
        max_context: 2048,
        tier: None,
    });

    // openrouter: succeeds, latency=500ms
    fabric.register_provider(Box::new(TestProvider {
        name: "openrouter".to_string(),
        succeed: true,
        response: "Target Answer".to_string(),
        failure_text: None,
        calls: calls_p2.clone(),
    }));
    fabric.register_model(ModelInfo {
        id: "cloud-gemini".to_string(),
        family: "gemini".to_string(),
        provider: "openrouter".to_string(),
        latency_ms: 500,
        reliability: 1.0,
        capabilities: vec![],
        max_context: 8192,
        tier: None,
    });

    // First request: ollama (faster) is tried, fails, then openrouter (slower) succeeds
    let res = fabric.complete("Hello").await.unwrap();
    assert_eq!(res, "Target Answer");
    assert_eq!(calls_p1.load(std::sync::atomic::Ordering::SeqCst), 1);
    assert_eq!(calls_p2.load(std::sync::atomic::Ordering::SeqCst), 1);

    // Second request: openrouter is now ranked higher because ollama's health degraded to 0.8.
    // So openrouter is tried first and succeeds directly.
    let res2 = fabric.complete("Hello again").await.unwrap();
    assert_eq!(res2, "Target Answer");
    assert_eq!(calls_p1.load(std::sync::atomic::Ordering::SeqCst), 1); // still 1
    assert_eq!(calls_p2.load(std::sync::atomic::Ordering::SeqCst), 2); // now 2
}

#[tokio::test]
async fn test_inference_fabric_advanced_features() {
    use friday_intelligence::{
        CircuitState, InferenceFabric, ModelCapability, ModelClient, ModelInfo, RoutingMode,
    };

    let mut fabric = InferenceFabric::new();
    fabric.default_mode = RoutingMode::Hybrid;

    let calls_p1 = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let calls_p2 = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));

    // ollama: fails 3 times to trip circuit, latency=100ms
    fabric.register_provider(Box::new(TestProvider {
        name: "ollama".to_string(),
        succeed: false,
        response: "".to_string(),
        failure_text: None,
        calls: calls_p1.clone(),
    }));
    fabric.register_model(ModelInfo {
        id: "local-llama".to_string(),
        family: "llama".to_string(),
        provider: "ollama".to_string(),
        latency_ms: 100,
        reliability: 1.0,
        capabilities: vec![ModelCapability::Json, ModelCapability::Local],
        max_context: 2048,
        tier: None,
    });

    // openrouter: succeeds, latency=500ms, requires Vision capability for capability testing
    fabric.register_provider(Box::new(TestProvider {
        name: "openrouter".to_string(),
        succeed: true,
        response: "Vision Answer".to_string(),
        failure_text: None,
        calls: calls_p2.clone(),
    }));
    fabric.register_model(ModelInfo {
        id: "cloud-gemini".to_string(),
        family: "gemini".to_string(),
        provider: "openrouter".to_string(),
        latency_ms: 500,
        reliability: 1.0,
        capabilities: vec![ModelCapability::Json, ModelCapability::Vision],
        max_context: 8192,
        tier: None,
    });

    // 1. Test Capability-Based Routing:
    // If fabric requires Vision, ollama (which lacks it) must be skipped entirely
    fabric.required_capabilities = vec![ModelCapability::Vision];
    let res = fabric.complete("Hello").await.unwrap();
    assert_eq!(res, "Vision Answer");
    assert_eq!(calls_p1.load(std::sync::atomic::Ordering::SeqCst), 0); // skipped!
    assert_eq!(calls_p2.load(std::sync::atomic::Ordering::SeqCst), 1);

    // 2. Test Circuit Breaker:
    // Restrict required capabilities to Local to force all calls to ollama
    fabric.required_capabilities = vec![ModelCapability::Local];

    // Call 1: fails
    let _ = fabric.complete("Hello").await.unwrap_err();
    // Call 2: fails
    let _ = fabric.complete("Hello").await.unwrap_err();
    // Call 3: fails
    let _ = fabric.complete("Hello").await.unwrap_err();

    assert_eq!(calls_p1.load(std::sync::atomic::Ordering::SeqCst), 3);

    // Verify circuit is now OPEN
    {
        let circuits = fabric.circuits.lock().unwrap();
        let cb = circuits.get("ollama").unwrap();
        assert_eq!(cb.state, CircuitState::Open);
    }

    // Reset required capabilities to Json. Ollama supports Json, but its circuit is OPEN,
    // so it must be skipped, routing directly to openrouter (which also supports Json).
    fabric.required_capabilities = vec![ModelCapability::Json];
    let res = fabric.complete("Hello").await.unwrap();
    assert_eq!(res, "Vision Answer");
    assert_eq!(calls_p1.load(std::sync::atomic::Ordering::SeqCst), 3); // still 3, skipped!
    assert_eq!(calls_p2.load(std::sync::atomic::Ordering::SeqCst), 2); // openrouter called again
}

#[tokio::test]
async fn test_inference_fabric_persistence_and_telemetry() {
    use friday_intelligence::{InferenceFabric, ModelClient, ModelInfo, RoutingMode};

    // Clean up before test
    let _ = std::fs::remove_dir_all(".fabric");

    let mut fabric = InferenceFabric::new();
    fabric.default_mode = RoutingMode::Free;

    let calls = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    fabric.register_provider(Box::new(TestProvider {
        name: "openrouter".to_string(),
        succeed: true,
        response: "Metric Response".to_string(),
        failure_text: None,
        calls,
    }));
    fabric.register_model(ModelInfo {
        id: "model-1".to_string(),
        family: "llama".to_string(),
        provider: "openrouter".to_string(),
        latency_ms: 100,
        reliability: 1.0,
        capabilities: vec![],
        max_context: 2048,
        tier: None,
    });

    let res = fabric.complete("test metric log").await.unwrap();
    assert_eq!(res, "Metric Response");

    // Assert files are created
    assert!(std::path::Path::new(".fabric/provider_health.json").exists());
    assert!(std::path::Path::new(".fabric/metrics.jsonl").exists());

    // Assert metrics.jsonl contains valid JSON
    let metrics_content = std::fs::read_to_string(".fabric/metrics.jsonl").unwrap();
    assert!(metrics_content.contains("request_id"));
    assert!(metrics_content.contains("model-1"));

    // Clean up after test
    let _ = std::fs::remove_dir_all(".fabric");
}

// ======================================================================
// R5-2-3-2 — failure/fallback qualification boundary tests (F1/F2/F3).
// Frozen spec: `docs/design/R5-2-3-2-FALLBACK-QUALIFICATION.md`. No architectural
// change — these pin the fallback guarantees that the R5-2-3-1 wiring
// (Copilot account-auth + Groq free-remote floor) must keep intact.
// ======================================================================

/// F2 — Copilot appears only under `Quality`/`Hybrid`, never `Free`/`Local`.
/// This matches `select_candidates`' hierarchy slots: `copilot` is a
/// subscription-class member gated to Quality/Hybrid, not a free/local seam.
#[tokio::test]
async fn fallback_routes_copilot_only_under_quality_or_hybrid() {
    use friday_intelligence::{InferenceFabric, ModelInfo, RoutingMode};

    let mut fabric = InferenceFabric::new();
    fabric.register_provider(Box::new(TestProvider::new("copilot", true, "ok")));
    fabric.register_model(ModelInfo {
        id: "auto".to_string(),
        family: "copilot-auto".to_string(),
        provider: "copilot".to_string(),
        latency_ms: 800,
        reliability: 1.0,
        capabilities: vec![],
        max_context: 200_000,
        tier: None,
    });

    let names = |mode: RoutingMode| {
        fabric
            .select_candidates(mode)
            .iter()
            .map(|(_, p)| p.name().to_string())
            .collect::<Vec<_>>()
    };

    assert!(
        names(RoutingMode::Quality).contains(&"copilot".to_string()),
        "copilot is a Quality (subscription) member"
    );
    assert!(
        names(RoutingMode::Hybrid).contains(&"copilot".to_string()),
        "copilot is reachable under Hybrid"
    );
    assert!(
        !names(RoutingMode::Free).contains(&"copilot".to_string()),
        "copilot must not appear under Free"
    );
    assert!(
        !names(RoutingMode::Local).contains(&"copilot".to_string()),
        "copilot must not appear under Local"
    );
}

/// F2 — Copilot is opt-in only: without `COPILOT_CLI_ENABLED` the real
/// registration path (`default_from_env`) must not put a copilot candidate in
/// the fabric; with it set, it must. Pins the account-auth gate so a bare
/// `copilot`/`gh` binary on PATH never silently adds a cloud candidate.
#[tokio::test]
async fn fallback_skips_copilot_when_optout() {
    use friday_intelligence::{InferenceFabric, RoutingMode};

    std::env::remove_var("COPILOT_CLI_ENABLED");
    let off = InferenceFabric::default_from_env();
    let q_off = off
        .select_candidates(RoutingMode::Quality)
        .iter()
        .map(|(_, p)| p.name().to_string())
        .collect::<Vec<_>>();
    assert!(
        !q_off.contains(&"copilot".to_string()),
        "COPILOT_CLI_ENABLED unset -> copilot must not be registered"
    );

    std::env::set_var("COPILOT_CLI_ENABLED", "true");
    let on = InferenceFabric::default_from_env();
    std::env::remove_var("COPILOT_CLI_ENABLED");
    let q_on = on
        .select_candidates(RoutingMode::Quality)
        .iter()
        .map(|(_, p)| p.name().to_string())
        .collect::<Vec<_>>();
    assert!(
        q_on.contains(&"copilot".to_string()),
        "COPILOT_CLI_ENABLED=true -> copilot is registered"
    );
}

/// R5-2-3-0 §4 — Groq is the free-remote floor, so it is selectable under
/// `RoutingMode::Free` (its `is_free_provider` reclassification must hold).
#[tokio::test]
async fn fallback_groq_is_free_provider() {
    use friday_intelligence::{InferenceFabric, ModelInfo, RoutingMode};

    let mut fabric = InferenceFabric::new();
    fabric.register_provider(Box::new(TestProvider::new("groq", true, "ok")));
    fabric.register_model(ModelInfo {
        id: "llama-3.3-70b-versatile".to_string(),
        family: "llama-3.3-70b".to_string(),
        provider: "groq".to_string(),
        latency_ms: 200,
        reliability: 1.0,
        capabilities: vec![],
        max_context: 128_000,
        tier: None,
    });

    let free = fabric
        .select_candidates(RoutingMode::Free)
        .iter()
        .map(|(_, p)| p.name().to_string())
        .collect::<Vec<_>>();
    assert!(
        free.contains(&"groq".to_string()),
        "groq must be a free-remote floor member under RoutingMode::Free"
    );
}

/// F3 — `classify_failure_reason` buckets a credential-bearing 401 as `auth`
/// and the reason string carries no key fragment.
#[test]
fn fallback_classify_failure_reason_does_not_contain_key() {
    use friday_intelligence::classify_failure_reason;

    let err = "HTTP 401: invalid Bearer ghp_secret123 token";
    assert_eq!(classify_failure_reason(err), "auth");
}

/// F3 — a provider failure whose error text contains a raw credential must
/// surface as a sanitized `CandidateFailure` in the routing-decision audit
/// trail: the `ghp_...` fragment must never appear in `failures[i].error`.
#[tokio::test]
async fn fallback_does_not_leak_credential_in_decision_audit() {
    use friday_intelligence::{InferenceFabric, ModelClient, ModelInfo, RoutingMode};

    let mut fabric = InferenceFabric::new();
    fabric.default_mode = RoutingMode::Free;
    fabric.register_provider(Box::new(TestProvider::failing_with(
        "openrouter",
        "HTTP 401: invalid Bearer ghp_secret123 token",
    )));
    fabric.register_model(ModelInfo {
        id: "free-model".to_string(),
        family: "auto".to_string(),
        provider: "openrouter".to_string(),
        latency_ms: 100,
        reliability: 1.0,
        capabilities: vec![],
        max_context: 4096,
        tier: None,
    });

    assert!(fabric.complete("trigger a failure").await.is_err());

    let d = fabric.last_decision().expect("routing decision recorded");
    assert!(
        d.failures
            .iter()
            .all(|f| !f.error.contains("ghp_secret123")),
        "no raw key fragment may surface in the audit trail: {:?}",
        d.failures
    );
}
