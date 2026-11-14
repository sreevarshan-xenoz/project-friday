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
    let engine = GoalEngine::new().with_registry(reg);
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
    let engine = GoalEngine::new()
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
    let engine = GoalEngine::new()
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
    let goal = Goal::new("cargo build");
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
    let engine = GoalEngine::new().with_registry(reg);

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
    let engine = GoalEngine::new().with_registry(reg);

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
    let engine = GoalEngine::new().with_registry(reg);

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
    let engine = GoalEngine::new().with_registry(reg);

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
    CausalCondition, CausalEdge, CausalEffect, DesiredState, Entity, EntityRelation, IntentLayer,
    PlanSource,
};

/// The causal acceptance: a causal edge answers "if action A runs, state S
/// changes to S'". The model stays pure; the projection carries S'.
#[test]
fn causal_edge_answers_if_action_runs_state_changes() {
    let mut model =
        WorldModel::new(WorldState::new().with_variable("head", serde_json::json!("old")));
    model
        .causal_mut()
        .add_entity(Entity::new("repo:project-friday", "GitRepository").with_state("clean"));
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
    assert_eq!(
        sim.projected_world.variables["head"],
        serde_json::json!("abc123")
    );
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
        model
            .causal()
            .entity("repo:project-friday")
            .unwrap()
            .state(),
        Some("clean")
    );
    assert_eq!(model.causal().relations.len(), 1);
}

