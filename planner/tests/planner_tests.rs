//! M6.2 — Planner pipeline tests: router selection, LLM planning + validation,
//! and deterministic validation rules. No network / API keys required.

use std::sync::Arc;

use friday_core::trace::{
    CognitiveTraceContext, GoalId, InMemoryTraceCollector, NoopTraceCollector, SessionId,
    TraceCollector, TraceId,
};
use friday_planner::execution::capability_registry::CapabilityRegistry;
use friday_planner::execution::lifecycle::ExecutionBudget;
use friday_planner::intent::parse;
use friday_planner::planning::planners::backend::{MockBackend, PlanError};
use friday_planner::planning::planners::context::{RiskPolicy, WorkspaceContext};
use friday_planner::planning::planners::llm::LlmPlanner;
use friday_planner::planning::planners::router::PlannerRouter;
use friday_planner::planning::planners::rule::RulePlanner;
use friday_planner::planning::planners::validation::validate_task_graph;
use friday_planner::planning::planners::Planner;
use friday_planner::planning::TaskGraph;
use friday_planner::register_engines_with_llm;

fn registry_with_natives() -> CapabilityRegistry {
    let reg = CapabilityRegistry::new();
    friday_planner::execution::native_caps::register_native_builtins(&reg);
    reg
}

#[test]
fn router_picks_rule_for_trivial_intent() {
    let router = PlannerRouter::new(
        Arc::new(RulePlanner::new()),
        Some(Arc::new(LlmPlanner::new(
            Box::new(MockBackend::new("{}")),
            Arc::new(registry_with_natives()),
            ExecutionBudget::default(),
            RiskPolicy::default(),
        ))),
    );
    let intent = parse("Open Chrome and search Tesla");
    assert_eq!(
        router.kind_for(&intent),
        friday_planner::planning::planners::PlannerKind::Rule
    );

    let complex = parse("Refactor this Rust workspace to use async traits");
    assert_eq!(
        router.kind_for(&complex),
        friday_planner::planning::planners::PlannerKind::Llm
    );

    let unknown = parse("Do something clever with my files");
    assert_eq!(
        router.kind_for(&unknown),
        friday_planner::planning::planners::PlannerKind::Llm
    );
}

#[test]
fn router_falls_back_to_rule_when_no_llm() {
    let router = PlannerRouter::rule_only(Arc::new(RulePlanner::new()));
    let complex = parse("Deploy to Docker");
    assert_eq!(
        router.kind_for(&complex),
        friday_planner::planning::planners::PlannerKind::Rule
    );
    assert!(!router.has_llm());
}

#[tokio::test]
async fn llm_planner_produces_validated_graph() {
    let plan_json = r#"{
        "tasks": [
            {"name": "Read Cargo.toml", "capability": "filesystem.read_file",
             "params": {"path": "Cargo.toml"}, "depends_on_indices": []},
            {"name": "Write summary", "capability": "filesystem.write_file",
             "params": {"path": "summary.txt"}, "depends_on_indices": [0], "produces": "summary"}
        ],
        "artifacts": ["summary"],
        "estimated_cost": 0.01
    }"#;
    let reg = Arc::new(registry_with_natives());
    let planner = LlmPlanner::new(
        Box::new(MockBackend::new(plan_json)),
        reg.clone(),
        ExecutionBudget::default(),
        RiskPolicy::default(),
    );
    let intent = parse("Summarize my Cargo.toml");
    let ctx = friday_planner::planning::planners::context::PlanContext {
        intent_raw: intent.raw.clone(),
        capabilities: vec![],
        budget: ExecutionBudget::default(),
        workspace: WorkspaceContext::default(),
        risk_policy: RiskPolicy::default(),
        experience: None,
        skills: None,
        investigation: None,
    };
    let tracer = NoopTraceCollector;
    let trace_ctx = CognitiveTraceContext::for_goal(
        TraceId::default(),
        SessionId::default(),
        GoalId::default(),
    );
    let result = planner
        .create_plan(&intent, &ctx, &tracer, &trace_ctx)
        .await
        .expect("plan should validate");
    assert_eq!(result.graph.tasks.len(), 2);
    assert_eq!(result.graph.tasks[1].depends_on.len(), 1);
    assert_eq!(
        result.graph.tasks[1].depends_on[0],
        result.graph.tasks[0].id
    );
    assert_eq!(result.graph.tasks[1].produces.as_deref(), Some("summary"));
    validate_task_graph(
        &result.graph,
        &reg,
        &ExecutionBudget::default(),
        &RiskPolicy::default(),
    )
    .expect("graph passes validation");
}

