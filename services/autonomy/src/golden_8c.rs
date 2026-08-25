//! 8C final gate — golden integration test.
//!
//! Proves the frozen 7D [`SkillStore`] contract is satisfied by the real
//! `friday-skills` engine through a [`ProductionSkillStore`]:
//!
//! ```text
//! 7D Skill
//!    └── ProductionSkillStore
//!          ├── SkillCompiler::compile   (refusal gate: unverified → refused)
//!          ├── SkillRegistry            (SkillSource::Learned candidate)
//!          ├── SkillRegistry::enable_learned  (explicit admission)
//!          └── SkillProvider::applicable_skills  (planner consultation surface)
//! ```
//!
//! The golden cases (so the test cannot prove only the happy path):
//!   * A verified, corroborated skill is compounded into the production registry,
//!     admitted, and retrievable via `find_applicable` / `applicable_skills`.
//!   * The critical asymmetry holds at the seam: an *unverified* skill (no
//!     verified trajectory, or one carrying a contradictory failure) is refused
//!     by the compiler and never registered — a failure is learned as a lesson,
//!     never compounded into a skill.
//!   * `strengthen` / `weaken` / `disable` are reflected in both the autonomy
//!     metadata and the production registry's enabled state.

use crate::memory::LessonCondition;
use crate::skill::{ApplicableSkill, Skill, SkillProvider, SkillStatus, SkillStore};
use crate::skill_production::ProductionSkillStore;

use friday_skills::registry::SkillSource;

fn trigger() -> LessonCondition {
    LessonCondition {
        action: "appX".into(),
        signals: vec!["conditionY".into()],
    }
}

/// A skill that already cleared the autonomy `SkillEvaluator` gate:
/// support ≥ 3, verifications ≥ 2, no contradictory failures, risk ≠ High.
fn promoted_skill() -> Skill {
    let t = trigger();
    Skill {
        id: format!("skill:{}:{}", t.action, t.signals.join("|")),
        trigger: t,
        trajectory: vec!["fallbackZ".into()],
        confidence: 0.8,
        support_count: 3,
        verification_count: 2,
        failure_count: 0,
        created_at_ms: 1,
        updated_at_ms: 2,
        status: SkillStatus::Active,
    }
}

#[test]
fn golden_8c_verified_skill_compounded_and_admitted() {
    let store = ProductionSkillStore::new();
    store.admit(promoted_skill());

    // (1) The production registry received exactly one compiled candidate.
    let reg = store.registry();
    assert_eq!(reg.all().len(), 1, "compiled candidate registered");
    let prod = reg.all().into_iter().next().unwrap();
    assert_eq!(prod.source, SkillSource::Learned, "compounded from a trajectory");
    assert!(prod.enabled, "explicitly admitted via enable_learned");

    // (2) find_applicable returns the faithful 7D skill.
    let found = store.find_applicable(&trigger()).expect("retrievable by trigger");
    assert_eq!(found.status, SkillStatus::Active);
    assert_eq!(found.trajectory, vec!["fallbackZ".to_string()]);

    // (3) The planner consultation surface exposes it (never auto-executes).
    let ctx = crate::skill::SkillContext {
        action: "appX".into(),
        signals: vec!["conditionY".into()],
    };
    let surfaced: Vec<ApplicableSkill> = store.applicable_skills(&ctx);
    assert_eq!(surfaced.len(), 1, "skill is surfaced for the planner");
    assert_eq!(surfaced[0].trajectory, vec!["fallbackZ".to_string()]);
}

#[test]
fn golden_8c_unverified_skill_is_refused_by_the_seam() {
    let store = ProductionSkillStore::new();

    // No verified trajectory → mapped to OutcomeSummary::Unverified → refused.
    let mut unverified = promoted_skill();
    unverified.verification_count = 0;
    store.admit(unverified);
    assert_eq!(store.skill_count(), 0, "unverified skill must NOT be compounded");
    assert!(
        store.find_applicable(&trigger()).is_none(),
        "refused candidate is never selectable"
    );

    // A contradictory (verified-failure) trajectory → also refused.
    let mut contradictory = promoted_skill();
    contradictory.verification_count = 1;
    contradictory.failure_count = 1;
    store.admit(contradictory);
    assert_eq!(
        store.skill_count(),
        0,
        "skill carrying a contradictory failure must NOT be compounded"
    );
}

#[test]
fn golden_8c_dedup_strengthens_instead_of_duplicating() {
    let store = ProductionSkillStore::new();
    store.admit(promoted_skill());
    store.admit(promoted_skill()); // same trigger → strengthen, not a 2nd candidate

    assert_eq!(store.skill_count(), 1, "one compiled candidate per trigger");
    let found = store.find_applicable(&trigger()).unwrap();
    assert_eq!(found.support_count, 4, "second admit strengthened (3 + 1)");

    store.strengthen(&trigger());
    let found = store.find_applicable(&trigger()).unwrap();
    assert_eq!(found.support_count, 5, "strengthen bumps corroboration");
}

#[test]
fn golden_8c_weaken_decays_then_disables_in_both_layers() {
    let store = ProductionSkillStore::new();
    store.admit(promoted_skill());
    let trig = trigger();
    let id = store.registry().all().into_iter().next().unwrap().id().to_string();

    // verification_count = 2: failures 1, 2 still ≤ successes → still Active.
    store.weaken(&trig);
    assert!(
        store.find_applicable(&trig).is_some(),
        "still active after 1 contradictory failure"
    );
    store.weaken(&trig);
    assert!(
        store.find_applicable(&trig).is_some(),
        "still active after 2 contradictory failures"
    );

    // 3rd failure > 2 successes → Disabled in the autonomy layer AND the
    // production registry's enabled flag.
    store.weaken(&trig);
    assert!(
        store.find_applicable(&trig).is_none(),
        "obsolete skill disabled after 3 failures"
    );
    assert!(
        !store.registry().get(&id).unwrap().enabled,
        "production registry reflects the disable"
    );
}

#[test]
fn golden_8c_disable_removes_from_both_layers() {
    let store = ProductionSkillStore::new();
    store.admit(promoted_skill());
    let id = store.registry().all().into_iter().next().unwrap().id().to_string();

    store.disable(&trigger());
    assert!(store.find_applicable(&trigger()).is_none());
    assert!(!store.registry().get(&id).unwrap().enabled);
}
