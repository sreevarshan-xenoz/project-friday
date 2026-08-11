use friday_goal_execution::{
    Action, Capability, Constraints, ExecutionContext, Expectation, Goal, GoalEngine,
    LearningEngine, MockTool, MockVerifier, PlanEvaluator, Planning, PlanningContext, Policy,
    Precondition, ToolRegistry, Verification, WorldModel, WorldState,
};
use friday_reflection::{Insight, InsightKind};

/// Full pipeline: Goal → Planning → Action → Tool → Verification → Outcome → Reflection → Learning → Policy.
#[tokio::test]
async fn full_pipeline_with_learning() {
    let mut reg = ToolRegistry::new();
    reg.register(Box::new(MockTool::success("cargo", "Build complete")));
    let planning = Planning::new();
    let mut engine = GoalEngine::new().with_registry(reg);
    let mut learning = LearningEngine::new();

    // 1. Goal
    let goal = Goal::new("cargo build");

    // 2. Planning → Actions (via PlanningContext)
    let ctx = PlanningContext::new(goal.clone());
    let actions = planning.plan(&ctx);
    assert_eq!(actions.len(), 1);
    assert_eq!(actions[0].capability, Capability::ExecuteCommand);

    // 3. Goal Engine → Outcome (with ExecutionContext)
    let exp = Expectation::success("build will succeed");
    let exec_ctx = ExecutionContext::for_goal(&goal.id);
    let report = engine.execute(&goal, &actions[0], &exp, &exec_ctx).await;
    assert!(report.success());
    assert!(report.verification.verified);

    // 4. Learning ← Insights → Policies
    let policies = learning.learn(&report.insights);
    // Reinforcement produces no new policies
    assert!(policies.is_empty());

    // 5. Planning consumes policies via PlanningContext
    let ctx2 = PlanningContext::new(goal).with_policies(learning.policies().to_vec());
    let actions2 = planning.plan(&ctx2);
    assert_eq!(actions2.len(), 1);
}

/// Verification catches tool success that isn't real success.
#[tokio::test]
async fn verification_catches_false_success() {
    let mut reg = ToolRegistry::new();
    reg.register(Box::new(MockTool::success("download", "content")));
    let verification = Verification::new().register(Box::new(MockVerifier::fail("checksum")));
    let mut engine = GoalEngine::new()
        .with_registry(reg)
        .with_verification(verification);

    let goal = Goal::new("download file");
    let action = Action::single("download", Capability::ExecuteCommand, &goal.id);
    let exp = Expectation::success("will succeed");
    let exec_ctx = ExecutionContext::for_goal(&goal.id);
    let report = engine.execute(&goal, &action, &exp, &exec_ctx).await;

    // Tool reported success, but verification failed
    assert!(report.verification.tool_success);
    assert!(!report.verification.verified);
    // Therefore outcome is Error
    assert!(!report.success());
}

/// Verification passes when all checks pass.
#[tokio::test]
async fn verification_all_pass() {
    let mut reg = ToolRegistry::new();
    reg.register(Box::new(MockTool::success("cargo", "ok")));
    let verification = Verification::new()
        .register(Box::new(MockVerifier::pass("exit_code")))
        .register(Box::new(MockVerifier::pass("output_check")));
    let mut engine = GoalEngine::new()
        .with_registry(reg)
        .with_verification(verification);

    let goal = Goal::new("cargo build");
    let action = Action::single("cargo build", Capability::ExecuteCommand, &goal.id);
    let exp = Expectation::success("will succeed");
    let exec_ctx = ExecutionContext::for_goal(&goal.id);
    let report = engine.execute(&goal, &action, &exp, &exec_ctx).await;

    assert!(report.verification.verified);
    assert_eq!(report.verification.checks.len(), 2);
    assert!(report.success());
}

/// Learning from failure produces retry policy.
#[test]
fn learning_failure_produces_policy() {
    let mut learning = LearningEngine::new();
    let insight = Insight {
        id: "i1".into(),
        kind: InsightKind::Lesson,
        knowledge_id: "k1".into(),
        outcome_id: "o1".into(),
        statement: "cargo build failed".into(),
        confidence: 0.8,
        derived_at_ms: 1000,
    };

    let policies = learning.learn(&[insight]);
    assert_eq!(policies.len(), 1);
    assert_eq!(policies[0].kind, friday_goal_execution::PolicyKind::Retry);
}

