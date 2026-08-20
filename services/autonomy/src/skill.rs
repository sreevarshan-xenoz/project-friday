//! 7D skill compounding — hermetic, strict lesson -> skill promotion.
//!
//! A Lesson (7C) is *knowledge*. A Skill is *executable behavior*. They are not
//! interchangeable, so 7D has a much higher admission bar than 7C: a lesson only
//! becomes a skill when it has corroborated evidence AND a known, verified-successful
//! trajectory. Repetition of a failure proves something is wrong; it does NOT prove
//! a proposed fix is correct — so unverified behavior is rejected.
//!
//! Pipeline (per the 7D design):
//!
//! Lesson -> SkillCandidate -> (evidence + safety) -> Evaluation -> Reject | Promote -> Skill
//!
//! The real FRIDAY skill system implements [SkillStore] later, exactly like the
//! [crate::world::WorldModel] and [crate::memory::MemoryStore] seams. 7D never
//! executes a skill or mutates the planner — it only learns whether a lesson has
//! become a reusable skill and records it.

use crate::memory::{Lesson, LessonCondition};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Mutex;

/// Risk classification of a candidate skill. High risk is never auto-promoted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RiskClass {
    Low,
    Medium,
    High,
}

/// Lifecycle status of an admitted skill. Decay moves Active -> Suspended -> Disabled.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SkillStatus {
    Active,
    Suspended,
    Disabled,
}

/// A proposed skill, before admission. Pure data — no embedded LLM prompt, keeping
/// skills portable (consistent with FRIDAY's skill-as-data philosophy).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SkillCandidate {
    /// Condition that should trigger the skill.
    pub trigger: LessonCondition,
    /// The action sequence that constitutes the skill (portable data).
    pub trajectory: Vec<String>,
    /// The originating lesson this candidate generalizes.
    pub source_lesson: LessonCondition,
    /// Number of supporting experiences (corroboration).
    pub supporting_experiences: u64,
    /// Number of independent verified-successful executions of the trajectory.
    pub verification_evidence: u64,
    /// Number of contradictory verified failures observed.
    pub contradictory_failures: u64,
    pub confidence: f32,
    pub risk: RiskClass,
}

/// An admitted, reusable skill. No LLM prompt — it is data the planner can later
/// consult via [SkillStore::find_applicable].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Skill {
    pub id: String,
    pub trigger: LessonCondition,
    pub trajectory: Vec<String>,
    pub confidence: f32,
    /// Supporting experiences.
    pub support_count: u64,
    /// Verified-successful executions.
    pub verification_count: u64,
    /// Verified-failure executions (used for decay).
    pub failure_count: u64,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
    pub status: SkillStatus,
}

/// The evaluation verdict for a candidate.
#[derive(Debug)]
pub enum SkillAdmission {
    Reject(String),
    Promote(Skill),
}

/// Production seam for skills. SimpleSkillStore is the test/fake; the real
/// friday-skills engine implements this trait later.
pub trait SkillStore: Send + Sync {
    /// Admit a promoted skill. If a skill already exists for the same trigger,
    /// strengthen it instead of creating a duplicate.
    fn admit(&self, skill: Skill);
    /// Find the active skill applicable to a trigger, if any.
    fn find_applicable(&self, trigger: &LessonCondition) -> Option<Skill>;
    /// Strengthen an applicable skill (more corroboration / verified success).
    fn strengthen(&self, trigger: &LessonCondition);
    /// Weaken an applicable skill (contradictory evidence) — may suspend/disable it.
    fn weaken(&self, trigger: &LessonCondition);
    /// Explicitly disable a skill (remove from applicability).
    fn disable(&self, trigger: &LessonCondition);
}

/// Decides whether a candidate becomes a skill. Holds deterministic thresholds and
/// the knowledge of which trajectories are known-good for a condition.
pub struct SkillEvaluator {
    /// A lesson needs at least this much support before it can even form a candidate.
    pub candidate_lesson_support: u64,
    /// Supporting experiences required to promote.
    pub promote_support: u64,
    /// Independent verified-successful executions required to promote.
    pub promote_verifications: u64,
    /// Reject candidates that carry any contradictory verified failure.
    pub reject_on_contradictory: bool,
    /// Known-good trajectories per condition (the verified-successful action sequence).
    pub known_trajectories: HashMap<LessonCondition, Vec<String>>,
}

