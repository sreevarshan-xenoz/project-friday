//! Skill learning acceptance (S3) — the runtime compounding seam, end-to-end:
//!
//! ```text
//! verified task (real CodeTestVerifier over a real command run)
//!   → trajectory
//!   → SkillLearningService::learn    (candidate, disabled, not selectable)
//!   → SkillLearningService::admit    (qualification → explicit admission)
//!   → planner discovery (select_for finds it)
//!   → second execution reuses the same verified capability
//! ```
//!
//! This runs against the REAL wiring (`load_skill_engine` over the bundled
//! skills + the native capability registry) — the same registry the planner
//! selects from — and the REAL goal-execution verifier. No mocks.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use friday_context::{load_skill_engine, LearnError, SkillLearningService, SkillSource};
use friday_goal_execution::{run_command_in, CodeTestVerifier};
use friday_planner::execution::capability_registry::CapabilityRegistry;
use friday_skills::{OutcomeSummary, TrajectorySkill, TrajectoryStep};

fn bundled_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../skills/bundled")
}

fn planner_registry() -> CapabilityRegistry {
    let reg = CapabilityRegistry::new();
    friday_planner::execution::native_caps::register_native_builtins(&reg);
    reg
}

fn learning(reg: &Arc<CapabilityRegistry>) -> SkillLearningService {
    let wiring = load_skill_engine(Arc::clone(reg), &[(bundled_dir(), SkillSource::Builtin)]);
    wiring.skill_learning()
}

/// A trajectory whose steps use REAL canonical capability ids (`terminal.exec`,
/// `filesystem.read_file`) so the seam's honest capability gate passes.
fn verified_trajectory(goal: &str, verified: bool, failure: &str) -> TrajectorySkill {
    TrajectorySkill {
        goal: goal.to_string(),
        outcome: if verified {
            OutcomeSummary::Verified {
                evidence_ids: vec!["ev_verify_1".to_string()],
            }
        } else {
            OutcomeSummary::Unverified {
                reason: failure.to_string(),
            }
        },
        steps: vec![
            TrajectoryStep {
                instruction: "run the friday test suite".to_string(),
                capability: "terminal.exec".to_string(),
            },
            TrajectoryStep {
                instruction: "check for the test result marker".to_string(),
                capability: "filesystem.read_file".to_string(),
            },
        ],
        confidence: 0.95,
        support: 1,
    }
}

/// The acceptance: ONE real verified execution becomes a learned candidate,
/// explicit admission makes it discoverable by the planner, and a SECOND real
/// execution of the same capability still verifies.
#[test]
fn verified_experience_learns_admits_and_reuses() {
    let reg = Arc::new(planner_registry());
    let service = learning(&reg);
    let registry = service.registry_handle();

    // 1. Real verification of the FIRST execution (exit code + markers).
    let verifier = CodeTestVerifier::for_test_run();
    let obs = run_command_in(std::env::temp_dir().as_path(), "echo test result: ok");
    let verified = verifier.verify_observation(&obs);
    assert!(verified.verified, "first run must verify: {:?}", verified.checks);

    // 2. learn → a CANDIDATE: registered, disabled, invisible to the planner.
    let candidate = service
        .learn(&verified_trajectory(
            "run the friday tests and check the result",
            true,
            "",
        ))
        .expect("learned");
    assert!(!candidate.skill.enabled, "a candidate is never immediately live");
    assert_eq!(
        candidate.skill.resolved_capabilities(),
        vec!["terminal.exec", "filesystem.read_file"],
        "honest resolution against the real registry"
    );
    let skill_id = candidate.skill.id().to_string();
    assert_eq!(skill_id, "run-the-friday-tests-and-check-the-result");
    // A candidate is invisible to the planner until admitted: whatever the
    // bundled skills would have chosen, it is NOT the learned candidate.
    let chosen_before = registry.select_for("run the friday tests and check the result", 8);
    assert!(
        chosen_before.as_ref().map(|c| c.skill.id()) != Some(skill_id.as_str()),
        "the candidate must not be selected before admission"
    );
    // Explainability: the candidate still *ranks* as a disabled skill.
    let disabled = registry.search_disabled("run the friday tests and check the result", 8);
    assert!(
        disabled.iter().any(|r| r.skill.id() == skill_id),
        "a disabled candidate ranks for the goal it came from: {disabled:?}"
    );
    assert_eq!(candidate.provenance.evidence_ids, vec!["ev_verify_1"]);

    // 3. Explicit admission (qualification gate) → enabled + discoverable.
    let report = service.admit(&skill_id).expect("admission ran");
    assert!(report.passed, "admission qualification: {:?}", report.checks);
    assert!(registry.get(&skill_id).unwrap().enabled);
    assert!(registry.get(&skill_id).unwrap().is_usable());

    // 3b. Planner discovery: a LATER goal finds the learned skill.
    let selected = registry
        .select_for("run the friday tests and check the result", 8)
        .expect("the planner discovers the admitted skill");
    assert_eq!(selected.skill.id(), skill_id);

    // 4. SECOND execution: the same real capability re-verifies — the reuse
    //    loop is sound, not just selectable-on-paper.
    let obs2 = run_command_in(std::env::temp_dir().as_path(), "echo test result: ok");
    let verified2 = verifier.verify_observation(&obs2);
    assert!(verified2.verified, "reuse still verifies: {:?}", verified2.checks);
}