/// Planning with sequencing policy produces multi-step actions via deliberative pipeline.
#[test]
fn planning_with_sequence_policy() {
    let policy = Policy::sequence(Capability::ExecuteCommand, Capability::ReadFile, 0.7, 10);
    let planning = Planning::new();
    let evaluator = PlanEvaluator::new();
    let goal = Goal::new("build project");
    let ctx = PlanningContext::new(goal).with_policies(vec![policy]);
    let candidates = planning.think(&ctx);
    assert!(
        candidates.len() >= 2,
        "expected at least direct + policy-driven candidates"
    );

    let world = friday_goal_execution::WorldState::default();
    let ranked = evaluator.evaluate(&candidates, &world);
    let actions = &ranked.plan.actions;
    assert!(
        actions.len() >= 2,
        "ranked plan should have at least 2 actions from policy-driven candidate"
    );
}

/// Goal with context is preserved through the full pipeline.
#[tokio::test]
async fn goal_context_preserved() {
    let mut reg = ToolRegistry::new();
    reg.register(Box::new(MockTool::success("echo", "ok")));
    let planning = Planning::new();
    let mut engine = GoalEngine::new().with_registry(reg);

    let goal = Goal::new("deploy").with_context("before release");
    let ctx = PlanningContext::new(goal.clone());
    let actions = planning.plan(&ctx);
    let exp = Expectation::success("will work");
    let exec_ctx = ExecutionContext::for_goal(&goal.id);
    let report = engine.execute(&goal, &actions[0], &exp, &exec_ctx).await;

    assert_eq!(report.outcome.context, "deploy");
    let goal_episode = &report.episodes[0];
    let detail = goal_episode
        .event
        .payload
        .get("description")
        .and_then(|v| v.as_str());
    assert_eq!(detail, Some("deploy"));
}

/// Expectation determines outcome kind.
#[tokio::test]
async fn expectation_determines_outcome() {
    let mut reg = ToolRegistry::new();
    reg.register(Box::new(MockTool::failure("cargo", "compile error")));
    let mut engine = GoalEngine::new().with_registry(reg);

    let goal = Goal::new("cargo build");
    let action = Action::single("cargo build", Capability::ExecuteCommand, &goal.id);

    // Expected failure → TaskCompleted
    let exp = Expectation::failure("will fail");
    let exec_ctx = ExecutionContext::for_goal(&goal.id);
    let report = engine.execute(&goal, &action, &exp, &exec_ctx).await;
    assert!(report.success());

    // Expected success → Error
    let exp = Expectation::success("will succeed");
    let exec_ctx = ExecutionContext::for_goal(&goal.id);
    let report = engine.execute(&goal, &action, &exp, &exec_ctx).await;
    assert!(!report.success());
}

/// Planning context carries constraints.
#[test]
fn planning_context_with_constraints() {
    let planning = Planning::new();
    let goal = Goal::new("cargo build");
    let ctx = PlanningContext::new(goal).with_constraints(Constraints {
        max_actions: Some(5),
        timeout_ms: Some(30_000),
        required_permissions: vec!["execute".into()],
        excluded_capabilities: vec![Capability::DeleteFile],
    });

    let actions = planning.plan(&ctx);
    assert_eq!(actions.len(), 1);
    // Constraints don't affect the current planner logic, but they're available for future use
}