impl Default for SkillEvaluator {
    fn default() -> Self {
        Self {
            candidate_lesson_support: 3,
            promote_support: 3,
            promote_verifications: 2,
            reject_on_contradictory: true,
            known_trajectories: HashMap::new(),
        }
    }
}

fn now_ms() -> u64 {
    chrono::Utc::now().timestamp_millis() as u64
}

fn skill_id(trigger: &LessonCondition) -> String {
    format!("skill:{}:{}", trigger.action, trigger.signals.join("|"))
}

impl SkillEvaluator {
    pub fn new() -> Self {
        Self::default()
    }

    /// The known-good trajectory for a condition, if one has been verified.
    pub fn trajectory_for(&self, trigger: &LessonCondition) -> Option<Vec<String>> {
        self.known_trajectories.get(trigger).cloned()
    }

    /// Form a candidate from a corroborated lesson + a known trajectory.
    pub fn form_candidate(
        &self,
        lesson: &Lesson,
        trigger: &LessonCondition,
        trajectory: Vec<String>,
    ) -> SkillCandidate {
        // A known (non-empty) trajectory is treated as low risk; an unknown one as
        // high risk, which is never auto-promoted.
        let risk = if trajectory.is_empty() {
            RiskClass::High
        } else {
            RiskClass::Low
        };
        SkillCandidate {
            trigger: trigger.clone(),
            trajectory,
            source_lesson: lesson.condition.clone(),
            supporting_experiences: lesson.support_count,
            verification_evidence: 0,
            contradictory_failures: 0,
            confidence: lesson.confidence,
            risk,
        }
    }

    /// Evaluate a candidate against the deterministic admission policy.
    pub fn evaluate(&self, c: &SkillCandidate) -> SkillAdmission {
        if c.risk == RiskClass::High {
            return SkillAdmission::Reject("high-risk trajectory without approval".into());
        }
        if self.reject_on_contradictory && c.contradictory_failures > 0 {
            return SkillAdmission::Reject("contradictory verified failure".into());
        }
        if c.supporting_experiences < self.promote_support {
            return SkillAdmission::Reject("insufficient corroboration".into());
        }
        if c.verification_evidence < self.promote_verifications {
            return SkillAdmission::Reject("insufficient verified trajectory".into());
        }
        let now = now_ms();
        SkillAdmission::Promote(Skill {
            id: skill_id(&c.trigger),
            trigger: c.trigger.clone(),
            trajectory: c.trajectory.clone(),
            confidence: c.confidence,
            support_count: c.supporting_experiences,
            verification_count: c.verification_evidence,
            failure_count: 0,
            created_at_ms: now,
            updated_at_ms: now,
            status: SkillStatus::Active,
        })
    }
}

/// Hermetic in-memory SkillStore used by 7D tests and as the reference adapter.
pub struct SimpleSkillStore {
    skills: Mutex<HashMap<String, Skill>>,
    by_trigger: Mutex<HashMap<LessonCondition, String>>,
}

impl SimpleSkillStore {
    pub fn new() -> Self {
        Self {
            skills: Mutex::new(HashMap::new()),
            by_trigger: Mutex::new(HashMap::new()),
        }
    }

    /// Total admitted skills (dedup check).
    pub fn skill_count(&self) -> usize {
        self.skills.lock().expect("skill store poisoned").len()
    }

    /// All admitted skills.
    pub fn skills(&self) -> Vec<Skill> {
        self.skills
            .lock()
            .expect("skill store poisoned")
            .values()
            .cloned()
            .collect()
    }
}

impl Default for SimpleSkillStore {
    fn default() -> Self {
        Self::new()
    }
}

impl SkillStore for SimpleSkillStore {
    fn admit(&self, skill: Skill) {
        let mut by = self.by_trigger.lock().expect("skill store poisoned");
        // Duplicate prevention: one skill per trigger. If one exists, strengthen it
        // instead of creating a second skill for the same condition.
        if by.contains_key(&skill.trigger) {
            drop(by);
            self.strengthen(&skill.trigger);
            return;
        }
        by.insert(skill.trigger.clone(), skill.id.clone());
        self.skills
            .lock()
            .expect("skill store poisoned")
            .insert(skill.id.clone(), skill);
    }

    fn find_applicable(&self, trigger: &LessonCondition) -> Option<Skill> {
        let by = self.by_trigger.lock().expect("skill store poisoned");
        let id = by.get(trigger)?;
        let skills = self.skills.lock().expect("skill store poisoned");
        let sk = skills.get(id)?;
        if sk.status == SkillStatus::Active {
            Some(sk.clone())
        } else {
            None
        }
    }