#[tokio::test]
async fn llm_planner_rejects_unknown_capability() {
    let plan_json = r#"{
        "tasks": [
            {"name": "Magic", "capability": "unobtanium.do_thing", "params": {}, "depends_on_indices": []}
        ]
    }"#;
    let planner = LlmPlanner::new(
        Box::new(MockBackend::new(plan_json)),
        Arc::new(registry_with_natives()),
        ExecutionBudget::default(),
        RiskPolicy::default(),
    );
    let intent = parse("Do magic");
    let ctx = friday_planner::planning::planners::context::PlanContext {
        intent_raw: intent.raw.clone(),
        capabilities: vec![],
        budget: ExecutionBudget::default(),
        workspace: WorkspaceContext::default(),
        risk_policy: RiskPolicy::default(),
        experience: None,
        skills: None,
        investigation: None,
    };
    let tracer = NoopTraceCollector;
    let trace_ctx = CognitiveTraceContext::for_goal(
        TraceId::default(),
        SessionId::default(),
        GoalId::default(),
    );
    let err = planner
        .create_plan(&intent, &ctx, &tracer, &trace_ctx)
        .await
        .unwrap_err();
    match err {
        PlanError::Validation(issues) => assert!(
            issues.iter().any(|i| i.contains("unobtanium.do_thing")),
            "should flag unknown capability: {issues:?}"
        ),
        other => panic!("expected validation error, got {other}"),
    }
}

#[test]
fn validation_detects_cycles_and_budget() {
    let reg = registry_with_natives();
    let mut g = TaskGraph::new("cycle");
    let a = g.add_task("A", "filesystem.read_file", serde_json::json!({}), &[]);
    let b = g.add_task("B", "filesystem.write_file", serde_json::json!({}), &[a]);
    if let Some(t) = g.task_mut(a) {
        t.depends_on = vec![b];
    }
    let err = validate_task_graph(
        &g,
        &reg,
        &ExecutionBudget::default(),
        &RiskPolicy::default(),
    )
    .unwrap_err();
    match err {
        PlanError::Validation(issues) => assert!(
            issues.iter().any(|i| i.contains("cycle")),
            "should detect cycle: {issues:?}"
        ),
        other => panic!("expected validation error, got {other}"),
    }

    let mut g2 = TaskGraph::new("big");
    for i in 0..5 {
        g2.add_task(
            format!("t{i}"),
            "filesystem.read_file",
            serde_json::json!({}),
            &[],
        );
    }
    let budget = ExecutionBudget {
        max_steps: Some(2),
        ..ExecutionBudget::default()
    };
    let err2 = validate_task_graph(&g2, &reg, &budget, &RiskPolicy::default()).unwrap_err();
    match err2 {
        PlanError::Validation(issues) => assert!(
            issues.iter().any(|i| i.contains("budget")),
            "should flag budget: {issues:?}"
        ),
        other => panic!("expected validation error, got {other}"),
    }
}

// --- End-to-end: an LLM-backed planning engine drives a real goal ---

use async_trait::async_trait;
use friday_core::event::Event;
use friday_core::plugin::{Plugin, PluginMetadata};
use friday_core::Friday;
use friday_planner::events;

/// Stands in for the action runtime: answers `exec.action` with success.
struct MockExecutor {
    meta: PluginMetadata,
}

#[async_trait]
impl Plugin for MockExecutor {
    fn metadata(&self) -> &PluginMetadata {
        &self.meta
    }
    async fn handle_event(&self, event: &Event) -> friday_core::Result<Vec<Event>> {
        let task_id = event
            .payload
            .get("task_id")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let plan_id = event
            .payload
            .get("plan_id")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let capability = event
            .payload
            .get("capability")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        Ok(vec![events::event(
            events::EXEC_RESULT,
            "mock",
            serde_json::json!({ "task_id": task_id, "plan_id": plan_id, "success": true, "output": {}, "capability": capability }),
            event.trace_id,
        )])
    }
}