/// ExecutionContext flows through to the tool.
#[tokio::test]
async fn execution_context_flows_to_tool() {
    use std::sync::{Arc, Mutex};

    let seen = Arc::new(Mutex::new(None::<ExecutionContext>));
    let seen_clone = seen.clone();

    struct ContextCapture {
        seen: Arc<Mutex<Option<ExecutionContext>>>,
    }

    #[async_trait::async_trait]
    impl friday_goal_execution::Tool for ContextCapture {
        fn name(&self) -> &str {
            "capture"
        }
        fn description(&self) -> &str {
            "Captures context"
        }
        fn capabilities(&self) -> Vec<Capability> {
            vec![Capability::ExecuteCommand]
        }
        async fn execute(
            &self,
            _action: &Action,
            context: &ExecutionContext,
        ) -> friday_goal_execution::ToolResult {
            *self.seen.lock().unwrap() = Some(context.clone());
            friday_goal_execution::ToolResult::success("ok")
        }
    }

    let mut reg = ToolRegistry::new();
    reg.register(Box::new(ContextCapture { seen: seen_clone }));
    let mut engine = GoalEngine::new().with_registry(reg);

    let goal = Goal::new("test context flow");
    let action = Action::single("test", Capability::ExecuteCommand, &goal.id);
    let exp = Expectation::success("will work");
    let exec_ctx = ExecutionContext::for_goal(&goal.id)
        .with_mission_id("m1")
        .with_working_directory("/tmp");

    engine.execute(&goal, &action, &exp, &exec_ctx).await;

    let captured = seen.lock().unwrap();
    let captured = captured.as_ref().unwrap();
    assert_eq!(captured.goal_id, goal.id);
    assert_eq!(captured.mission_id.as_deref(), Some("m1"));
    assert_eq!(captured.working_directory.as_deref(), Some("/tmp"));
}

/// WorldModel checks preconditions before execution and records transitions after.
#[test]
fn world_model_precondition_check_and_transition() {
    let model = WorldModel::new(WorldState::new().with_tool(Capability::ReadFile));

    // Precondition passes
    let failed = model.check_preconditions(&[Precondition::ToolAvailable {
        capability: Capability::ReadFile,
    }]);
    assert!(failed.is_empty());

    // Precondition fails
    let failed = model.check_preconditions(&[Precondition::ToolAvailable {
        capability: Capability::WriteFile,
    }]);
    assert_eq!(failed.len(), 1);
}

/// WorldModel records state transitions with diffs.
#[test]
fn world_model_records_transition_with_diff() {
    let mut model = WorldModel::new(WorldState::new());
    let new_state = WorldState::new()
        .with_file(
            "output.txt",
            friday_goal_execution::FileMeta {
                path: "output.txt".into(),
                exists: true,
                size_bytes: Some(100),
                is_directory: false,
                modified_at_ms: None,
            },
        )
        .with_variable("step", serde_json::json!(1));

    let transition = model.record_transition("a1", "g1", new_state, &[]);
    assert!(model.history().len() == 1);
    assert!(!transition.diff.added_files.is_empty());
    assert!(model.state().file_exists("output.txt"));
}

/// Cognitive trace is produced by GoalEngine and contains execution + verification + reflection.
#[tokio::test]
async fn goal_engine_produces_cognitive_trace() {
    let mut reg = ToolRegistry::new();
    reg.register(Box::new(MockTool::success("cargo", "Build OK")));
    let mut engine = GoalEngine::new().with_registry(reg);

    let goal = Goal::new("cargo build");
    let action = Action::single("cargo build", Capability::ExecuteCommand, &goal.id);
    let exp = Expectation::success("will succeed");
    let exec_ctx = ExecutionContext::for_goal(&goal.id);
    let report = engine.execute(&goal, &action, &exp, &exec_ctx).await;

    // Trace has the right goal
    assert_eq!(report.cognitive_trace.goal_id, goal.id);

    // Trace has at least 3 nodes: execution, verification, reflection
    assert!(report.cognitive_trace.nodes.len() >= 3);

    // Each node has valid timing
    for node in &report.cognitive_trace.nodes {
        assert!(node.finished_at_ms >= node.started_at_ms);
    }

    // Display works
    let display = report.cognitive_trace.display();
    assert!(display.contains("Goal:"));
}

// ── Causal World Model (synthesis Phase 4) ──────────────────────────

use friday_goal_execution::{
    CausalCondition, CausalEdge, CausalEffect, Entity, EntityRelation,
};