/// Committing a projection adopts S' as the real state and records an audit
/// transition — the causal loop closes: predict → execute → commit.
#[test]
fn apply_simulation_commits_the_projected_state() {
    let mut model = WorldModel::new(WorldState::new());
    model
        .causal_mut()
        .add_entity(Entity::new("repo:x", "GitRepository"));
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
    assert_eq!(
        transition.diff.added_files,
        vec!["/work/.git/config".to_string()]
    );
    assert_eq!(model.history().len(), 1);

    // An inapplicable simulation commits nothing — and its projection is the
    // unchanged state, not an empty world.
    let no_fire = model.simulate(&Capability::Custom("web.search".into()));
    assert!(!no_fire.applicable);
    assert!(
        no_fire.projected_world.file_exists("/work/.git/config"),
        "inapplicable projection carries the input state, not a blank world"
    );
    assert!(model
        .apply_simulation("act_2", "goal_1", &no_fire)
        .is_none());
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

// ── LifeOS intent layer (#10): desired → gap → objective → plan ──

/// The LifeOS loop at the public API: a desired state with an open gap yields
/// an objective whose closing action (from the causal model's projection)
/// really closes the gap once committed.
#[test]
fn intent_gap_closes_after_committing_the_proposed_action() {
    let mut model = WorldModel::new(WorldState::new());
    model.causal_mut().add_edge(CausalEdge::new(
        "e:git.init",
        Capability::Custom("git.init".into()),
        CausalCondition::Always,
        vec![CausalEffect::EntityCreated(Entity::new(
            "repo:work",
            "GitRepository",
        ))],
    ));
    let desired = DesiredState::EntityExists {
        kind: "GitRepository".into(),
    };

    let layer = IntentLayer::new();
    let gap = layer.analyze(&model, &desired);
    assert!(
        !gap.is_closed(),
        "ideal not yet reached: {}",
        gap.describe()
    );

    let objective = layer
        .objective(&model, &desired)
        .expect("open gap → objective");
    assert_eq!(
        objective.closing_actions,
        vec![Capability::Custom("git.init".into())],
        "the modeled action whose projection closes the gap"
    );

    let sim = model.simulate(&objective.closing_actions[0]);
    assert!(model.apply_simulation("git.init", "goal_1", &sim).is_some());
    assert!(
        layer.analyze(&model, &desired).is_closed(),
        "committing the closing action reaches the ideal"
    );
}

/// Constitution acceptance (§3.2 Cognitive planner): a plan can be generated
/// *from* a desired-state gap — not only from a one-shot goal string. The
/// plan's provenance is `PlanSource::GapDriven` and its action is the closing
/// capability.
#[test]
fn plan_is_generated_from_a_desired_state_gap() {
    let mut model = WorldModel::new(WorldState::new());
    model.causal_mut().add_edge(CausalEdge::new(
        "e:git.init",
        Capability::Custom("git.init".into()),
        CausalCondition::Always,
        vec![CausalEffect::EntityCreated(Entity::new(
            "repo:work",
            "GitRepository",
        ))],
    ));
    let desired = DesiredState::EntityExists {
        kind: "GitRepository".into(),
    };

    let layer = IntentLayer::new();
    let planning = Planning::new();
    let ranked = layer
        .plan(&planning, &model, &desired)
        .expect("open gap yields a plan");

    assert_eq!(ranked.plan.source, PlanSource::GapDriven);
    assert_eq!(ranked.plan.action_count(), 1);
    assert_eq!(
        ranked.plan.actions[0].capability,
        Capability::Custom("git.init".into())
    );
}

/// Honesty rule: a satisfied ideal produces neither an objective nor a plan —
/// the intent layer never invents busywork.
#[test]
fn closed_desired_state_yields_no_objective_or_plan() {
    let mut model = WorldModel::new(WorldState::new());
    model
        .causal_mut()
        .add_entity(Entity::new("repo:work", "GitRepository"));
    let desired = DesiredState::EntityExists {
        kind: "GitRepository".into(),
    };

    let layer = IntentLayer::new();
    assert!(layer.analyze(&model, &desired).is_closed());
    assert!(layer.objective(&model, &desired).is_none());
    assert!(layer.plan(&Planning::new(), &model, &desired).is_none());
}

// ── Skill compounding (#11): verified trajectory → skill → reuse ──

use friday_goal_execution::{run_command_in, CodeTestVerifier};
use friday_skills::{
    OutcomeSummary, SkillCompiler, SkillRegistry, TrajectorySkill, TrajectoryStep,
};

/// The constitution acceptance for skill compounding (§3.2 Reflection / §3.3):
/// ONE verified successful trajectory becomes a reusable skill in the
/// registry, and a later goal uses it. The "verified" here is goal-execution's
/// REAL `CodeTestVerifier` over an actual command run — never a mock.
#[test]
fn verified_trajectory_compounds_into_a_reusable_skill() {
    // 1. Real verification: the command actually runs and the concrete
    //    verifier interprets the observed output (exit code + expect marker
    //    + failure-marker forbids).
    let verifier = CodeTestVerifier::for_test_run();
    let obs = run_command_in(std::env::temp_dir().as_path(), "echo test result: ok");
    let verified = verifier.verify_observation(&obs);
    assert!(
        verified.verified,
        "the trajectory must be VERIFIED: {:?}",
        verified.checks
    );

    // 2. The verified trajectory maps into the neutral compiler input,
    //    carrying its evidence ids (the audit trail).
    let trajectory = TrajectorySkill {
        goal: "Run the friday test suite and check the result".to_string(),
        outcome: OutcomeSummary::Verified {
            evidence_ids: vec!["ev_verify_1".to_string()],
        },
        steps: vec![
            TrajectoryStep {
                instruction: "run the friday test suite".to_string(),
                capability: "terminal.execute".to_string(),
            },
            TrajectoryStep {
                instruction: "check for the test result marker".to_string(),
                capability: "filesystem.read".to_string(),
            },
        ],
        confidence: 0.9,
        support: 2,
    };

    // 3. Compile → registered as a CANDIDATE (source Learned, disabled).
    let registry = SkillRegistry::new();
    let outcome = SkillCompiler::new().compile(&registry, &trajectory);
    assert!(outcome.compiled(), "refusals: {:?}", outcome.refusals);
    assert_eq!(
        outcome.skill.as_ref().unwrap().resolved_capabilities(),
        vec!["terminal.execute", "filesystem.read"]
    );
    assert!(
        !outcome.skill.as_ref().unwrap().enabled,
        "compounding produces a candidate, never an immediately-live skill"
    );

    // 3.5. Explicit admission: qualification + enable_learned flips the
    //      candidate into an executable skill (the reconciled §3.3 gate).
    let skill_id = outcome.skill.as_ref().unwrap().id().to_string();
    assert!(registry
        .select_for("run the friday tests and show me the result", 5)
        .is_none());
    registry.enable_learned(&skill_id).unwrap();

    // 4. A LATER goal uses the skill: selection ranks and chooses it.
    let selected = registry
        .select_for("run the friday tests and show me the result", 5)
        .expect("a later goal finds the learned skill");
    assert_eq!(
        selected.skill.id(),
        "run-the-friday-test-suite-and-check-the-result"
    );
}

/// The gate that makes compounding trustworthy: an UNVERIFIED trajectory is
/// refused — nothing is registered, so a later goal cannot reuse a failure.
#[test]
fn unverified_trajectory_is_refused_and_never_reused() {
    let registry = SkillRegistry::new();
    let trajectory = TrajectorySkill {
        goal: "Deploy to production".to_string(),
        outcome: OutcomeSummary::Unverified {
            reason: "test result: FAILED. 1 failed; 0 passed".to_string(),
        },
        steps: vec![TrajectoryStep {
            instruction: "deploy the service".to_string(),
            capability: "git.push".to_string(),
        }],
        confidence: 0.9,
        support: 1,
    };

    let outcome = SkillCompiler::new().compile(&registry, &trajectory);
    assert!(!outcome.compiled(), "a failure is never compounded");
    assert!(registry.all().is_empty());
    assert!(
        registry.select_for("deploy to production", 5).is_none(),
        "nothing unverified is reusable"
    );
}