    fn strengthen(&self, trigger: &LessonCondition) {
        let mut skills = self.skills.lock().expect("skill store poisoned");
        if let Some(id) = self.by_trigger.lock().expect("skill store poisoned").get(trigger) {
            if let Some(sk) = skills.get_mut(id) {
                sk.support_count += 1;
                sk.confidence = (sk.confidence + 0.05).min(0.99);
                sk.updated_at_ms = now_ms();
            }
        }
    }

    fn weaken(&self, trigger: &LessonCondition) {
        let mut skills = self.skills.lock().expect("skill store poisoned");
        if let Some(id) = self.by_trigger.lock().expect("skill store poisoned").get(trigger) {
            if let Some(sk) = skills.get_mut(id) {
                sk.failure_count += 1;
                sk.confidence = (sk.confidence - 0.1).max(0.01);
                sk.updated_at_ms = now_ms();
                // Skill decay: once verified failures outnumber verified successes,
                // disable. Before that, a badly-shaken confidence suspends.
                if sk.failure_count > sk.verification_count {
                    sk.status = SkillStatus::Disabled;
                } else if sk.confidence < 0.3 {
                    sk.status = SkillStatus::Suspended;
                }
            }
        }
    }

    fn disable(&self, trigger: &LessonCondition) {
        let mut skills = self.skills.lock().expect("skill store poisoned");
        if let Some(id) = self.by_trigger.lock().expect("skill store poisoned").get(trigger) {
            if let Some(sk) = skills.get_mut(id) {
                sk.status = SkillStatus::Disabled;
            }
        }
        self.by_trigger
            .lock()
            .expect("skill store poisoned")
            .remove(trigger);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn trigger() -> LessonCondition {
        LessonCondition {
            action: "appX".into(),
            signals: vec!["conditionY".to_string()],
        }
    }

    fn corroborated_lesson(support: u64) -> Lesson {
        Lesson {
            condition: trigger(),
            claim: "Tool X fails under condition Y".into(),
            confidence: 0.7,
            support_count: support,
            first_observed_ms: 1,
            last_observed_ms: 2,
        }
    }

    fn candidate(support: u64, verifications: u64, contradictions: u64, risk: RiskClass) -> SkillCandidate {
        SkillCandidate {
            trigger: trigger(),
            trajectory: vec!["fallbackZ".to_string()],
            source_lesson: trigger(),
            supporting_experiences: support,
            verification_evidence: verifications,
            contradictory_failures: contradictions,
            confidence: 0.7,
            risk,
        }
    }

    // 7D.1: verified trajectory + corroboration -> promote
    #[test]
    fn verified_trajectory_promotes_skill() {
        let ev = SkillEvaluator::default();
        let cand = candidate(3, 2, 0, RiskClass::Low);
        match ev.evaluate(&cand) {
            SkillAdmission::Promote(skill) => {
                assert_eq!(skill.status, SkillStatus::Active);
                assert_eq!(skill.verification_count, 2);
                assert_eq!(skill.support_count, 3);
            }
            other => panic!("expected promote, got {:?}", other),
        }
    }

    // 7D.2: unverified action sequence -> reject (no skill)
    #[test]
    fn unverified_execution_rejected() {
        let ev = SkillEvaluator::default();
        let cand = candidate(3, 0, 0, RiskClass::Low); // no verified success
        assert!(matches!(ev.evaluate(&cand), SkillAdmission::Reject(_)));
    }

    // 7D.3: high-risk trajectory -> reject even with evidence
    #[test]
    fn high_risk_rejected_without_approval() {
        let ev = SkillEvaluator::default();
        let cand = candidate(3, 2, 0, RiskClass::High);
        assert!(matches!(ev.evaluate(&cand), SkillAdmission::Reject(_)));
    }

    // 7D.4: contradictory verified failure -> reject
    #[test]
    fn contradictory_evidence_rejected() {
        let ev = SkillEvaluator::default();
        let cand = candidate(3, 2, 1, RiskClass::Low);
        assert!(matches!(ev.evaluate(&cand), SkillAdmission::Reject(_)));
    }

    // 7D.5: insufficient corroboration -> reject
    #[test]
    fn insufficient_corroboration_rejected() {
        let ev = SkillEvaluator::default();
        let cand = candidate(2, 2, 0, RiskClass::Low); // support < 3
        assert!(matches!(ev.evaluate(&cand), SkillAdmission::Reject(_)));
    }

    // 7D.6: strengthening (no duplicate skills)
    #[test]
    fn strengthen_increments_support_no_duplicate() {
        let store = SimpleSkillStore::new();
        let ev = SkillEvaluator::default();
        let cand = candidate(3, 2, 0, RiskClass::Low);
        if let SkillAdmission::Promote(s) = ev.evaluate(&cand) {
            store.admit(s);
        }
        assert_eq!(store.skill_count(), 1);
        store.strengthen(&trigger());
        let skills = store.skills();
        assert_eq!(skills.len(), 1);
        assert_eq!(skills[0].support_count, 4);
    }

    // 7D.7: duplicate trigger -> one skill, strengthened not duplicated
    #[test]
    fn duplicate_trigger_prevents_second_skill() {
        let store = SimpleSkillStore::new();
        let ev = SkillEvaluator::default();
        if let SkillAdmission::Promote(s) = ev.evaluate(&candidate(3, 2, 0, RiskClass::Low)) {
            store.admit(s);
        }
        // Second admit with a different (higher-support) candidate for same trigger.
        let c2 = candidate(5, 3, 0, RiskClass::Low);
        if let SkillAdmission::Promote(s) = ev.evaluate(&c2) {
            store.admit(s);
        }
        assert_eq!(store.skill_count(), 1, "still exactly one skill per trigger");
    }

    // 7D.8: contradiction weakens, then disables (skill decay)
    #[test]
    fn contradiction_weakens_and_disables_skill() {
        let store = SimpleSkillStore::new();
        let ev = SkillEvaluator::default();
        // verification_count = 2 so three failures are needed to exceed successes.
        let c = candidate(3, 2, 0, RiskClass::Low);
        if let SkillAdmission::Promote(s) = ev.evaluate(&c) {
            store.admit(s);
        }
        assert!(store.find_applicable(&trigger()).is_some());

        store.weaken(&trigger()); // failure 1 <= success 2 -> still active
        assert!(store.find_applicable(&trigger()).is_some());

        store.weaken(&trigger()); // failure 2 <= success 2 -> still active
        assert!(store.find_applicable(&trigger()).is_some());

        store.weaken(&trigger()); // failure 3 > success 2 -> disabled
        assert!(store.find_applicable(&trigger()).is_none(), "obsolete skill disabled");
    }

    // 7D.9: full Lesson -> Candidate -> Skill promotion path
    #[test]
    fn lesson_to_skill_full_path() {
        let ev = SkillEvaluator::default();
        let lesson = corroborated_lesson(3);
        let mut ev2 = SkillEvaluator::default();
        ev2.known_trajectories
            .insert(trigger(), vec!["fallbackZ".to_string()]);
        let traj = ev2.trajectory_for(&trigger()).unwrap();
        let cand = ev2.form_candidate(&lesson, &trigger(), traj);
        // form_candidate sets verification_evidence to 0; promote requires >=2, so it
        // rejects until verified trajectory evidence is attached. Simulate attach:
        let mut cand = cand;
        cand.verification_evidence = 2;
        match ev.evaluate(&cand) {
            SkillAdmission::Promote(skill) => {
                assert_eq!(skill.trigger, trigger());
                assert_eq!(skill.trajectory, vec!["fallbackZ".to_string()]);
            }
            other => panic!("expected promote, got {:?}", other),
        }
    }
}

/// Context the planner uses to query applicable skills (consultation seam). Mirrors
/// [crate::memory::LessonCondition] so a `SkillStore` can answer it directly.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SkillContext {
    pub action: String,
    pub signals: Vec<String>,
}

/// A skill the planner is *allowed to consider* — pure data, never auto-executed.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ApplicableSkill {
    pub id: String,
    pub trajectory: Vec<String>,
}

/// Planner-consultation seam. Given a context, returns the skills the planner MAY
/// use. The planner still decides whether to apply one — surfacing is not execution,
/// and must never bypass planning, permissions, or verification.
pub trait SkillProvider: Send + Sync {
    fn applicable_skills(&self, ctx: &SkillContext) -> Vec<ApplicableSkill>;
}

impl SkillProvider for SimpleSkillStore {
    fn applicable_skills(&self, ctx: &SkillContext) -> Vec<ApplicableSkill> {
        let cond = LessonCondition {
            action: ctx.action.clone(),
            signals: ctx.signals.clone(),
        };
        match self.find_applicable(&cond) {
            Some(sk) => vec![ApplicableSkill {
                id: sk.id,
                trajectory: sk.trajectory,
            }],
            None => vec![],
        }
    }
}