/// The causal acceptance: a causal edge answers "if action A runs, state S
/// changes to S'". The model stays pure; the projection carries S'.
#[test]
fn causal_edge_answers_if_action_runs_state_changes() {
    let mut model = WorldModel::new(WorldState::new().with_variable("head", serde_json::json!("old")));
    model.causal_mut().add_entity(
        Entity::new("repo:project-friday", "GitRepository").with_state("clean"),
    );
    model.causal_mut().add_relation(EntityRelation::new(
        "repo:project-friday",
        "hosts",
        "branch:dev",
    ));
    model.causal_mut().add_edge(CausalEdge::new(
        "git.commit->commit",
        Capability::Custom("git.commit".into()),
        CausalCondition::EntityExists {
            kind: "GitRepository".into(),
        },
        vec![
            CausalEffect::VariableSet {
                key: "head".into(),
                value: serde_json::json!("abc123"),
            },
            CausalEffect::EntityStateChanged {
                entity_id: "repo:project-friday".into(),
                to: "committed".into(),
            },
        ],
    ));

    // S → S': predict what git.commit does WITHOUT running it.
    let sim = model.simulate(&Capability::Custom("git.commit".into()));
    assert!(sim.applicable, "unmet: {:?}", sim.unmet);
    assert_eq!(sim.fired_edge.as_deref(), Some("git.commit->commit"));
    assert_eq!(sim.projected_world.variables["head"], serde_json::json!("abc123"));
    assert_eq!(
        sim.projected_entities
            .iter()
            .find(|e| e.id == "repo:project-friday")
            .unwrap()
            .state(),
        Some("committed")
    );

    // Counterfactual purity: the model did not move.
    assert_eq!(model.state().variables["head"], serde_json::json!("old"));
    assert_eq!(
        model.causal().entity("repo:project-friday").unwrap().state(),
        Some("clean")
    );
    assert_eq!(model.causal().relations.len(), 1);
}

/// Committing a projection adopts S' as the real state and records an audit
/// transition — the causal loop closes: predict → execute → commit.
#[test]
fn apply_simulation_commits_the_projected_state() {
    let mut model = WorldModel::new(WorldState::new());
    model.causal_mut().add_entity(Entity::new("repo:x", "GitRepository"));
    model.causal_mut().add_edge(CausalEdge::new(
        "init",
        Capability::Custom("git.init".into()),
        CausalCondition::Always,
        vec![CausalEffect::FileCreated {
            path: "/work/.git/config".into(),
        }],
    ));

    let sim = model.simulate(&Capability::Custom("git.init".into()));
    assert!(sim.applicable);
    assert!(sim.projected_world.file_exists("/work/.git/config"));

    // Simulated adoption: the state really becomes S'.
    assert!(!model.state().file_exists("/work/.git/config"));
    let transition = model
        .apply_simulation("act_1", "goal_1", &sim)
        .expect("applicable simulation commits");
    assert!(model.state().file_exists("/work/.git/config"));
    assert_eq!(transition.diff.added_files, vec!["/work/.git/config".to_string()]);
    assert_eq!(model.history().len(), 1);

    // An inapplicable simulation commits nothing — and its projection is the
    // unchanged state, not an empty world.
    let no_fire = model.simulate(&Capability::Custom("web.search".into()));
    assert!(!no_fire.applicable);
    assert!(
        no_fire.projected_world.file_exists("/work/.git/config"),
        "inapplicable projection carries the input state, not a blank world"
    );
    assert!(model.apply_simulation("act_2", "goal_1", &no_fire).is_none());
    assert_eq!(model.history().len(), 1, "nothing adopted");

    // A STALE projection (derived from an older world) is refused: committing
    // it would silently clobber the current state.
    let stale = model.simulate(&Capability::Custom("git.init".into()));
    assert!(stale.applicable);
    model.set_variable("moved", serde_json::json!(true)); // world advanced
    assert!(
        model.apply_simulation("act_3", "goal_1", &stale).is_none(),
        "stale projection must not be committed"
    );
    assert_eq!(model.history().len(), 1, "no transition from stale commit");
}

/// `find_missing_capability_prerequisites` reflects the causal layer's
/// grounded entity graph, not just the file-path heuristic.
#[test]
fn missing_prerequisites_reflect_the_entity_graph() {
    // A GitRepository entity in the causal layer satisfies git.commit even
    // though no `.git` file path is present in the world state.
    // (`KnowledgeGraph::new()` already seeds the ontology.)
    let mut model = WorldModel::new(WorldState::new());
    model
        .causal_mut()
        .add_entity(Entity::new("repo:x", "GitRepository"));
    assert!(
        model
            .find_missing_capability_prerequisites("git.commit")
            .is_empty(),
        "entity graph satisfies the prerequisite"
    );

    // Without the entity, the missing prerequisite is reported as before.
    let bare = WorldModel::new(WorldState::new());
    assert_eq!(
        bare.find_missing_capability_prerequisites("git.commit"),
        vec!["GitRepository".to_string()]
    );
}
