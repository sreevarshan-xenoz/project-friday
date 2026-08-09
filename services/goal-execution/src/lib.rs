//! # FRIDAY Goal Execution
//!
//! The execution pipeline: **Goal → Planning → Action → Tool → Verification → Outcome → Reflection**.
//!
//! # Architecture
//!
//! ```text
//! Mission
//!   ├── Goal A ──→ Think ──→ Evaluate ──→ Choose ──→ Execute ──→ Verify ──→ Reflect
//!   ├── Goal B ──→ Think ──→ Evaluate ──→ Choose ──→ Execute ──→ Verify ──→ Reflect
//!   └── Goal C
//!
//! PlanningContext → Planning → Vec<PlanCandidate> → PlanEvaluator → RankedPlan
//!   (Goal, Knowledge, Policies, Constraints, WorldState)
//!
//! ExecutionContext → ToolResult
//!   (mission_id, goal_id, environment, variables, timeout, permissions)
//! ```
//!
//! # Responsibilities
//!
//! | Component        | Owns                                              |
//! | ---------------- | ------------------------------------------------- |
//! | Runtime          | *When* things happen (scheduling, phases)         |
//! | Mission          | *Coordination* (multi-goal sequencing)            |
//! | PlanningContext  | *Inputs* (Goal, Knowledge, Policies, WorldState)  |
//! | Planning         | *Hypotheses* (Context → candidate plans)          |
//! | PlanEvaluator    | *Judgment* (candidates → ranked plan)             |
//! | WorldState       | *Environment* (current state snapshot)            |
//! | WorldModel       | *Transitions* (state changes, preconditions)      |
//! | Simulator        | *Prediction* (simulate plans, no execution)       |
//! | RiskModel        | *Safety* (risk assessment per action)             |
//! | Confidence       | *Certainty* (validated belief strength)           |
//! | ConfidenceEngine | *Propagation* (reason under uncertainty)          |
//! | TraceRecorder    | *Audit assembly* (events → CognitiveTrace)        |
//! | WorkingMemory    | *Scratchpad* (what every engine reads first)      |
//! | Belief           | *Stance* (current position + uncertainty + freshness) |
//! | AttentionScore   | *Prioritization* (importance × urgency × novelty) |
//! | CognitiveEpisode | *Experience* (full goal lifecycle record)          |
//! | EpisodeStore     | *Episodic memory* (durable store with indexing)     |
//! | ExperiencePattern| *Intermediate knowledge* (evidence-backed patterns)  |
//! | SemanticMemory   | *Abstraction engine* (episodes → patterns → knowledge) |
//! | AttentionManager | *Scheduler* (scoring, ranking, focus selection)        |
//! | CognitiveBlackboard | *Cognitive state* (every engine reads/writes here) |
//! | WorkspaceSnapshot  | *Summary* (point-in-time cognitive snapshot)        |
//! | CognitiveTrace   | *Audit* (full decision trail)                     |
//! | ExecutionContext | *State* (mission, environment, variables)        |
//! | Goal Engine      | *How* to execute (Action → Tool → Outcome)        |
//! | Tool Registry    | *Which* tool handles a capability                 |
//! | Tool             | *Implementation* (terminal, filesystem)           |
//! | Verification     | *Interpretation* (tool output → verified result)  |
//! | Reflection       | *Evaluation* (Expected vs Observed)               |
//! | Learning         | *Adaptation* (Insights → Policies)                |
//!
//! # Invariants
//!
//! - Every Goal execution produces exactly **one Outcome**.
//! - Tool output ≠ success. Verification interprets execution results.
//! - Planning produces candidate plans, never executes.
//! - Learning produces Policies. Planning consumes Policies.
//! - Policies are immutable evidence-backed records.
//! - Mission coordinates Goals. Planning operates per Goal.
//! - Every Tool receives an ExecutionContext.
//! - PlanEvaluator is deterministic: same candidates → same ranking.
//! - Every action produces a StateTransition (immutable audit trail).
//! - Preconditions are checked before execution; postconditions applied after.
//! - CognitiveTrace records every decision node: input, output, world state, timing.
//! - Simulation never calls tools. Pure prediction.
//! - TraceRecorder decouples tracing from execution logic.
//! - Every action has a Risk level. Critical risk requires user approval.
//! - Confidence is a first-class value: Goal, Action, WorldState, Plan, Verification all carry it.
//! - ConfidenceEngine propagates confidence through rules (multiply, average, discount, cap, min).
//! - WorkingMemory is the cognitive scratchpad. All engines read it first.
//! - Beliefs are the system's stance: confidence + evidence + freshness. Knowledge derives from episodes; Beliefs capture current trust.
//! - AttentionScore drives selective focus. Nothing is treated equally.
//! - CognitiveEpisode captures the complete goal lifecycle: Goal → Planning → Simulation → Execution → Verification → Reflection.
//! - Semantic Memory derives patterns from episodes, never invents knowledge independently.
//! - ExperiencePattern accumulates evidence. Only when support/confidence thresholds are met does it become Knowledge or Policy.
//! - Semantic Memory never writes knowledge directly — it produces evidence-backed patterns that meet promotion thresholds.
//! - AttentionManager is the scheduler for cognition. Every object competes for attention; nothing is treated equally.
//! - Attention scores decay over time (novelty), and deprioritized items are heavily penalized but never deleted.
//! - CognitiveBlackboard is the cognitive state. Every engine reads from and writes to it. No engine talks directly to another.
//! - Workspace changes broadcast so observers can react without direct coupling.

pub mod confidence;
pub use confidence::*;
pub mod goal;
pub use goal::*;
pub mod executive;
pub use executive::*;
pub mod common;
pub use common::*;
pub mod world;
pub use world::*;
pub mod trace;
pub use trace::*;
pub mod planning;
pub use planning::*;
pub mod simulation;
pub use simulation::*;
pub mod verification;
pub use verification::*;
pub mod policy;
pub use policy::*;
pub mod learning;
pub use learning::*;
pub mod memory;
pub use memory::*;
pub mod experience;
pub use experience::*;
pub mod attention;
pub use attention::*;
pub mod workspace;
pub use workspace::*;
pub mod execution;
pub use execution::*;
pub mod evidence;
pub use evidence::*;
pub mod workflow;
pub use workflow::*;

