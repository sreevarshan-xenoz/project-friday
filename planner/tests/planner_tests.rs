//! M6.2 — Planner pipeline tests: router selection, LLM planning + validation,
//! and deterministic validation rules. No network / API keys required.

use std::sync::Arc;

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
    // browser.search is a known deterministic type -> rule.
    let intent = parse("Open Chrome and search Tesla");
    assert_eq!(
        router.kind_for(&intent),
        friday_planner::planning::planners::PlannerKind::Rule
    );

    // unknown / complex -> llm.
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
    // No LLM wired -> must use rule (never strand the goal).
    assert_eq!(
        router.kind_for(&complex),
        friday_planner::planning::planners::PlannerKind::Rule
    );
    assert!(!router.has_llm());
}

#[tokio::test]
async fn llm_planner_produces_validated_graph() {
    // A model that emits a 2-task DAG using real native capabilities.
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
    let graph = planner
        .create_plan(&intent, &ctx)
        .await
        .expect("plan should validate");
    assert_eq!(graph.tasks.len(), 2);
    // Dependency resolved: task 2 depends on task 1.
    assert_eq!(graph.tasks[1].depends_on.len(), 1);
    assert_eq!(graph.tasks[1].depends_on[0], graph.tasks[0].id);
    // produces published by task 2.
    assert_eq!(graph.tasks[1].produces.as_deref(), Some("summary"));
    // Re-validate independently to be sure.
    validate_task_graph(
        &graph,
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
    let err = planner.create_plan(&intent, &ctx).await.unwrap_err();
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
    // Build a cyclic graph manually.
    let mut g = TaskGraph::new("cycle");
    let a = g.add_task("A", "filesystem.read_file", serde_json::json!({}), &[]);
    let b = g.add_task("B", "filesystem.write_file", serde_json::json!({}), &[a]);
    if let Some(t) = g.task_mut(a) {
        t.depends_on = vec![b]; // cycle a -> b -> a
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

    // Budget: too many tasks.
    let mut g2 = TaskGraph::new("big");
    for i in 0..5 {
        g2.add_task(
            format!("t{i}"),
            "filesystem.read_file",
            serde_json::json!({}),
            &[],
        );
    }
    let mut budget = ExecutionBudget::default();
    budget.max_steps = Some(2);
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
    // The "model" emits a 2-task native DAG for a complex (LLM-routed) goal.
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

    // Planner should produce a valid plan even with experience context present.
    let graph = planner
        .create_plan(&intent, &ctx)
        .await
        .expect("plan should validate with experience context");
    assert_eq!(graph.tasks.len(), 1);

    // Also verify that experience=None still works (graceful degradation).
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
    let graph2 = planner
        .create_plan(&intent, &ctx_no_exp)
        .await
        .expect("plan should validate without experience");
    assert_eq!(graph2.tasks.len(), 1);
}