#[tokio::test]
async fn llm_backed_pipeline_produces_and_runs_plan() {
    let plan_json = r#"{
        "tasks": [
            {"name": "Read README", "capability": "filesystem.read_file",
             "params": {"path": "README.md"}, "depends_on_indices": []},
            {"name": "Write notes", "capability": "filesystem.write_file",
             "params": {"path": "notes.md"}, "depends_on_indices": [0]}
        ]
    }"#;
    friday_core::init_telemetry();
    let mut friday = Friday::new();
    register_engines_with_llm(&mut friday, Some(Box::new(MockBackend::new(plan_json))))
        .await
        .expect("register");

    let exec = MockExecutor {
        meta: PluginMetadata::builder("mock", "Mock Executor")
            .subscribes(&[events::EXEC_ACTION])
            .emits(&[events::EXEC_RESULT])
            .build(),
    };
    friday.register_plugin(Box::new(exec)).await.unwrap();

    let bus = friday.bus().clone();
    let mut rx = bus.subscribe();
    bus.publish(Event::new(
        events::GOAL_RAW,
        "user",
        serde_json::json!({ "text": "Refactor the workspace and summarize it" }),
    ))
    .await
    .unwrap();

    let handle = tokio::spawn(async move {
        friday
            .run_with_timeout(std::time::Duration::from_secs(3))
            .await
    });
    let mut saw_plan = false;
    let mut completed = 0usize;
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(3);
    while tokio::time::Instant::now() < deadline {
        if let Ok(Ok(ev)) =
            tokio::time::timeout(std::time::Duration::from_millis(100), rx.recv()).await
        {
            if ev.kind == events::PLAN_CREATED {
                saw_plan = true;
                let tasks = ev
                    .payload
                    .get("tasks")
                    .and_then(|t| t.as_array())
                    .map(|a| a.len())
                    .unwrap_or(0);
                assert_eq!(tasks, 2, "LLM plan should have 2 tasks");
            }
            if ev.kind == events::TASK_COMPLETED {
                completed += 1;
            }
        }
    }
    handle.await.unwrap().unwrap();
    assert!(saw_plan, "an LLM-produced plan should be published");
    assert_eq!(
        completed, 2,
        "both tasks should run to completion via supervisor"
    );
}

#[tokio::test]
async fn llm_planner_handles_experience_context() {
    use friday_planner::experience::{
        context::{ExperienceContext, ExperienceEvidence, PlanningGoal, SuccessfulWorkflow},
        ExperienceMatch, GoalOutcome, MatchReason, ReflectionCategory, ReflectionRecord,
        TrustComponents,
    };
    use uuid::Uuid;

    let plan_json = r#"{
        "tasks": [
            {"name": "Read file", "capability": "filesystem.read_file",
             "params": {"path": "test.txt"}, "depends_on_indices": []}
        ]
    }"#;

    let goal_id = Uuid::new_v4();
    let experience = ExperienceContext {
        goal: PlanningGoal {
            id: goal_id,
            title: "Read test file".into(),
            description: "Read the test file".into(),
        },
        capabilities: vec!["filesystem.read_file".into()],
        constraints: vec![],
        evidence: ExperienceEvidence {
            similar_experiences: vec![ExperienceMatch {
                similarity_score: 0.85,
                trust_score: 0.7,
                trust_components: TrustComponents {
                    similarity: 0.85,
                    recency: 0.9,
                    outcome_quality: 1.0,
                    repetition: 0.5,
                },
                match_reason: MatchReason::SimilarGoal,
                goal_id: Uuid::new_v4(),
                workflow_id: None,
                outcome: GoalOutcome::Success,
                reflection: vec![ReflectionRecord {
                    id: Uuid::new_v4(),
                    timeline_entry_id: Uuid::new_v4(),
                    insight: "File reads are safe and fast".into(),
                    category: ReflectionCategory::Success,
                    timestamp: chrono::Utc::now(),
                }],
                artifacts: vec![],
                commit_hash: None,
                goal_title: "Read config file".into(),
            }],
            previous_outcome: None,
            successful_workflows: vec![SuccessfulWorkflow {
                title: "Read then summarize".into(),
                capabilities: vec!["filesystem.read_file".into()],
                outcome: GoalOutcome::Success,
            }],
            workflow_suggestions: vec![],
            replay_candidates: vec![],
            reflections: vec![],
            provenance: vec![],
        },
    };

    let planner = LlmPlanner::new(
        Box::new(MockBackend::new(plan_json)),
        Arc::new(registry_with_natives()),
        ExecutionBudget::default(),
        RiskPolicy::default(),
    );

    let intent = parse("Read test file");
    let ctx = friday_planner::planning::planners::context::PlanContext {
        intent_raw: intent.raw.clone(),
        capabilities: vec![],
        budget: ExecutionBudget::default(),
        workspace: WorkspaceContext::default(),
        risk_policy: RiskPolicy::default(),
        experience: Some(experience),
        skills: None,
        investigation: None,
    };

    let tracer = NoopTraceCollector;
    let trace_ctx = CognitiveTraceContext::for_goal(
        TraceId::default(),
        SessionId::default(),
        GoalId::default(),
    );
    let result = planner
        .create_plan(&intent, &ctx, &tracer, &trace_ctx)
        .await
        .expect("plan should validate with experience context");
    assert_eq!(result.graph.tasks.len(), 1);

    let ctx_no_exp = friday_planner::planning::planners::context::PlanContext {
        intent_raw: intent.raw.clone(),
        capabilities: vec![],
        budget: ExecutionBudget::default(),
        workspace: WorkspaceContext::default(),
        risk_policy: RiskPolicy::default(),
        experience: None,
        skills: None,
        investigation: None,
    };
    let result2 = planner
        .create_plan(&intent, &ctx_no_exp, &tracer, &trace_ctx)
        .await
        .expect("plan should validate without experience");
    assert_eq!(result2.graph.tasks.len(), 1);
}

