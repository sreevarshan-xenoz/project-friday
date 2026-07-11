use friday_context::AgentContext;
use friday_intelligence::{
    CognitiveMode, IntelligenceRouter, ModelProvider, PromptCompiler, ResponseInterpreter
};

#[test]
fn test_intelligence_router() {
    // System 1 chitchat/greeting matches
    assert_eq!(IntelligenceRouter::route("Hello there!"), CognitiveMode::System1);
    assert_eq!(IntelligenceRouter::route("what time is it"), CognitiveMode::System1);
    
    // System 1 direct command matches
    assert_eq!(IntelligenceRouter::route("open notepad"), CognitiveMode::System1);

    // System 2 complex plannable actions
    assert_eq!(IntelligenceRouter::route("compile the main Rust application and run tests, then commit changes to git"), CognitiveMode::System2);
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
    assert!(parse_err.contains("windows.open") || parse_err.contains("windows.close")); // suggestion check
}

struct TestProvider {
    name: String,
    succeed: bool,
    response: String,
    calls: std::sync::Arc<std::sync::atomic::AtomicUsize>,
}

#[async_trait::async_trait]
impl friday_intelligence::Provider for TestProvider {
    fn name(&self) -> &str {
        &self.name
    }
    async fn complete(&self, _model_id: &str, _prompt: &str, _require_json: bool) -> Result<String, String> {
        self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        if self.succeed {
            Ok(self.response.clone())
        } else {
            Err("Simulated failure".to_string())
        }
    }
    async fn check_health(&self) -> bool {
        self.succeed
    }
}

#[tokio::test]
async fn test_inference_fabric_failover() {
    use friday_intelligence::{InferenceFabric, RoutingMode, ModelInfo, ModelClient};

    let mut fabric = InferenceFabric::new();
    fabric.default_mode = RoutingMode::Hybrid;

    let calls_p1 = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let calls_p2 = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));

    // ollama: fails, latency=100ms
    fabric.register_provider(Box::new(TestProvider {
        name: "ollama".to_string(),
        succeed: false,
        response: "".to_string(),
        calls: calls_p1.clone(),
    }));
    fabric.register_model(ModelInfo {
        id: "local-llama".to_string(),
        family: "llama".to_string(),
        provider: "ollama".to_string(),
        latency_ms: 100,
        reliability: 1.0,
        supports_json: true,
        supports_tools: true,
        max_context: 2048,
    });

    // openrouter: succeeds, latency=500ms
    fabric.register_provider(Box::new(TestProvider {
        name: "openrouter".to_string(),
        succeed: true,
        response: "Target Answer".to_string(),
        calls: calls_p2.clone(),
    }));
    fabric.register_model(ModelInfo {
        id: "cloud-gemini".to_string(),
        family: "gemini".to_string(),
        provider: "openrouter".to_string(),
        latency_ms: 500,
        reliability: 1.0,
        supports_json: true,
        supports_tools: true,
        max_context: 8192,
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