// ── Tests ───────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use friday_reflection::{Insight, InsightKind, Outcome, OutcomeKind};
    use std::collections::HashMap;

    // ── Goal ────────────────────────────────────────────────────

    #[test]
    fn goal_creation() {
        let goal = Goal::new("open notepad");
        assert_eq!(goal.description, "open notepad");
        assert!(goal.context.is_none());
        assert!(goal.id.starts_with("goal_"));
    }

    #[test]
    fn goal_with_context() {
        let goal = Goal::new("build project").with_context("before commit");
        assert_eq!(goal.context.unwrap(), "before commit");
    }

    // ── Action ──────────────────────────────────────────────────

    #[test]
    fn action_single() {
        let goal = Goal::new("build project");
        let action = Action::single("cargo build", Capability::ExecuteCommand, &goal.id);
        assert_eq!(action.instruction, "cargo build");
        assert_eq!(action.capability, Capability::ExecuteCommand);
        assert!(action.is_first());
        assert!(action.is_last());
        assert!(!action.is_multi_step());
    }

    #[test]
    fn action_multi_step() {
        let goal = Goal::new("build project");
        let a1 = Action::step("cargo fmt", Capability::ExecuteCommand, &goal.id, 0, 3);
        let a3 = Action::step("cargo build", Capability::ExecuteCommand, &goal.id, 2, 3);
        assert!(a1.is_first());
        assert!(!a1.is_last());
        assert!(a3.is_last());
    }

    // ── Capability ──────────────────────────────────────────────

    #[test]
    fn capability_display() {
        assert_eq!(Capability::ExecuteCommand.to_string(), "execute_command");
        assert_eq!(Capability::ReadFile.to_string(), "read_file");
    }

    // ── Expectation ─────────────────────────────────────────────

    #[test]
    fn expectation_success() {
        let exp = Expectation::success("notepad will open");
        assert!(exp.expect_success);
    }

    #[test]
    fn expectation_failure() {
        let exp = Expectation::failure("build will fail");
        assert!(!exp.expect_success);
    }

    // ── World State ─────────────────────────────────────────────

    #[test]
    fn world_state_file_exists() {
        let world = WorldState::new().with_file(
            "/tmp/test.txt",
            FileMeta {
                path: "/tmp/test.txt".into(),
                exists: true,
                size_bytes: Some(100),
                is_directory: false,
                modified_at_ms: None,
            },
        );

        assert!(world.file_exists("/tmp/test.txt"));
        assert!(!world.file_exists("/tmp/missing.txt"));
    }

    #[test]
    fn world_state_has_tool() {
        let world = WorldState::new()
            .with_tool(Capability::ExecuteCommand)
            .with_tool(Capability::ReadFile);

        assert!(world.has_tool(&Capability::ExecuteCommand));
        assert!(world.has_tool(&Capability::ReadFile));
        assert!(!world.has_tool(&Capability::DeleteFile));
    }

    // ── Plan ────────────────────────────────────────────────────

    #[test]
    fn plan_from_action() {
        let goal = Goal::new("cargo build");
        let action = Action::single("cargo build", Capability::ExecuteCommand, &goal.id);
        let plan = Plan::from_action(action, "Direct build");

        assert_eq!(plan.action_count(), 1);
        assert_eq!(plan.source, PlanSource::Direct);
        assert!(!plan.has_prerequisites());
    }

    #[test]
    fn plan_from_actions() {
        let goal = Goal::new("build project");
        let actions = vec![
            Action::step("cargo fmt", Capability::ExecuteCommand, &goal.id, 0, 2),
            Action::step("cargo build", Capability::ExecuteCommand, &goal.id, 1, 2),
        ];
        let plan = Plan::from_actions(actions, "Format then build", PlanSource::PolicyDriven);

        assert_eq!(plan.action_count(), 2);
        assert_eq!(plan.source, PlanSource::PolicyDriven);
        assert_eq!(plan.estimated_cost, 2.0);
    }

    // ── Execution Context ───────────────────────────────────────

    #[test]
    fn execution_context_for_goal() {
        let ctx = ExecutionContext::for_goal("g1");
        assert_eq!(ctx.goal_id, "g1");
        assert!(ctx.mission_id.is_none());
    }

    // ── Tool Result ─────────────────────────────────────────────

    #[test]
    fn tool_result_success() {
        let r = ToolResult::success("hello");
        assert!(r.success);
        assert_eq!(r.output, "hello");
    }

    #[test]
    fn tool_result_error() {
        let r = ToolResult::error("not found");
        assert!(!r.success);
    }

    // ── Tool Registry ───────────────────────────────────────────

    #[test]
    fn registry_resolve_by_capability() {
        let mut reg = ToolRegistry::new();
        reg.register(Box::new(MockTool::success("terminal", "ok")));
        reg.register(Box::new(
            MockTool::success("filesystem", "ok").with_capabilities(vec![Capability::ReadFile]),
        ));

        assert_eq!(
            reg.resolve(&Capability::ExecuteCommand).unwrap().name(),
            "terminal"
        );
        assert_eq!(
            reg.resolve(&Capability::ReadFile).unwrap().name(),
            "filesystem"
        );
        assert!(reg.resolve(&Capability::DeleteFile).is_none());
    }

    // ── Verification ────────────────────────────────────────────

    #[test]
    fn verification_no_verifiers_trusts_tool() {
        let verification = Verification::new();
        let action = Action::single("echo hi", Capability::ExecuteCommand, "g1");
        let result = ToolResult::success("output");
        let verified = verification.verify(&action, &result);
        assert!(verified.verified);
    }

    #[test]
    fn verification_failing_verifier() {
        let verification = Verification::new().register(Box::new(MockVerifier::fail("checksum")));
        let action = Action::single("download", Capability::ExecuteCommand, "g1");
        let result = ToolResult::success("content");
        let verified = verification.verify(&action, &result);
        assert!(!verified.verified);
        assert!(verified.tool_success);
    }

    // ── Policy ──────────────────────────────────────────────────

    #[test]
    fn policy_prefer_tool() {
        let p = Policy::prefer_tool(Capability::ReadFile, "filesystem", 0.9, 5);
        assert_eq!(p.kind, PolicyKind::PreferTool);
        assert!(p.statement.contains("filesystem"));
    }

    #[test]
    fn policy_retry() {
        let p = Policy::retry(Capability::ExecuteCommand, 2, 0.8, 3);
        assert_eq!(p.kind, PolicyKind::Retry);
    }

    #[test]
    fn policy_immutable_with_evidence() {
        let p = Policy::retry(Capability::ExecuteCommand, 1, 0.8, 3)
            .with_created_from(vec!["i1".into()])
            .with_expires_at(1000);
        assert_eq!(p.created_from, vec!["i1"]);
        assert!(!p.is_expired(999));
        assert!(p.is_expired(1001));
    }

    // ── Learning Engine ─────────────────────────────────────────

    #[test]
    fn learning_from_lesson_produces_retry_policy() {
        let mut engine = LearningEngine::new();
        let insight = Insight {
            id: "i1".into(),
            kind: InsightKind::Lesson,
            knowledge_id: "k1".into(),
            outcome_id: "o1".into(),
            statement: "cargo build failed".into(),
            confidence: 0.8,
            derived_at_ms: 1000,
        };
        let policies = engine.learn(&[insight]);
        assert_eq!(policies.len(), 1);
        assert_eq!(policies[0].kind, PolicyKind::Retry);
    }

    #[test]
    fn learning_prune_expired() {
        let mut engine = LearningEngine::new();
        engine.learn(&[Insight {
            id: "i1".into(),
            kind: InsightKind::Lesson,
            knowledge_id: "k1".into(),
            outcome_id: "o1".into(),
            statement: "failed".into(),
            confidence: 0.8,
            derived_at_ms: 1000,
        }]);
        assert_eq!(engine.policy_count(), 1);

        let policy_id = engine.policies()[0].id.clone();
        engine.policies.iter_mut().for_each(|p| {
            if p.id == policy_id {
                p.expires_at = Some(100);
            }
        });

        engine.prune_expired(50);
        assert_eq!(engine.policy_count(), 1);
        engine.prune_expired(150);
        assert_eq!(engine.policy_count(), 0);
    }

    // ── Plan Evaluator ──────────────────────────────────────────

    #[test]
    fn evaluator_prefers_higher_confidence() {
        let evaluator = PlanEvaluator::new();
        let world = WorldState::new().with_tool(Capability::ExecuteCommand);

        let goal = Goal::new("cargo build");
        let action = Action::single("cargo build", Capability::ExecuteCommand, &goal.id);

        let c1 = PlanCandidate {
            plan: Plan::from_action(action.clone(), "Low confidence").with_confidence(0.3),
            preliminary_score: 0.3,
        };
        let c2 = PlanCandidate {
            plan: Plan::from_action(action, "High confidence").with_confidence(0.9),
            preliminary_score: 0.9,
        };

        let ranked = evaluator.evaluate(&[c1, c2], &world);
        assert!(ranked.score > 0.5);
        assert!(ranked.plan.strategy.contains("High confidence"));
    }

    #[test]
    fn evaluator_penalizes_unmet_prerequisites() {
        let evaluator = PlanEvaluator::new();
        let world = WorldState::new();

        let goal = Goal::new("cargo build");
        let action = Action::single("cargo build", Capability::ExecuteCommand, &goal.id);

        let plan_with_prereq = Plan::from_action(action, "Needs file")
            .with_prerequisites(vec!["file_exists:/tmp/required.txt".into()])
            .with_confidence(0.9);

        let plan_no_prereq = Plan::from_action(
            Action::single("cargo build", Capability::ExecuteCommand, &goal.id),
            "No prerequisites",
        )
        .with_confidence(0.7);

        let ranked = evaluator.evaluate(
            &[
                PlanCandidate {
                    plan: plan_with_prereq,
                    preliminary_score: 0.9,
                },
                PlanCandidate {
                    plan: plan_no_prereq,
                    preliminary_score: 0.7,
                },
            ],
            &world,
        );

        // Plan with unmet prereqs should lose despite higher confidence
        assert!(ranked.plan.strategy.contains("No prerequisites"));
    }

    #[test]
    fn evaluator_penalizes_missing_tools() {
        let evaluator = PlanEvaluator::new();
        let world = WorldState::new(); // no tools

        let goal = Goal::new("read file");
        let action = Action::single("read file", Capability::ReadFile, &goal.id);

        let plan = Plan::from_action(action, "Read without tool").with_confidence(0.9);
        let ranked = evaluator.evaluate(
            &[PlanCandidate {
                plan,
                preliminary_score: 0.9,
            }],
            &world,
        );

        // Score should be penalized
        assert!(ranked.score < 0.5);
    }

    #[test]
    fn evaluator_empty_panics() {
        let evaluator = PlanEvaluator::new();
        let world = WorldState::new();
        let result = std::panic::catch_unwind(|| {
            evaluator.evaluate(&[], &world);
        });
        assert!(result.is_err());
    }

    // ── Planning (think) ───────────────────────────────────────

    #[test]
    fn think_generates_candidates() {
        let planning = Planning::new();
        let goal = Goal::new("cargo build");
        let ctx = PlanningContext::new(goal);
        let candidates = planning.think(&ctx);

        // Should generate at least 1 candidate (direct)
        assert!(!candidates.is_empty());
        assert_eq!(candidates[0].plan.source, PlanSource::Direct);
    }

    #[test]
    fn think_with_policy_generates_more_candidates() {
        let planning = Planning::new();
        let goal = Goal::new("cargo build");
        let policy = Policy::sequence(Capability::ExecuteCommand, Capability::ReadFile, 0.8, 5);
        let ctx = PlanningContext::new(goal).with_policies(vec![policy]);
        let candidates = planning.think(&ctx);

        // Should generate 2 candidates (direct + policy)
        assert!(candidates.len() >= 2);
    }

    #[test]
    fn planning_legacy_plan_still_works() {
        let planning = Planning::new();
        let goal = Goal::new("cargo build");
        let ctx = PlanningContext::new(goal);
        let actions = planning.plan(&ctx);

        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0].capability, Capability::ExecuteCommand);
    }

    // ── Full Pipeline (Deliberative) ───────────────────────────

    #[tokio::test]
    async fn deliberative_pipeline() {
        let mut reg = ToolRegistry::new();
        reg.register(Box::new(MockTool::success("cargo", "Build complete")));
        let planning = Planning::new();
        let evaluator = PlanEvaluator::new();
        let mut engine = GoalEngine::new().with_registry(reg);
        let mut learning = LearningEngine::new();

        let goal = Goal::new("cargo build");
        let world = WorldState::new().with_tool(Capability::ExecuteCommand);

        // 1. Think: generate candidates
        let ctx = PlanningContext::new(goal.clone()).with_world_state(world.clone());
        let candidates = planning.think(&ctx);
        assert!(!candidates.is_empty());

        // 2. Evaluate: choose best plan
        let ranked = evaluator.evaluate(&candidates, &world);
        assert!(ranked.score > 0.0);
        assert!(!ranked.plan.actions.is_empty());

        // 3. Execute
        let exp = Expectation::success("build will succeed");
        let exec_ctx = ExecutionContext::for_goal(&goal.id);
        let report = engine
            .execute(&goal, &ranked.plan.actions[0], &exp, &exec_ctx)
            .await;
        assert!(report.success());

        // 4. Learn
        let policies = learning.learn(&report.insights);
        assert!(policies.is_empty());
    }

    // ── Mission ─────────────────────────────────────────────────

    #[test]
    fn mission_progression() {
        let mut mission = Mission::new(
            "prepare release",
            vec!["format".into(), "test".into(), "build".into()],
        );
        mission.start();
        assert_eq!(mission.next_goal(), Some(0));

        let outcome = Outcome {
            id: "o1".into(),
            kind: OutcomeKind::TaskCompleted,
            context: "format".into(),
            detail: serde_json::json!({}),
            occurred_at_ms: 1000,
        };
        mission.complete_goal(0, outcome);
        assert_eq!(mission.completed_goals(), 1);
        assert_eq!(mission.next_goal(), Some(1));
    }

    // ── WorldState Diff ─────────────────────────────────────────

    #[test]
    fn diff_detects_added_file() {
        let a = WorldState::new();
        let b = WorldState::new().with_file(
            "new.txt",
            FileMeta {
                path: "new.txt".into(),
                exists: true,
                size_bytes: Some(100),
                is_directory: false,
                modified_at_ms: None,
            },
        );
        let diff = a.diff(&b);
        assert_eq!(diff.added_files, vec!["new.txt".to_string()]);
        assert!(!diff.is_empty());
    }

    #[test]
    fn diff_detects_modified_file() {
        let a = WorldState::new().with_file(
            "f.txt",
            FileMeta {
                path: "f.txt".into(),
                exists: true,
                size_bytes: Some(10),
                is_directory: false,
                modified_at_ms: None,
            },
        );
        let b = WorldState::new().with_file(
            "f.txt",
            FileMeta {
                path: "f.txt".into(),
                exists: true,
                size_bytes: Some(20),
                is_directory: false,
                modified_at_ms: None,
            },
        );
        let diff = a.diff(&b);
        assert_eq!(diff.modified_files, vec!["f.txt".to_string()]);
    }

    #[test]
    fn diff_same_state_is_empty() {
        let a = WorldState::new().with_variable("k", serde_json::json!(1));
        let b = WorldState::new().with_variable("k", serde_json::json!(1));
        assert!(a.diff(&b).is_empty());
    }

    #[test]
    fn diff_detects_variable_change() {
        let a = WorldState::new().with_variable("k", serde_json::json!(1));
        let b = WorldState::new().with_variable("k", serde_json::json!(2));
        let diff = a.diff(&b);
        assert_eq!(diff.changed_vars, vec!["k".to_string()]);
    }

    // ── Preconditions ───────────────────────────────────────────

    #[test]
    fn precondition_file_exists_passes() {
        let state = WorldState::new().with_file(
            "x.txt",
            FileMeta {
                path: "x.txt".into(),
                exists: true,
                size_bytes: None,
                is_directory: false,
                modified_at_ms: None,
            },
        );
        assert!(Precondition::FileExists {
            path: "x.txt".into()
        }
        .check(&state));
    }

    #[test]
    fn precondition_file_exists_fails() {
        let state = WorldState::new();
        assert!(!Precondition::FileExists {
            path: "x.txt".into()
        }
        .check(&state));
    }

    #[test]
    fn precondition_file_not_exists_passes() {
        let state = WorldState::new();
        assert!(Precondition::FileNotExists {
            path: "x.txt".into()
        }
        .check(&state));
    }

    #[test]
    fn precondition_tool_available() {
        let state = WorldState::new().with_tool(Capability::ReadFile);
        assert!(Precondition::ToolAvailable {
            capability: Capability::ReadFile
        }
        .check(&state));
        assert!(!Precondition::ToolAvailable {
            capability: Capability::WriteFile
        }
        .check(&state));
    }

    #[test]
    fn precondition_variable_equals() {
        let state = WorldState::new().with_variable("mode", serde_json::json!("fast"));
        assert!(Precondition::VariableEquals {
            key: "mode".into(),
            value: serde_json::json!("fast")
        }
        .check(&state));
        assert!(!Precondition::VariableEquals {
            key: "mode".into(),
            value: serde_json::json!("slow")
        }
        .check(&state));
    }

    #[test]
    fn precondition_network_available() {
        let state = WorldState::default();
        assert!(!Precondition::NetworkAvailable.check(&state));
        let state = WorldState::new().with_network(true);
        assert!(Precondition::NetworkAvailable.check(&state));
    }

    #[test]
    fn precondition_describe() {
        let p = Precondition::FileExists {
            path: "a.txt".into(),
        };
        assert_eq!(p.describe(), "file exists: a.txt");
        let p = Precondition::ToolAvailable {
            capability: Capability::ExecuteCommand,
        };
        assert_eq!(p.describe(), "tool available: execute_command");
    }

    // ── Postconditions ──────────────────────────────────────────

    #[test]
    fn postcondition_describe() {
        let p = Postcondition::FileCreated {
            path: "out.txt".into(),
        };
        assert_eq!(p.describe(), "file created: out.txt");
        let p = Postcondition::VariableSet {
            key: "result".into(),
        };
        assert_eq!(p.describe(), "variable set: result");
    }

    // ── State Transition ────────────────────────────────────────

    #[test]
    fn transition_records_diff() {
        let before = WorldState::new();
        let after = WorldState::new().with_file(
            "new.txt",
            FileMeta {
                path: "new.txt".into(),
                exists: true,
                size_bytes: None,
                is_directory: false,
                modified_at_ms: None,
            },
        );
        let t = StateTransition::new("a1", "g1", before, after, &[]);
        assert!(!t.diff.is_empty());
        assert!(t.preconditions_met);
        assert!(t.preconditions_failed.is_empty());
    }

    #[test]
    fn transition_records_failed_preconditions() {
        let before = WorldState::new();
        let after = WorldState::new();
        let preconditions = vec![
            Precondition::FileExists {
                path: "missing.txt".into(),
            },
            Precondition::ToolAvailable {
                capability: Capability::ReadFile,
            },
        ];
        let t = StateTransition::new("a1", "g1", before, after, &preconditions);
        assert!(!t.preconditions_met);
        assert_eq!(t.preconditions_failed.len(), 2);
    }

    // ── World Model ─────────────────────────────────────────────

    #[test]
    fn world_model_check_preconditions() {
        let model = WorldModel::new(WorldState::new().with_tool(Capability::ReadFile));
        let failed = model.check_preconditions(&[
            Precondition::ToolAvailable {
                capability: Capability::ReadFile,
            },
            Precondition::ToolAvailable {
                capability: Capability::WriteFile,
            },
        ]);
        assert_eq!(failed.len(), 1);
        assert!(failed[0].contains("write_file"));
    }

    #[test]
    fn world_model_record_transition() {
        let mut model = WorldModel::new(WorldState::new());
        let new_state = WorldState::new().with_variable("step", serde_json::json!(1));
        let t = model.record_transition("a1", "g1", new_state, &[]);
        assert_eq!(model.history().len(), 1);
        assert_eq!(t.action_id, "a1");
        assert_eq!(
            model.state().variables.get("step"),
            Some(&serde_json::json!(1))
        );
    }

    #[test]
    fn world_model_apply_postconditions() {
        let mut model = WorldModel::new(WorldState::new());
        model.apply_postconditions(&[
            Postcondition::FileCreated {
                path: "output.txt".into(),
            },
            Postcondition::VariableSet { key: "done".into() },
        ]);
        assert!(model.state().file_exists("output.txt"));
        assert!(model.state().variables.contains_key("done"));
    }

    #[test]
    fn world_model_setters() {
        let mut model = WorldModel::new(WorldState::new());
        model.set_working_directory("/tmp");
        assert_eq!(model.state().working_directory.as_deref(), Some("/tmp"));
        model.set_variable("x", serde_json::json!(42));
        assert_eq!(
            model.state().variables.get("x"),
            Some(&serde_json::json!(42))
        );
    }

    // ── Cognitive Trace ─────────────────────────────────────────

    #[test]
    fn trace_node_basic() {
        let node = TraceNode::new(
            TraceNodeKind::Planning,
            serde_json::json!("input"),
            serde_json::json!("output"),
        );
        assert_eq!(node.kind, TraceNodeKind::Planning);
        assert!(node.parent_id.is_none());
    }

    #[test]
    fn trace_node_with_parent_and_confidence() {
        let parent = TraceNode::new(
            TraceNodeKind::Planning,
            serde_json::json!({}),
            serde_json::json!({}),
        );
        let child = TraceNode::new(
            TraceNodeKind::Evaluation,
            serde_json::json!({}),
            serde_json::json!({}),
        )
        .with_parent(&parent.id)
        .with_confidence(0.85);
        assert_eq!(child.parent_id.as_deref(), Some(parent.id.as_str()));
        assert_eq!(child.confidence, Some(0.85));
    }

    #[test]
    fn trace_node_confidence_clamped() {
        let node = TraceNode::new(
            TraceNodeKind::Execution,
            serde_json::json!({}),
            serde_json::json!({}),
        )
        .with_confidence(1.5);
        assert_eq!(node.confidence, Some(1.0));
        let node = TraceNode::new(
            TraceNodeKind::Execution,
            serde_json::json!({}),
            serde_json::json!({}),
        )
        .with_confidence(-0.5);
        assert_eq!(node.confidence, Some(0.0));
    }

    #[test]
    fn cognitive_trace_nodes_of_kind() {
        let mut trace = CognitiveTrace::new("g1");
        trace.push(TraceNode::new(
            TraceNodeKind::Planning,
            serde_json::json!({}),
            serde_json::json!({}),
        ));
        trace.push(TraceNode::new(
            TraceNodeKind::Execution,
            serde_json::json!({}),
            serde_json::json!({}),
        ));
        trace.push(TraceNode::new(
            TraceNodeKind::Planning,
            serde_json::json!({}),
            serde_json::json!({}),
        ));
        assert_eq!(trace.nodes_of_kind(&TraceNodeKind::Planning).len(), 2);
        assert_eq!(trace.nodes_of_kind(&TraceNodeKind::Execution).len(), 1);
    }

    #[test]
    fn cognitive_trace_display() {
        let mut trace = CognitiveTrace::new("g1");
        trace.push(
            TraceNode::new(
                TraceNodeKind::Planning,
                serde_json::json!({}),
                serde_json::json!({}),
            )
            .with_confidence(0.7),
        );
        trace.push(TraceNode::new(
            TraceNodeKind::Evaluation,
            serde_json::json!({}),
            serde_json::json!({}),
        ));
        let display = trace.display();
        assert!(display.contains("Goal: g1"));
        assert!(display.contains("Planning"));
        assert!(display.contains("0.70"));
    }

    // ── GoalEngine Cognitive Trace ──────────────────────────────

    #[tokio::test]
    async fn execute_produces_cognitive_trace() {
        let mut reg = ToolRegistry::new();
        reg.register(Box::new(MockTool::success("cargo", "ok")));
        let mut engine = GoalEngine::new().with_registry(reg);

        let goal = Goal::new("cargo build");
        let action = Action::single("cargo build", Capability::ExecuteCommand, &goal.id);
        let exp = Expectation::success("will succeed");
        let exec_ctx = ExecutionContext::for_goal(&goal.id);
        let report = engine.execute(&goal, &action, &exp, &exec_ctx).await;

        let trace = &report.cognitive_trace;
        assert_eq!(trace.goal_id, goal.id);
        assert!(
            trace.nodes.len() >= 3,
            "trace should have execution, verification, reflection nodes"
        );

        let kinds: Vec<_> = trace.nodes.iter().map(|n| n.kind.clone()).collect();
        assert!(kinds.contains(&TraceNodeKind::Execution));
        assert!(kinds.contains(&TraceNodeKind::Verification));
        assert!(kinds.contains(&TraceNodeKind::Reflection));
    }

    #[tokio::test]
    async fn cognitive_trace_records_timing() {
        let mut reg = ToolRegistry::new();
        reg.register(Box::new(MockTool::success("echo", "ok")));
        let mut engine = GoalEngine::new().with_registry(reg);

        let goal = Goal::new("echo test");
        let action = Action::single("echo", Capability::ExecuteCommand, &goal.id);
        let exp = Expectation::success("will work");
        let exec_ctx = ExecutionContext::for_goal(&goal.id);
        let report = engine.execute(&goal, &action, &exp, &exec_ctx).await;

        for node in &report.cognitive_trace.nodes {
            assert!(node.finished_at_ms >= node.started_at_ms);
        }
    }

    // ── Risk Model ──────────────────────────────────────────────

    #[test]
    fn risk_read_is_low() {
        let r = assess_risk(&Capability::ReadFile);
        assert_eq!(r.risk, Risk::Low);
    }

    #[test]
    fn risk_delete_is_high() {
        let r = assess_risk(&Capability::DeleteFile);
        assert_eq!(r.risk, Risk::High);
        assert!(r.risk.requires_approval());
    }

    #[test]
    fn risk_execute_is_medium() {
        let r = assess_risk(&Capability::ExecuteCommand);
        assert_eq!(r.risk, Risk::Medium);
        assert!(!r.risk.requires_approval());
    }

    #[test]
    fn risk_display() {
        assert_eq!(Risk::Low.to_string(), "low");
        assert_eq!(Risk::Critical.to_string(), "critical");
    }

    #[test]
    fn risk_ordering() {
        assert!(Risk::Low < Risk::Medium);
        assert!(Risk::Medium < Risk::High);
        assert!(Risk::High < Risk::Critical);
    }

    // ── Simulation ──────────────────────────────────────────────

    #[test]
    fn simulator_feasible_plan() {
        let sim = DefaultSimulator::new();
        let world = WorldState::new().with_tool(Capability::ExecuteCommand);
        let goal = Goal::new("cargo build");
        let action = Action::single("cargo build", Capability::ExecuteCommand, &goal.id);
        let plan = Plan::from_action(action, "build");

        let result = sim.simulate(&plan, &world);
        assert!(result.feasible);
        assert!(result.confidence > 0.0);
        assert!(result.failed_preconditions.is_empty());
        assert_eq!(result.risks.len(), 1);
        assert_eq!(result.risks[0].risk, Risk::Medium);
    }

    #[test]
    fn simulator_infeasible_plan() {
        let sim = DefaultSimulator::new();
        let world = WorldState::new(); // no tools available
        let goal = Goal::new("read file");
        let action = Action::single("read file", Capability::ReadFile, &goal.id);
        let plan = Plan::from_action(action, "read");

        let result = sim.simulate(&plan, &world);
        assert!(!result.feasible);
        assert!(!result.failed_preconditions.is_empty());
    }

    #[test]
    fn simulator_multi_step() {
        let sim = DefaultSimulator::new();
        let world = WorldState::new()
            .with_tool(Capability::ExecuteCommand)
            .with_tool(Capability::ReadFile);
        let goal = Goal::new("build and verify");
        let a1 = Action::step("cargo build", Capability::ExecuteCommand, &goal.id, 0, 2);
        let a2 = Action::step("read output", Capability::ReadFile, &goal.id, 1, 2);
        let plan = Plan::from_actions(vec![a1, a2], "build then read", PlanSource::Direct);

        let result = sim.simulate(&plan, &world);
        assert!(result.feasible);
        assert_eq!(result.step_results.len(), 2);
        assert!(result.expected_postconditions.len() >= 2);
    }

    #[test]
    fn simulator_aggregate_risk() {
        let sim = DefaultSimulator::new();
        let world = WorldState::new()
            .with_tool(Capability::ExecuteCommand)
            .with_tool(Capability::DeleteFile);
        let goal = Goal::new("dangerous");
        let a1 = Action::step("run", Capability::ExecuteCommand, &goal.id, 0, 2);
        let a2 = Action::step("delete", Capability::DeleteFile, &goal.id, 1, 2);
        let plan = Plan::from_actions(vec![a1, a2], "run then delete", PlanSource::Direct);

        let result = sim.simulate(&plan, &world);
        assert_eq!(result.aggregate_risk, Risk::High);
    }

    // ── Trace Recorder ──────────────────────────────────────────

    #[test]
    fn trace_recorder_assembles_trace() {
        let mut recorder = TraceRecorder::new("g1");
        recorder.record(TraceEvent::PlanningStarted {
            goal_id: "g1".into(),
        });
        recorder.record(TraceEvent::PlanGenerated {
            goal_id: "g1".into(),
            candidate_count: 3,
        });
        recorder.record(TraceEvent::ExecutionStarted {
            goal_id: "g1".into(),
            action_id: "a1".into(),
        });
        recorder.record(TraceEvent::ExecutionFinished {
            goal_id: "g1".into(),
            action_id: "a1".into(),
            success: true,
        });

        let trace = recorder.trace();
        assert_eq!(trace.goal_id, "g1");
        assert!(trace.nodes.len() >= 3);
    }

    #[test]
    fn trace_recorder_event_log() {
        let mut recorder = TraceRecorder::new("g1");
        recorder.record(TraceEvent::PlanningStarted {
            goal_id: "g1".into(),
        });
        recorder.record(TraceEvent::SimulationFinished {
            goal_id: "g1".into(),
            plan_id: "p1".into(),
            feasible: true,
            confidence: 0.9,
        });
        assert_eq!(recorder.events().len(), 2);
    }

    #[test]
    fn trace_recorder_into_trace() {
        let mut recorder = TraceRecorder::new("g1");
        recorder.record(TraceEvent::PlanningStarted {
            goal_id: "g1".into(),
        });
        let trace = recorder.into_trace();
        assert_eq!(trace.goal_id, "g1");
        assert_eq!(trace.nodes.len(), 1);
    }

    #[test]
    fn trace_recorder_records_investigation() {
        let mut recorder = TraceRecorder::new("g1");
        recorder.record(TraceEvent::InvestigationFinished {
            goal_id: "g1".into(),
            requirement_count: 2,
            channels: vec!["web".into(), "git".into()],
            confidence: 0.85,
            self_contained: false,
        });
        let trace = recorder.trace();
        assert_eq!(trace.nodes_of_kind(&TraceNodeKind::Investigation).len(), 1);
        let node = &trace.nodes[0];
        assert_eq!(node.kind, TraceNodeKind::Investigation);
        assert_eq!(node.output["requirement_count"], serde_json::json!(2));
        assert_eq!(node.output["channels"], serde_json::json!(["web", "git"]));
        assert_eq!(node.confidence, Some(0.85));
    }

    // ── Confidence ──────────────────────────────────────────────

    #[test]
    fn confidence_semantic_constructors() {
        assert_eq!(Confidence::none().value(), 0.0);
        assert_eq!(Confidence::low().value(), 0.25);
        assert_eq!(Confidence::medium().value(), 0.5);
        assert_eq!(Confidence::high().value(), 0.75);
        assert_eq!(Confidence::certain().value(), 1.0);
    }

    #[test]
    fn confidence_from_raw_clamped() {
        assert_eq!(Confidence::from_raw(1.5).value(), 1.0);
        assert_eq!(Confidence::from_raw(-0.5).value(), 0.0);
    }

    #[test]
    fn confidence_from_ratio() {
        let c = Confidence::from_ratio(3, 4);
        assert!((c.value() - 0.75).abs() < 0.001);
        let c = Confidence::from_ratio(0, 0);
        assert_eq!(c.value(), 0.0);
    }

    #[test]
    fn confidence_and_multiply() {
        let a = Confidence::high(); // 0.75
        let b = Confidence::medium(); // 0.5
        let result = a.and(b);
        assert!((result.value() - 0.375).abs() < 0.001);
    }

    #[test]
    fn confidence_or_average() {
        let a = Confidence::high(); // 0.75
        let b = Confidence::low(); // 0.25
        let result = a.or(b);
        assert!((result.value() - 0.5).abs() < 0.001);
    }

    #[test]
    fn confidence_discount() {
        let c = Confidence::high(); // 0.75
        let discounted = c.discount(0.5);
        assert!((discounted.value() - 0.375).abs() < 0.001);
    }

    #[test]
    fn confidence_level_classification() {
        assert_eq!(Confidence::none().level(), ConfidenceLevel::None);
        assert_eq!(Confidence::low().level(), ConfidenceLevel::Low);
        assert_eq!(Confidence::medium().level(), ConfidenceLevel::Medium);
        assert_eq!(Confidence::high().level(), ConfidenceLevel::High);
        assert_eq!(Confidence::certain().level(), ConfidenceLevel::Certain);
    }

    #[test]
    fn confidence_meets_threshold() {
        assert!(Confidence::high().meets(Confidence::medium()));
        assert!(!Confidence::low().meets(Confidence::medium()));
        assert!(Confidence::certain().meets(Confidence::certain()));
    }

    #[test]
    fn confidence_display() {
        assert_eq!(Confidence::medium().to_string(), "50%");
        assert_eq!(Confidence::certain().to_string(), "100%");
    }

    #[test]
    fn confidence_mul_operator() {
        let a = Confidence::high();
        let b = Confidence::high();
        let result = a * b;
        assert!((result.value() - 0.5625).abs() < 0.001);
    }

    #[test]
    fn confidence_into_f32() {
        let c = Confidence::from_raw(0.42);
        let v: f32 = c.into();
        assert!((v - 0.42).abs() < 0.001);
    }

    // ── Confidence Engine ───────────────────────────────────────

    #[test]
    fn confidence_engine_multiply() {
        let engine = ConfidenceEngine::new().add_rule(
            "combined",
            ConfidenceRule::Multiply {
                sources: vec!["a".into(), "b".into()],
            },
        );

        let mut inputs = HashMap::new();
        inputs.insert("a".into(), Confidence::high()); // 0.75
        inputs.insert("b".into(), Confidence::medium()); // 0.5

        let outputs = engine.evaluate(&inputs);
        let combined = outputs.get("combined").unwrap();
        assert!((combined.value() - 0.375).abs() < 0.001);
    }

    #[test]
    fn confidence_engine_average() {
        let engine = ConfidenceEngine::new().add_rule(
            "avg",
            ConfidenceRule::Average {
                sources: vec!["a".into(), "b".into()],
            },
        );

        let mut inputs = HashMap::new();
        inputs.insert("a".into(), Confidence::high()); // 0.75
        inputs.insert("b".into(), Confidence::low()); // 0.25

        let outputs = engine.evaluate(&inputs);
        let avg = outputs.get("avg").unwrap();
        assert!((avg.value() - 0.5).abs() < 0.001);
    }

    #[test]
    fn confidence_engine_discount() {
        let engine = ConfidenceEngine::new().add_rule(
            "discounted",
            ConfidenceRule::Discount {
                source: "a".into(),
                factor: 0.5,
            },
        );

        let mut inputs = HashMap::new();
        inputs.insert("a".into(), Confidence::high()); // 0.75

        let outputs = engine.evaluate(&inputs);
        let discounted = outputs.get("discounted").unwrap();
        assert!((discounted.value() - 0.375).abs() < 0.001);
    }

    #[test]
    fn confidence_engine_cap() {
        let engine = ConfidenceEngine::new().add_rule(
            "capped",
            ConfidenceRule::Cap {
                source: "a".into(),
                max: Confidence::medium(),
            },
        );

        let mut inputs = HashMap::new();
        inputs.insert("a".into(), Confidence::certain()); // 1.0

        let outputs = engine.evaluate(&inputs);
        let capped = outputs.get("capped").unwrap();
        assert_eq!(capped.value(), 0.5);
    }

    #[test]
    fn confidence_engine_min() {
        let engine = ConfidenceEngine::new().add_rule(
            "min_val",
            ConfidenceRule::Min {
                sources: vec!["a".into(), "b".into()],
            },
        );

        let mut inputs = HashMap::new();
        inputs.insert("a".into(), Confidence::high()); // 0.75
        inputs.insert("b".into(), Confidence::low()); // 0.25

        let outputs = engine.evaluate(&inputs);
        let min_val = outputs.get("min_val").unwrap();
        assert_eq!(min_val.value(), 0.25);
    }

    #[test]
    fn confidence_engine_missing_input_defaults_to_none() {
        let engine = ConfidenceEngine::new().add_rule(
            "result",
            ConfidenceRule::Multiply {
                sources: vec!["a".into(), "missing".into()],
            },
        );

        let mut inputs = HashMap::new();
        inputs.insert("a".into(), Confidence::high());

        let outputs = engine.evaluate(&inputs);
        let result = outputs.get("result").unwrap();
        assert_eq!(result.value(), 0.0); // 0.75 * 0.0 = 0.0
    }

    // ── Belief ──────────────────────────────────────────────────

    #[test]
    fn belief_basic() {
        let b = Belief::new(
            "cargo builds on this machine",
            Confidence::high(),
            "observation",
        );
        assert_eq!(b.statement, "cargo builds on this machine");
        assert_eq!(b.confidence, Confidence::high());
        assert_eq!(b.source, "observation");
    }

    #[test]
    fn belief_fresh_by_default() {
        let b = Belief::new("x", Confidence::medium(), "test");
        let now = chrono::Utc::now().timestamp_millis() as u64;
        assert!(b.is_fresh(now));
        assert!(b.is_still_believed());
        assert!(b.is_usable(now));
    }

    #[test]
    fn belief_expired() {
        let b = Belief::new("x", Confidence::medium(), "test").with_expiry_ms(100);
        // 200ms in the future
        assert!(!b.is_fresh(200));
    }

    #[test]
    fn belief_below_floor() {
        let b =
            Belief::new("x", Confidence::low(), "test").with_confidence_floor(Confidence::medium());
        assert!(!b.is_still_believed());
    }

    #[test]
    fn belief_verify_updates_confidence() {
        let mut b = Belief::new("x", Confidence::medium(), "test");
        let now = chrono::Utc::now().timestamp_millis() as u64;
        b.verify(Confidence::high(), now);
        assert_eq!(b.confidence, Confidence::high());
        assert_eq!(b.last_verified_ms, now);
    }

    #[test]
    fn belief_with_evidence() {
        let b = Belief::new("x", Confidence::medium(), "test")
            .with_evidence("ep_123")
            .with_evidence("ep_456");
        assert_eq!(b.evidence.len(), 2);
    }

    // ── Attention Score ─────────────────────────────────────────

    #[test]
    fn attention_score_zero() {
        let s = AttentionScore::zero();
        assert_eq!(s.aggregate(), 0.0);
    }

    #[test]
    fn attention_score_builder() {
        let s = AttentionScore::builder()
            .importance(0.8)
            .urgency(0.5)
            .novelty(0.3)
            .risk(0.6)
            .confidence(0.9)
            .mission_priority(0.4)
            .build();
        let agg = s.aggregate();
        assert!(
            agg > 0.5 && agg < 1.0,
            "aggregate should be reasonable, got {}",
            agg
        );
    }

    #[test]
    fn attention_score_weighted() {
        // All weight on importance → aggregate = 0.5 * 0.3 = 0.15
        let s = AttentionScore {
            importance: 0.5,
            ..AttentionScore::zero()
        };
        assert!((s.aggregate() - 0.15).abs() < 0.001);
    }

    // ── Working Memory ──────────────────────────────────────────

    #[test]
    fn working_memory_default() {
        let wm = WorkingMemory::new();
        assert_eq!(wm.focus, FocusTarget::Idle);
        assert!(wm.active_goal.is_none());
        assert!(wm.recent_events.is_empty());
        assert!(wm.beliefs.is_empty());
        assert_eq!(wm.context_window, 50);
    }

    #[test]
    fn working_memory_events_ring_buffer() {
        let mut wm = WorkingMemory::new().with_context_window(3);
        for i in 0..5 {
            wm.push_event(WorkspaceEventRef {
                event_id: format!("e{}", i),
                topic: "test".into(),
                engine: "test".into(),
                timestamp_ms: i,
            });
        }
        assert_eq!(wm.recent_events().len(), 3);
        // Oldest (e0, e1) dropped, retained e2, e3, e4
        assert_eq!(wm.recent_events()[0].event_id, "e2");
        assert_eq!(wm.recent_events()[2].event_id, "e4");
    }

    #[test]
    fn working_memory_beliefs() {
        let mut wm = WorkingMemory::new();
        let idx = wm.add_belief(Belief::new("a", Confidence::high(), "test"));
        assert_eq!(idx, 0);
        wm.add_belief(Belief::new("b", Confidence::low(), "test"));
        assert_eq!(wm.beliefs.len(), 2);
    }

    #[test]
    fn working_memory_prune_stale_beliefs() {
        let mut wm = WorkingMemory::new();
        wm.add_belief(Belief::new("fresh", Confidence::high(), "test"));
        wm.add_belief(Belief::new("stale", Confidence::high(), "test").with_expiry_ms(100));

        let now = chrono::Utc::now().timestamp_millis() as u64;
        let removed = wm.prune_stale_beliefs(now + 200);
        assert_eq!(removed.len(), 1);
        assert_eq!(removed[0].statement, "stale");
        assert_eq!(wm.beliefs.len(), 1);
    }

    #[test]
    fn working_memory_active_beliefs() {
        let mut wm = WorkingMemory::new();
        wm.add_belief(Belief::new("a", Confidence::high(), "test"));
        wm.add_belief(
            Belief::new("b", Confidence::low(), "test").with_confidence_floor(Confidence::medium()),
        );

        let now = chrono::Utc::now().timestamp_millis() as u64;
        let active = wm.active_beliefs(now);
        assert_eq!(active.len(), 1);
        assert_eq!(active[0].statement, "a");
    }

    #[test]
    fn working_memory_top_attended() {
        let mut wm = WorkingMemory::new();
        wm.set_attention("a", AttentionScore::builder().importance(0.9).build());
        wm.set_attention("b", AttentionScore::builder().importance(0.3).build());
        wm.set_attention("c", AttentionScore::builder().importance(0.7).build());

        let top = wm.top_attended(2);
        assert_eq!(top.len(), 2);
        assert_eq!(top[0].0, "a");
        assert_eq!(top[1].0, "c");
    }

    #[test]
    fn working_memory_clear() {
        let mut wm = WorkingMemory::new();
        wm.set_goal(Goal::new("test"));
        wm.add_belief(Belief::new("x", Confidence::medium(), "test"));
        wm.set_attention("a", AttentionScore::zero());
        wm.clear();
        assert!(wm.active_goal.is_none());
        assert!(wm.beliefs.is_empty());
        assert!(wm.attention_scores.is_empty());
        assert_eq!(wm.focus, FocusTarget::Idle);
    }

    #[test]
    fn working_memory_focus_transitions() {
        let mut wm = WorkingMemory::new();
        assert_eq!(wm.focus, FocusTarget::Idle);

        wm.set_focus(FocusTarget::Goal("g1".into()));
        assert_eq!(wm.focus, FocusTarget::Goal("g1".into()));

        wm.set_focus(FocusTarget::Executing("build".into()));
        assert_eq!(wm.focus, FocusTarget::Executing("build".into()));

        wm.set_focus(FocusTarget::Idle);
        assert_eq!(wm.focus, FocusTarget::Idle);
    }

    // ── Cognitive Episode ───────────────────────────────────────

    #[test]
    fn episode_begin() {
        let goal = Goal::new("build project");
        let ep = CognitiveEpisode::begin(goal.clone());
        assert_eq!(ep.goal.description, "build project");
        assert_eq!(ep.outcome, EpisodeOutcome::InProgress);
        assert!(!ep.is_complete());
        assert!(ep.world_before.is_none());
        assert!(ep.plan.is_none());
    }

    #[test]
    fn episode_lifecycle() {
        let goal = Goal::new("test");
        let action = Action::single("echo", Capability::ExecuteCommand, &goal.id);
        let plan = Plan::from_action(action, "test");

        let ep = CognitiveEpisode::begin(goal)
            .with_world_before(WorldState::new())
            .with_plan(plan)
            .with_execution_result(ToolResult::success("ok"))
            .with_verification(VerifiedResult {
                verified: true,
                tool_success: true,
                checks: vec![],
                confidence: Confidence::high(),
            })
            .with_reflection("worked as expected")
            .with_world_after(WorldState::new())
            .with_tag("cargo")
            .finish(EpisodeOutcome::Success);

        assert!(ep.is_complete());
        assert!(ep.succeeded());
        assert!(!ep.failed());
        assert!(ep.world_before.is_some());
        assert!(ep.plan.is_some());
        assert!(ep.execution_result.is_some());
        assert!(ep.verification_result.is_some());
        assert_eq!(ep.reflection.as_deref(), Some("worked as expected"));
        assert!(ep.tags.contains(&"cargo".to_string()));
    }

    #[test]
    fn episode_failure() {
        let ep = CognitiveEpisode::begin(Goal::new("failing task")).finish(EpisodeOutcome::Failure);
        assert!(ep.failed());
        assert!(!ep.succeeded());
    }

    #[test]
    fn episode_duration() {
        let ep = CognitiveEpisode::begin(Goal::new("test"));
        // Not finished → duration is 0
        assert_eq!(ep.duration_ms(), 0);
    }

    #[test]
    fn episode_trace_push() {
        let mut ep = CognitiveEpisode::begin(Goal::new("test"));
        ep.push_trace(TraceNode::new(
            TraceNodeKind::Planning,
            serde_json::json!({}),
            serde_json::json!({}),
        ));
        assert_eq!(ep.trace.nodes.len(), 1);
    }

    #[test]
    fn episode_outcome_display() {
        assert_eq!(EpisodeOutcome::Success.to_string(), "success");
        assert_eq!(
            EpisodeOutcome::UnexpectedFailure.to_string(),
            "unexpected_failure"
        );
        assert_eq!(EpisodeOutcome::Aborted.to_string(), "aborted");
        assert_eq!(EpisodeOutcome::InProgress.to_string(), "in_progress");
    }

    // ── Episode Store ───────────────────────────────────────────

    #[test]
    fn episode_store_basic() {
        let mut store = EpisodeStore::new();
        assert_eq!(store.count(), 0);

        store.store(CognitiveEpisode::begin(Goal::new("a")).finish(EpisodeOutcome::Success));
        store.store(CognitiveEpisode::begin(Goal::new("b")).finish(EpisodeOutcome::Failure));
        assert_eq!(store.count(), 2);
    }

    #[test]
    fn episode_store_prunes_at_capacity() {
        let mut store = EpisodeStore::with_capacity(3);
        for i in 0..5 {
            store.store(CognitiveEpisode::begin(Goal::new(format!("g{}", i))));
        }
        assert_eq!(store.count(), 3);
    }

    #[test]
    fn episode_store_by_goal() {
        let mut store = EpisodeStore::new();
        let g1 = Goal::new("a");
        let g1_id = g1.id.clone();
        store.store(CognitiveEpisode::begin(g1));
        store.store(CognitiveEpisode::begin(Goal::new("b")));
        assert_eq!(store.by_goal(&g1_id).len(), 1);
        assert_eq!(store.by_goal("nonexistent").len(), 0);
    }

    #[test]
    fn episode_store_by_outcome() {
        let mut store = EpisodeStore::new();
        store.store(CognitiveEpisode::begin(Goal::new("a")).finish(EpisodeOutcome::Success));
        store.store(CognitiveEpisode::begin(Goal::new("b")).finish(EpisodeOutcome::Failure));
        store.store(CognitiveEpisode::begin(Goal::new("c")).finish(EpisodeOutcome::Success));
        assert_eq!(store.successes().len(), 2);
        assert_eq!(store.failures().len(), 1);
    }

    #[test]
    fn episode_store_by_tag() {
        let mut store = EpisodeStore::new();
        store.store(CognitiveEpisode::begin(Goal::new("a")).with_tag("cargo"));
        store.store(CognitiveEpisode::begin(Goal::new("b")).with_tag("git"));
        store.store(CognitiveEpisode::begin(Goal::new("c")).with_tag("cargo"));
        assert_eq!(store.by_tag("cargo").len(), 2);
    }

    #[test]
    fn episode_store_success_rate() {
        let mut store = EpisodeStore::new();
        store.store(CognitiveEpisode::begin(Goal::new("a")).finish(EpisodeOutcome::Success));
        store.store(CognitiveEpisode::begin(Goal::new("b")).finish(EpisodeOutcome::Success));
        store.store(CognitiveEpisode::begin(Goal::new("c")).finish(EpisodeOutcome::Failure));
        assert!((store.success_rate() - 0.666).abs() < 0.01);
    }

    #[test]
    fn episode_store_empty_success_rate() {
        let store = EpisodeStore::new();
        assert_eq!(store.success_rate(), 0.0);
    }

    #[test]
    fn episode_store_recent() {
        let mut store = EpisodeStore::new();
        for i in 0..5 {
            store.store(CognitiveEpisode::begin(Goal::new(format!("g{}", i))));
        }
        let recent = store.recent(2);
        assert_eq!(recent.len(), 2);
        assert_eq!(recent[0].goal.description, "g3");
        assert_eq!(recent[1].goal.description, "g4");
    }

    #[test]
    fn episode_store_get_by_id() {
        let mut store = EpisodeStore::new();
        let ep = CognitiveEpisode::begin(Goal::new("test"));
        let id = ep.id.clone();
        store.store(ep);
        assert!(store.get(&id).is_some());
        assert!(store.get("nonexistent").is_none());
    }

    #[test]
    fn episode_store_clear() {
        let mut store = EpisodeStore::new();
        store.store(CognitiveEpisode::begin(Goal::new("a")));
        store.clear();
        assert_eq!(store.count(), 0);
    }

    // ── Experience Pattern ──────────────────────────────────────

    #[test]
    fn pattern_basic() {
        let mut p = ExperiencePattern::new(PatternKind::RepeatedSuccess, "cargo builds succeed");
        assert_eq!(p.support, 0);
        assert_eq!(p.confidence, 0.0);
        assert!(!p.promoted);

        p.add_evidence("ep_1");
        assert_eq!(p.support, 1);
        assert!(p.confidence > 0.0);
    }

    #[test]
    fn pattern_confidence_grows_with_support() {
        let mut p = ExperiencePattern::new(PatternKind::RepeatedSuccess, "test");
        p.add_evidence("ep_1");
        let c1 = p.confidence;
        p.add_evidence("ep_2");
        let c2 = p.confidence;
        p.add_evidence("ep_3");
        let c3 = p.confidence;
        assert!(c1 < c2);
        assert!(c2 < c3);
        assert!(c3 < 1.0);
    }

    #[test]
    fn pattern_meets_threshold() {
        let mut p = ExperiencePattern::new(PatternKind::RepeatedSuccess, "test");
        assert!(!p.meets_threshold(3, 0.6));
        p.add_evidence("ep_1");
        p.add_evidence("ep_2");
        p.add_evidence("ep_3");
        // support=3, confidence = 1 - 1/(1+3) = 0.75
        assert!(p.meets_threshold(3, 0.6));
    }

    #[test]
    fn pattern_with_capability() {
        let p = ExperiencePattern::new(PatternKind::RepeatedSuccess, "test")
            .with_capability(Capability::ExecuteCommand);
        assert_eq!(p.capability, Some(Capability::ExecuteCommand));
    }

    #[test]
    fn pattern_kind_display() {
        assert_eq!(PatternKind::RepeatedSuccess.to_string(), "repeated_success");
        assert_eq!(PatternKind::Workflow.to_string(), "workflow");
        assert_eq!(PatternKind::RiskObservation.to_string(), "risk_observation");
    }

    // ── Semantic Memory ─────────────────────────────────────────

    #[test]
    fn semantic_memory_processes_success_episodes() {
        let mut store = EpisodeStore::new();

        for _ in 0..4 {
            store.store(
                CognitiveEpisode::begin(Goal::new("cargo build"))
                    .with_plan(Plan::from_action(
                        Action::single("cargo build", Capability::ExecuteCommand, "g"),
                        "build",
                    ))
                    .finish(EpisodeOutcome::Success),
            );
        }

        let mut sem = SemanticMemory::new();
        let result = sem.process_episodes(&store);
        assert_eq!(result.episodes_processed, 4);
        // 4 episodes → support=4, confidence=0.8 → above threshold → promoted
        assert!(
            result.patterns_promoted > 0,
            "should have promoted patterns"
        );
    }

    #[test]
    fn semantic_memory_promotes_strong_patterns() {
        let mut store = EpisodeStore::new();
        // Create 5 identical success episodes (above default threshold of 3)
        for _ in 0..5 {
            store.store(
                CognitiveEpisode::begin(Goal::new("cargo build"))
                    .with_plan(Plan::from_action(
                        Action::single("cargo build", Capability::ExecuteCommand, "g"),
                        "build",
                    ))
                    .finish(EpisodeOutcome::Success),
            );
        }

        let mut sem = SemanticMemory::with_thresholds(PromotionThresholds {
            min_support: 3,
            min_confidence: 0.6,
        });
        let result = sem.process_episodes(&store);
        assert!(
            result.patterns_promoted > 0,
            "should promote patterns with 5 episodes"
        );
        assert!(result.policies_proposed > 0, "should propose policies");
    }

    #[test]
    fn semantic_memory_deduplicates() {
        let mut store = EpisodeStore::new();
        store.store(CognitiveEpisode::begin(Goal::new("test")).finish(EpisodeOutcome::Success));

        let mut sem = SemanticMemory::new();
        let r1 = sem.process_episodes(&store);
        assert_eq!(r1.episodes_processed, 1);

        // Process again — should skip the same episode.
        let r2 = sem.process_episodes(&store);
        assert_eq!(r2.episodes_processed, 0);
    }

    #[test]
    fn semantic_memory_queries() {
        let mut store = EpisodeStore::new();
        for _ in 0..4 {
            store.store(
                CognitiveEpisode::begin(Goal::new("cargo build"))
                    .with_plan(Plan::from_action(
                        Action::single("cargo build", Capability::ExecuteCommand, "g"),
                        "build",
                    ))
                    .finish(EpisodeOutcome::Success),
            );
        }

        let mut sem = SemanticMemory::new();
        sem.process_episodes(&store);

        let cap_patterns = sem.patterns_for_capability(&Capability::ExecuteCommand);
        assert!(!cap_patterns.is_empty());
    }

    #[test]
    fn semantic_memory_failure_pattern() {
        let mut store = EpisodeStore::new();
        for _ in 0..4 {
            store.store(
                CognitiveEpisode::begin(Goal::new("read broken file"))
                    .with_plan(Plan::from_action(
                        Action::single("read file", Capability::ReadFile, "g"),
                        "read",
                    ))
                    .finish(EpisodeOutcome::Failure),
            );
        }

        let mut sem = SemanticMemory::new();
        sem.process_episodes(&store);

        let failure_patterns: Vec<_> = sem
            .patterns()
            .iter()
            .filter(|p| p.kind == PatternKind::RepeatedFailure)
            .collect();
        assert!(!failure_patterns.is_empty());
    }

    #[test]
    fn semantic_memory_workflow_pattern() {
        let mut store = EpisodeStore::new();
        for _ in 0..4 {
            let goal = Goal::new("build and test");
            let a1 = Action::single("cargo build", Capability::ExecuteCommand, &goal.id);
            let a2 = Action::single("cargo test", Capability::ExecuteCommand, &goal.id);
            let plan = Plan::from_actions(vec![a1, a2], "build-test", PlanSource::Direct);
            store.store(
                CognitiveEpisode::begin(goal)
                    .with_plan(plan)
                    .finish(EpisodeOutcome::Success),
            );
        }

        let mut sem = SemanticMemory::new();
        sem.process_episodes(&store);

        let wf_patterns: Vec<_> = sem
            .patterns()
            .iter()
            .filter(|p| p.kind == PatternKind::Workflow)
            .collect();
        assert!(!wf_patterns.is_empty());
    }

    // ── Attention Manager ───────────────────────────────────────

    #[test]
    fn attention_manager_add_and_count() {
        let mut mgr = AttentionManager::new();
        mgr.add(AttentionItem::new(
            AttentionItemKind::Goal,
            "build project",
            AttentionScore::builder().importance(0.8).build(),
        ));
        assert_eq!(mgr.count(), 1);
    }

    #[test]
    fn attention_manager_ranked_order() {
        let mut mgr = AttentionManager::new();
        mgr.add(AttentionItem::new(
            AttentionItemKind::Goal,
            "low priority",
            AttentionScore::builder().importance(0.4).build(),
        ));
        mgr.add(AttentionItem::new(
            AttentionItemKind::Goal,
            "high priority",
            AttentionScore::builder().importance(0.9).build(),
        ));
        mgr.add(AttentionItem::new(
            AttentionItemKind::Goal,
            "medium priority",
            AttentionScore::builder().importance(0.6).build(),
        ));

        let ranked = mgr.ranked();
        assert_eq!(ranked.len(), 3);
        assert_eq!(ranked[0].description, "high priority");
        assert_eq!(ranked[1].description, "medium priority");
        assert_eq!(ranked[2].description, "low priority");
    }

    #[test]
    fn attention_manager_focus_removes_item() {
        let mut mgr = AttentionManager::new();
        let item = AttentionItem::new(
            AttentionItemKind::Goal,
            "urgent task",
            AttentionScore::builder()
                .importance(0.9)
                .urgency(0.8)
                .build(),
        );
        mgr.add(item);

        let focused = mgr.focus();
        assert!(focused.is_some());
        assert_eq!(focused.unwrap().description, "urgent task");
        assert_eq!(mgr.count(), 0);
    }

    #[test]
    fn attention_manager_focus_tracks_count() {
        let mut mgr = AttentionManager::new();
        mgr.add(AttentionItem::new(
            AttentionItemKind::Goal,
            "task",
            AttentionScore::builder().importance(0.9).build(),
        ));

        let item = mgr.focus().unwrap();
        assert_eq!(item.focus_count, 1);
        assert!(mgr.was_recently_focused(&item.id));
    }

    #[test]
    fn attention_manager_deprioritize() {
        let mut mgr = AttentionManager::new();
        let high = AttentionItem::new(
            AttentionItemKind::Goal,
            "deprioritized",
            AttentionScore::builder().importance(0.9).build(),
        );
        let low = AttentionItem::new(
            AttentionItemKind::Goal,
            "normal",
            AttentionScore::builder().importance(0.5).build(),
        );
        let high_id = high.id.clone();
        mgr.add(high);
        mgr.add(low);

        mgr.deprioritize(&high_id);

        let ranked = mgr.ranked();
        // Normal (0.5 * 0.3 = 0.15) should rank higher than deprioritized (0.9 * 0.3 * 0.3 = 0.081)
        assert_eq!(ranked[0].description, "normal");
    }

    #[test]
    fn attention_manager_reprioritize() {
        let mut mgr = AttentionManager::new();
        let item = AttentionItem::new(
            AttentionItemKind::Goal,
            "task",
            AttentionScore::builder().importance(0.9).build(),
        );
        let id = item.id.clone();
        mgr.add(item);

        mgr.deprioritize(&id);
        assert!(mgr.ranked().is_empty() || mgr.ranked()[0].deprioritized);

        mgr.reprioritize(&id);
        assert_eq!(mgr.ranked().len(), 1);
        assert!(!mgr.ranked()[0].deprioritized);
    }

    #[test]
    fn attention_manager_remove() {
        let mut mgr = AttentionManager::new();
        let item = AttentionItem::new(
            AttentionItemKind::Goal,
            "removable",
            AttentionScore::builder().importance(0.5).build(),
        );
        let id = item.id.clone();
        mgr.add(item);
        assert!(mgr.remove(&id));
        assert_eq!(mgr.count(), 0);
        assert!(!mgr.remove("nonexistent"));
    }

    #[test]
    fn attention_manager_rescore() {
        let mut mgr = AttentionManager::new();
        let item = AttentionItem::new(
            AttentionItemKind::Goal,
            "rescored",
            AttentionScore::builder().importance(0.3).build(),
        );
        let id = item.id.clone();
        mgr.add(item);

        let new_score = AttentionScore::builder().importance(0.9).build();
        assert!(mgr.rescore(&id, new_score));
        let ranked = mgr.ranked();
        assert!((ranked[0].score.importance - 0.9).abs() < 0.001);
    }

    #[test]
    fn attention_manager_by_kind() {
        let mut mgr = AttentionManager::new();
        mgr.add(AttentionItem::new(
            AttentionItemKind::Goal,
            "g1",
            AttentionScore::builder().importance(0.5).build(),
        ));
        mgr.add(AttentionItem::new(
            AttentionItemKind::Belief,
            "b1",
            AttentionScore::builder().importance(0.5).build(),
        ));
        mgr.add(AttentionItem::new(
            AttentionItemKind::Goal,
            "g2",
            AttentionScore::builder().importance(0.5).build(),
        ));

        assert_eq!(mgr.by_kind(&AttentionItemKind::Goal).len(), 2);
        assert_eq!(mgr.by_kind(&AttentionItemKind::Belief).len(), 1);
    }

    #[test]
    fn attention_manager_clear() {
        let mut mgr = AttentionManager::new();
        mgr.add(AttentionItem::new(
            AttentionItemKind::Goal,
            "g1",
            AttentionScore::zero(),
        ));
        mgr.add(AttentionItem::new(
            AttentionItemKind::Belief,
            "b1",
            AttentionScore::zero(),
        ));
        mgr.clear();
        assert_eq!(mgr.count(), 0);
    }

    #[test]
    fn attention_manager_peek() {
        let mut mgr = AttentionManager::new();
        mgr.add(AttentionItem::new(
            AttentionItemKind::Goal,
            "visible",
            AttentionScore::builder().importance(0.9).build(),
        ));
        let peeked = mgr.peek();
        assert!(peeked.is_some());
        assert_eq!(peeked.unwrap().description, "visible");
        // Peek doesn't remove
        assert_eq!(mgr.count(), 1);
    }

    #[test]
    fn attention_manager_min_focus_score_filters() {
        let config = AttentionConfig {
            min_focus_score: 0.5,
            ..Default::default()
        };
        let mut mgr = AttentionManager::with_config(config);
        mgr.add(AttentionItem::new(
            AttentionItemKind::Goal,
            "below threshold",
            AttentionScore::builder().importance(0.1).build(), // aggregate = 0.03
        ));

        let ranked = mgr.ranked();
        assert!(
            ranked.is_empty(),
            "items below min_focus_score should be filtered"
        );

        let focused = mgr.focus();
        assert!(
            focused.is_none(),
            "focus should return None for below-threshold items"
        );
    }

    #[test]
    fn attention_item_effective_score_deprioritized() {
        let item = AttentionItem {
            score: AttentionScore::builder().importance(0.8).build(),
            deprioritized: true,
            ..AttentionItem::new(AttentionItemKind::Goal, "test", AttentionScore::zero())
        };
        let base = AttentionScore::builder()
            .importance(0.8)
            .build()
            .aggregate();
        assert!((item.effective_score() - base * 0.3).abs() < 0.001);
    }

    #[test]
    fn attention_item_kind_display() {
        assert_eq!(AttentionItemKind::Goal.to_string(), "goal");
        assert_eq!(AttentionItemKind::Belief.to_string(), "belief");
        assert_eq!(AttentionItemKind::Custom("test".into()).to_string(), "test");
    }

    // ── Global Workspace ────────────────────────────────────────

    #[test]
    fn workspace_create() {
        let ws = CognitiveBlackboard::new();
        assert!(ws.get_active_goal().is_none());
        assert!(ws.get_active_plan().is_none());
        assert!(ws.get_attention().is_empty());
        assert!(ws.get_beliefs().is_empty());
    }

    #[test]
    fn workspace_set_active_goal() {
        let ws = CognitiveBlackboard::new();
        ws.set_active_goal(Goal::new("test goal"));
        let goal = ws.get_active_goal();
        assert_eq!(goal.unwrap().description, "test goal");
    }

    #[test]
    fn workspace_clear_active_goal() {
        let ws = CognitiveBlackboard::new();
        ws.set_active_goal(Goal::new("test"));
        assert!(ws.get_active_goal().is_some());
        ws.clear_active_goal();
        assert!(ws.get_active_goal().is_none());
    }

    #[test]
    fn workspace_set_active_plan() {
        let ws = CognitiveBlackboard::new();
        let plan = Plan::from_action(
            Action::single("echo", Capability::ExecuteCommand, "g"),
            "test",
        );
        ws.set_active_plan(plan);
        let p = ws.get_active_plan();
        assert_eq!(p.unwrap().strategy, "test");
    }

    #[test]
    fn workspace_attention() {
        let ws = CognitiveBlackboard::new();
        let items = vec![
            AttentionItem::new(
                AttentionItemKind::Goal,
                "urgent",
                AttentionScore::builder().importance(0.9).build(),
            ),
            AttentionItem::new(
                AttentionItemKind::Belief,
                "stale data",
                AttentionScore::builder().importance(0.3).build(),
            ),
        ];
        ws.set_attention(items);
        assert_eq!(ws.get_attention().len(), 2);
    }

    #[test]
    fn workspace_beliefs() {
        let ws = CognitiveBlackboard::new();
        ws.add_belief(Belief::new(
            "server is available",
            Confidence::high(),
            "ping",
        ));
        ws.add_belief(Belief::new("disk is full", Confidence::low(), "df"));
        assert_eq!(ws.get_beliefs().len(), 2);
    }

    #[test]
    fn workspace_world_state() {
        let ws = CognitiveBlackboard::new();
        let state = WorldState::new().with_tool(Capability::ExecuteCommand);
        ws.set_world_state(state);
        let s = ws.get_world_state();
        assert!(s.is_some());
        assert!(s
            .unwrap()
            .available_tools
            .contains(&Capability::ExecuteCommand));
    }

    #[test]
    fn workspace_thoughts() {
        let ws = CognitiveBlackboard::new();
        ws.push_thought("plan the approach");
        ws.push_thought("simulate the plan");
        ws.push_thought("execute step 1");
        assert_eq!(ws.get_thoughts().len(), 3);
    }

    #[test]
    fn workspace_thoughts_ring_buffer() {
        let ws = CognitiveBlackboard::new();
        for i in 0..60 {
            ws.push_thought(format!("thought {}", i));
        }
        assert_eq!(ws.get_thoughts().len(), 50);
    }

    #[test]
    fn workspace_predictions() {
        let ws = CognitiveBlackboard::new();
        let sim = DefaultSimulator::new();
        let world = WorldState::new().with_tool(Capability::ExecuteCommand);
        let goal = Goal::new("test");
        let action = Action::single("echo", Capability::ExecuteCommand, &goal.id);
        let plan = Plan::from_action(action, "test");
        let result = sim.simulate(&plan, &world);

        ws.set_predictions(vec![result]);
        assert_eq!(ws.get_predictions().len(), 1);
    }

    #[test]
    fn workspace_hypotheses() {
        let ws = CognitiveBlackboard::new();
        let h = ExperiencePattern::new(PatternKind::RepeatedSuccess, "test pattern");
        ws.set_hypotheses(vec![h]);
        assert_eq!(ws.get_hypotheses().len(), 1);
    }

    #[test]
    fn workspace_execution() {
        let ws = CognitiveBlackboard::new();
        ws.set_execution(ToolResult::success("ok"));
        let r = ws.get_execution();
        assert!(r.unwrap().success);

        ws.clear_execution();
        assert!(ws.get_execution().is_none());
    }

    #[test]
    fn workspace_reflection() {
        let ws = CognitiveBlackboard::new();
        ws.set_reflection("goal completed successfully");
        assert_eq!(
            ws.get_reflection().as_deref(),
            Some("goal completed successfully")
        );
    }

    #[test]
    fn workspace_current_episode() {
        let ws = CognitiveBlackboard::new();
        let ep = CognitiveEpisode::begin(Goal::new("test")).finish(EpisodeOutcome::Success);
        ws.set_current_episode(ep);
        let e = ws.get_current_episode();
        assert!(e.unwrap().succeeded());

        ws.clear_current_episode();
        assert!(ws.get_current_episode().is_none());
    }

    #[test]
    fn workspace_current_trace() {
        let ws = CognitiveBlackboard::new();
        let mut trace = CognitiveTrace::new("g1");
        trace.push(TraceNode::new(
            TraceNodeKind::Planning,
            serde_json::json!({}),
            serde_json::json!({}),
        ));
        ws.set_current_trace(trace);
        let t = ws.get_current_trace();
        assert_eq!(t.unwrap().nodes.len(), 1);
    }

    #[test]
    fn workspace_snapshot() {
        let ws = CognitiveBlackboard::new();
        ws.set_active_goal(Goal::new("snapshot test"));
        ws.push_thought("thinking");
        ws.add_belief(Belief::new("test", Confidence::medium(), "test"));

        let snap = ws.snapshot();
        assert_eq!(snap.active_goal.unwrap().description, "snapshot test");
        assert_eq!(snap.thought_count, 1);
        assert_eq!(snap.belief_count, 1);
    }

    #[tokio::test]
    async fn workspace_change_notification() {
        let ws = CognitiveBlackboard::new();
        let mut rx = ws.subscribe();

        ws.set_active_goal(Goal::new("notify test"));

        let change = tokio::time::timeout(std::time::Duration::from_secs(1), rx.recv()).await;

        assert!(change.is_ok(), "should receive a change notification");
        let change = change.unwrap().unwrap();
        assert_eq!(change.slot, "active_goal");
    }

    #[test]
    fn workspace_clear_all() {
        let ws = CognitiveBlackboard::new();
        ws.set_active_goal(Goal::new("test"));
        ws.add_belief(Belief::new("x", Confidence::medium(), "test"));
        ws.set_attention(vec![AttentionItem::new(
            AttentionItemKind::Goal,
            "g",
            AttentionScore::zero(),
        )]);
        ws.push_thought("think");
        ws.set_execution(ToolResult::success("ok"));

        ws.clear();
        assert!(ws.get_active_goal().is_none());
        assert!(ws.get_beliefs().is_empty());
        assert!(ws.get_attention().is_empty());
        assert!(ws.get_thoughts().is_empty());
        assert!(ws.get_execution().is_none());
    }

    #[test]
    fn workspace_change_struct() {
        let c = WorkspaceChange::new("beliefs", "updated core beliefs");
        assert_eq!(c.slot, "beliefs");
        assert_eq!(c.description, "updated core beliefs");
        assert!(c.timestamp_ms > 0);
    }

    // ── Executive ───────────────────────────────────────────────

    #[test]
    fn executive_creates_default() {
        let exec = Executive::new();
        assert_eq!(exec.phase(), ExecutivePhase::Idle);
        assert_eq!(exec.cycle_count(), 0);
    }

    #[test]
    fn executive_cycle_without_focus_idles() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let workspace = CognitiveBlackboard::new();

        rt.block_on(async {
            let mut exec = Executive::new();
            let report = exec.cycle(&workspace).await;
            assert!(report.phases_completed.contains(&ExecutivePhase::Idle));
            assert!(!report.goal_created);
            assert!(!report.plan_executed);
        });
    }

    #[tokio::test]
    async fn executive_cycle_with_goal_executes() {
        let workspace = CognitiveBlackboard::new();

        // Set up world state with the tool capability so simulator finds it feasible.
        let mut world = WorldState::default();
        world.available_tools.push(Capability::ExecuteCommand);
        workspace.set_world_state(world);

        let mut exec = Executive::new();
        exec.register_tool(Box::new(MockTool::success("cargo", "build succeeded")));
        exec.add_goal("cargo build", 0.9);

        let report = exec.cycle(&workspace).await;
        assert!(
            report.phases_completed.len() >= 7,
            "should complete at least 7 phases, got {}: {:?}",
            report.phases_completed.len(),
            report.phases_completed
        );
        assert!(report.plan_executed);
        assert!(report.episode_stored);

        let ep = workspace.get_current_episode();
        assert!(ep.is_some());
        assert!(ep.unwrap().is_complete());

        let thoughts = workspace.get_thoughts();
        assert!(!thoughts.is_empty());
    }

    #[tokio::test]
    async fn executive_multiple_cycles() {
        let workspace = CognitiveBlackboard::new();

        let mut world = WorldState::default();
        world.available_tools.push(Capability::ExecuteCommand);
        workspace.set_world_state(world);

        let mut exec = Executive::new();
        exec.register_tool(Box::new(MockTool::success("echo", "ok")));

        exec.add_goal("first task", 0.8);
        let r1 = exec.cycle(&workspace).await;
        assert!(r1.plan_executed);

        let _r2 = exec.cycle(&workspace).await;
        assert_eq!(exec.cycle_count(), 2);
        assert!(exec.episode_store().count() >= 1);
    }

    #[test]
    fn executive_add_goal_and_observation() {
        let mut exec = Executive::new();
        exec.add_goal("important task", 0.9);
        exec.add_observation("something changed", 0.6);
        assert_eq!(exec.attention_manager().count(), 2);
    }

    #[test]
    fn executive_phase_display() {
        assert_eq!(ExecutivePhase::Observe.to_string(), "observe");
        assert_eq!(ExecutivePhase::Attend.to_string(), "attend");
        assert_eq!(ExecutivePhase::Investigate.to_string(), "investigate");
        assert_eq!(ExecutivePhase::Plan.to_string(), "plan");
        assert_eq!(ExecutivePhase::Simulate.to_string(), "simulate");
        assert_eq!(ExecutivePhase::Execute.to_string(), "execute");
        assert_eq!(ExecutivePhase::Verify.to_string(), "verify");
        assert_eq!(ExecutivePhase::Reflect.to_string(), "reflect");
        assert_eq!(ExecutivePhase::Consolidate.to_string(), "consolidate");
        assert_eq!(ExecutivePhase::Learn.to_string(), "learn");
        assert_eq!(ExecutivePhase::Idle.to_string(), "idle");
    }
}