// ── 4F: Real runtime pipeline test ────────────────────────────────────────────
//
// Proves the full cognitive pipeline:
//   GOAL_RAW → IntentEngine → PlanningEngine → PlannerRouter → LlmPlanner
//   → ReasoningBackend → fake provider → CompletionResult + ModelUsage
//   → CognitiveTrace collector → reconstruct_trace_tree()
//
// Uses a deterministic fake backend with predetermined ModelUsage values.
// Asserts exact telemetry values, trace identity constraints, and hierarchy.

use friday_core::trace::ModelUsage as MU;
use friday_core::trace::ResultType as RT;

struct FakeTelemetryBackend {
    response: String,
}

impl FakeTelemetryBackend {
    fn new(response: impl Into<String>) -> Self {
        Self {
            response: response.into(),
        }
    }
}

#[async_trait]
impl friday_planner::planning::planners::backend::ReasoningBackend for FakeTelemetryBackend {
    async fn complete(&self, _prompt: &str) -> Result<String, String> {
        Ok(self.response.clone())
    }

    async fn complete_with_usage(
        &self,
        prompt: &str,
    ) -> Result<friday_planner::planning::planners::backend::CompletionResult, String> {
        let content = self.complete(prompt).await?;
        Ok(
            friday_planner::planning::planners::backend::CompletionResult {
                content,
                usage: Some(MU {
                    model: "test-model".to_string(),
                    provider: "test-provider".to_string(),
                    tokens_in: Some(100),
                    tokens_out: Some(42),
                    cached_tokens: Some(10),
                    reasoning_tokens: Some(8),
                    cost_usd: Some(0.001),
                    finish_reason: Some("stop".to_string()),
                }),
            },
        )
    }
}

struct FailingBackend;

#[async_trait]
impl friday_planner::planning::planners::backend::ReasoningBackend for FailingBackend {
    async fn complete(&self, _prompt: &str) -> Result<String, String> {
        Err("simulated provider timeout".into())
    }
}

const VALID_PLAN_JSON: &str = r#"{
    "tasks": [
        {
            "name": "read readme",
            "capability": "filesystem.read_file",
            "params": {"path": "README.md"},
            "depends_on_indices": []
        }
    ]
}"#;