/// The verification-first rule at the seam: an UNVERIFIED trajectory is
/// refused by the compiler gate — a failure is never learned.
#[test]
fn unverified_trajectory_is_refused_and_nothing_is_registered() {
    let reg = Arc::new(planner_registry());
    let service = learning(&reg);

    // The real verifier says FAILED.
    let verifier = CodeTestVerifier::for_test_run();
    let obs = run_command_in(std::env::temp_dir().as_path(), "echo test result: FAILED");
    assert!(!verifier.verify_observation(&obs).verified);

    let err = service
        .learn(&verified_trajectory("deploy to production", false, "test result: FAILED"))
        .unwrap_err();
    assert!(
        matches!(&err, LearnError::Refused { .. }),
        "expected Refused, got {err}"
    );
    assert!(
        service
            .registry_handle()
            .get("deploy-to-production")
            .is_none(),
        "nothing from a failed run may register"
    );
}

/// The honesty gate: a trajectory whose only capability CANNOT resolve against
/// the live planner registry is refused before it registers — the compiler
/// must not mark every used capability Resolved on the trajectory's say-so.
#[test]
fn trajectory_with_no_resolvable_capability_is_refused_honestly() {
    let reg = Arc::new(planner_registry());
    let service = learning(&reg);

    // `terminal.execute` / `filesystem.read` are the phantom ids the compiler
    // used to mark Resolved blindly; the native registry binds terminal.exec /
    // filesystem.read_file. The seam must refuse.
    let trajectory = TrajectorySkill {
        goal: "run the friday tests".to_string(),
        outcome: OutcomeSummary::Verified {
            evidence_ids: vec!["ev_1".to_string()],
        },
        steps: vec![TrajectoryStep {
            instruction: "run the friday test suite".to_string(),
            capability: "terminal.execute".to_string(),
        }],
        confidence: 0.9,
        support: 1,
    };
    let err = service.learn(&trajectory).unwrap_err();
    assert!(
        matches!(&err, LearnError::NoUsableCapability { .. }),
        "expected NoUsableCapability, got {err}"
    );
    assert!(
        service.registry_handle().get("run-the-friday-tests").is_none(),
        "a trajectory nothing could execute must not register"
    );
}

/// Admission is scoped to learned skills: a bundled skill or an unknown id
/// cannot be "admitted" into the learned set.
#[test]
fn admit_gates_to_learned_skills_and_known_ids() {
    let reg = Arc::new(planner_registry());
    let service = learning(&reg);
    let registry = service.registry_handle();
    assert!(
        registry.get("git").is_some(),
        "bundled git should be loaded"
    );
    let git_before = registry.get("git").unwrap().enabled;

    // A bundled (non-Learned) skill → admission refuses (NotLearned).
    let err = service.admit("git").unwrap_err();
    assert!(
        matches!(&err, LearnError::Admission { .. }),
        "expected Admission, got {err}"
    );
    // The bundled skill's state is untouched — admission never ran.
    assert_eq!(
        registry.get("git").unwrap().enabled,
        git_before,
        "admission must not mutate a non-Learned skill"
    );

    // An unknown id → qualification fails; admission refuses with an
    // inspectable report and enables nothing.
    let report = service.admit("nope").expect("admission report");
    assert!(!report.passed, "unknown id must not qualify: {report:?}");
    assert!(registry.get("nope").is_none());
}