#[tokio::test]
async fn test_4f_real_runtime_pipeline_with_model_telemetry() {
    let collector = Arc::new(InMemoryTraceCollector::new());
    let registry = Arc::new(CapabilityRegistry::new());
    friday_planner::execution::native_caps::register_native_builtins(&registry);

    let backend = FakeTelemetryBackend::new(VALID_PLAN_JSON);
    let engine = friday_planner::engines::planning_engine_with_tracer(
        registry.clone(),
        Some(Box::new(backend)),
        ExecutionBudget::default(),
        RiskPolicy::default(),
        WorkspaceContext::default(),
        Some(collector.clone() as Arc<dyn TraceCollector>),
    );

    let trace_uuid = uuid::Uuid::new_v4();
    let goal_event = friday_planner::events::event(
        friday_planner::events::GOAL_RAW,
        "test",
        serde_json::json!({"text": "Refactor the auth module to use async traits"}),
        trace_uuid,
    );

    let intent_engine = friday_planner::IntentEngine::new();
    let intent_events = intent_engine
        .handle_event(&goal_event)
        .await
        .expect("intent should resolve");
    assert_eq!(
        intent_events.len(),
        1,
        "must emit exactly one INTENT_CREATED"
    );
    let intent_event = &intent_events[0];
    assert_eq!(intent_event.kind, friday_planner::events::INTENT_CREATED);
    assert_eq!(
        intent_event.trace_id, trace_uuid,
        "trace_id must be preserved"
    );

    let plan_events = engine
        .handle_event(intent_event)
        .await
        .expect("planning should succeed");
    assert!(
        !plan_events.is_empty(),
        "planning must emit at least one event"
    );

    let plan_event = plan_events
        .iter()
        .find(|e| e.kind == friday_planner::events::PLAN_CREATED)
        .expect("must emit PLAN_CREATED");
    let tasks = plan_event
        .payload
        .get("tasks")
        .and_then(|t| t.as_array())
        .expect("plan must carry tasks");
    assert_eq!(tasks.len(), 1, "fake backend produces a 1-task plan");

    let guard = collector.traces();
    let traces: Vec<_> = guard.clone();
    assert!(
        traces.len() >= 3,
        "must capture planner + model_call + plan_result spans, got {}",
        traces.len()
    );

    let trace_id = TraceId(trace_uuid);
    for t in &traces {
        assert_eq!(
            t.trace_id, trace_id,
            "every span must share the episode's trace_id"
        );
    }

    friday_core::trace::validate_trace_hierarchy(&traces)
        .expect("trace hierarchy must be structurally valid");
    let trees = friday_core::trace::reconstruct_trace_tree(&traces);
    assert_eq!(trees.len(), 1, "must have exactly one root");

    let root = &trees[0];
    assert_eq!(root.trace.organ, "planner");
    assert_eq!(root.trace.operation, "create_plan");
    assert!(
        root.trace.parent_span_id.is_none(),
        "root must have no parent"
    );

    let model_subtree = root
        .children
        .iter()
        .find(|c| c.trace.operation == "llm_call")
        .expect("model child span (llm_call) must exist");

    let model = &model_subtree.trace;
    let usage = model
        .model_usage
        .as_ref()
        .expect("model span must carry ModelUsage");
    assert_eq!(usage.model, "test-model", "model identity must match");
    assert_eq!(
        usage.provider, "test-provider",
        "provider identity must match"
    );
    assert_eq!(usage.tokens_in, Some(100), "tokens_in must match");
    assert_eq!(usage.tokens_out, Some(42), "tokens_out must match");
    assert_eq!(usage.cost_usd, Some(0.001), "cost_usd must match");
    assert_eq!(
        usage.finish_reason.as_deref(),
        Some("stop"),
        "finish_reason must match"
    );

    assert_eq!(
        model.parent_span_id,
        Some(root.trace.span_id),
        "model span must be child of planner root"
    );
    // Fake backend is near-instant; timing assertion is on the real backend path.
    // The fact that with_timing was called is proven by the span existing at all.

    let result_subtree = root
        .children
        .iter()
        .find(|c| c.trace.operation == "plan_result")
        .expect("plan_result child span must exist");
    assert_eq!(
        result_subtree.trace.parent_span_id,
        Some(root.trace.span_id),
        "result span must be child of planner root"
    );

    let span_ids: Vec<_> = traces.iter().map(|t| t.span_id).collect();
    for t in &traces {
        if let Some(parent) = t.parent_span_id {
            assert!(
                span_ids.contains(&parent),
                "span {} has parent {} that does not exist in the trace set",
                t.span_id,
                parent
            );
        }
    }

    println!("4F pipeline test passed — trace_id = {}", trace_id);
    for t in &traces {
        let parent = t
            .parent_span_id
            .map(|p| p.to_string())
            .unwrap_or_else(|| "none".to_string());
        println!(
            "  span {} | {} / {} | parent = {}",
            t.span_id, t.organ, t.operation, parent
        );
        if let Some(ref u) = t.model_usage {
            println!(
                "    model={}, provider={}, in={}, out={}, cost={}",
                u.model,
                u.provider,
                u.tokens_in.unwrap_or(0),
                u.tokens_out.unwrap_or(0),
                u.cost_usd.unwrap_or(0.0)
            );
        }
    }
}

#[tokio::test]
async fn test_4f_failure_path_emits_failure_trace() {
    let collector = Arc::new(InMemoryTraceCollector::new());
    let registry = Arc::new(CapabilityRegistry::new());
    friday_planner::execution::native_caps::register_native_builtins(&registry);

    let backend = FailingBackend;
    let engine = friday_planner::engines::planning_engine_with_tracer(
        registry.clone(),
        Some(Box::new(backend)),
        ExecutionBudget::default(),
        RiskPolicy::default(),
        WorkspaceContext::default(),
        Some(collector.clone() as Arc<dyn TraceCollector>),
    );

    let trace_uuid = uuid::Uuid::new_v4();
    let intent_event = friday_planner::events::event(
        friday_planner::events::INTENT_CREATED,
        "test",
        serde_json::json!({
            "intent": "unknown",
            "raw": "Refactor the entire authentication module to use async trait objects",
            "entities": {}
        }),
        trace_uuid,
    );

    let plan_events = engine
        .handle_event(&intent_event)
        .await
        .expect("planning should succeed even on backend failure (falls back to rule planner)");

    let plan_event = plan_events
        .iter()
        .find(|e| e.kind == friday_planner::events::PLAN_CREATED)
        .expect("must emit PLAN_CREATED after fallback");

    let tasks = plan_event
        .payload
        .get("tasks")
        .and_then(|t| t.as_array())
        .expect("plan must carry tasks");
    assert!(
        !tasks.is_empty(),
        "rule fallback must produce at least one task"
    );

    let guard = collector.traces();
    let traces: Vec<_> = guard.clone();
    assert!(!traces.is_empty(), "failure path must emit trace records");

    let trace_id = TraceId(trace_uuid);
    for t in &traces {
        assert_eq!(
            t.trace_id, trace_id,
            "trace_id must be preserved on failure"
        );
    }

    friday_core::trace::validate_trace_hierarchy(&traces)
        .expect("failure trace hierarchy must be structurally valid");

    let result_subtree = traces
        .iter()
        .find(|t| t.operation == "plan_result")
        .expect("plan_result span must exist");

    assert_eq!(
        result_subtree.result,
        RT::Failure,
        "plan_result must be Failure when backend fails"
    );
    assert!(
        result_subtree.error.is_some(),
        "failure span must carry error message"
    );
    let err_msg = result_subtree.error.as_ref().unwrap();
    assert!(
        err_msg.contains("simulated provider timeout"),
        "error message must contain the backend error: {}",
        err_msg
    );

    let trees = friday_core::trace::reconstruct_trace_tree(&traces);
    assert_eq!(trees.len(), 1, "must have exactly one root");
    let root = &trees[0];
    assert_eq!(root.trace.operation, "create_plan");

    // The LLM call span is NOT emitted when complete_with_usage() fails
    // because the error propagates via ? before the trace is recorded.
    // PlanningEngine::handle_event() catches the error and emits the failure
    // plan_result span instead.
    let has_llm_call = traces.iter().any(|t| t.operation == "llm_call");
    assert!(
        !has_llm_call,
        "llm_call span must NOT exist when backend fails (error propagates before trace)"
    );

    println!("4F failure path test passed — trace_id = {}", trace_id);
    for t in &traces {
        let parent = t
            .parent_span_id
            .map(|p| p.to_string())
            .unwrap_or_else(|| "none".to_string());
        println!(
            "  span {} | {} / {} | result={:?} | parent = {}",
            t.span_id, t.organ, t.operation, t.result, parent
        );
    }
}
