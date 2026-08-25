//! 8C — production skill store.
//!
//! Implements the frozen 7D [`SkillStore`](crate::skill::SkillStore) trait over
//! the real `friday-skills` engine ([`friday_skills::SkillRegistry`] +
//! [`friday_skills::SkillCompiler`]). The autonomy
//! [`SkillEvaluator`](crate::skill::SkillEvaluator) remains the authoritative
//! promotion gate (support ≥ 3, verifications ≥ 2, contradictory = 0, risk ≠
//! High). This adapter binds an *already-promoted* 7D `Skill` into the
//! production compounding pipeline:
//!
//! ```text
//! 7D Skill  →  TrajectorySkill (verified outcome derived from the 7D counts)
//!           →  SkillCompiler::compile  (refusal gate: unverified / empty refused)
//!           →  SkillRegistry (SkillSource::Learned candidate, registered DISABLED)
//!           →  SkillRegistry::enable_learned  (explicit admission gate)
//!           →  find_applicable via the production registry + the meta map
//! ```
//!
//! The asymmetry is preserved at the seam: a 7D skill with `verification_count
//! == 0` (i.e. anything that did not pass real verification) is mapped to
//! [`OutcomeSummary::Unverified`] and the compiler *refuses* it — it is never
//! registered, never selectable. A failure is learned as a lesson (7C), never
//! compounded into a skill.
//!
//! 7D-specific metadata the production `Skill` does not model (confidence,
//! `support_count`, `verification_count`, `failure_count`, the `SkillStatus`
//! enum, the exact `LessonCondition` trigger) is kept in an authoritative side
//! `meta` map so `find_applicable` returns a faithful 7D `Skill`. This mirrors
//! 8B's [`crate::memory_production::ProductionLessonStore`]: the production
//! engine owns durable admission/selection; the autonomy types own reasoning.

use std::collections::HashMap;
use std::sync::Mutex;

use friday_skills::compound::{OutcomeSummary, SkillCompiler, TrajectorySkill, TrajectoryStep};
use friday_skills::registry::SkillRegistry;

use crate::memory::LessonCondition;
use crate::skill::{
    ApplicableSkill, Skill as AutonomySkill, SkillContext, SkillProvider, SkillStatus, SkillStore,
};

/// Synthetic capability bound to every learned step. FRIDAY's hermetic 7D skills
/// carry no capability binding (they are consulted, never executed); the real
/// production `Skill` requires at least one capability to be selectable. Binding
/// a single synthetic capability keeps the production admission/selection path
/// live without pretending the autonomy layer knows the concrete runtime
/// capability — that binding is the planner's job when it actually consults.
const LEARNED_CAPABILITY: &str = "friday.skill.learned";

fn now_ms() -> u64 {
    chrono::Utc::now().timestamp_millis() as u64
}

/// A `SkillStore` backed by the real `friday-skills` registry + compiler.
pub struct ProductionSkillStore {
    compiler: SkillCompiler,
    registry: SkillRegistry,
    /// trigger → production skill id (dedup + navigation).
    by_trigger: Mutex<HashMap<LessonCondition, String>>,
    /// trigger → faithful 7D `Skill` metadata (the autonomy reasoning layer).
    meta: Mutex<HashMap<LessonCondition, AutonomySkill>>,
}

impl ProductionSkillStore {
    pub fn new() -> Self {
        Self {
            // The autonomy `SkillEvaluator` is the authoritative gate, so the
            // compiler's own confidence/support bars are set permissive; only the
            // structural + verification refusal gates stay live.
            compiler: SkillCompiler::new()
                .with_confidence_bar(0.0)
                .with_support_bar(0),
            registry: SkillRegistry::new(),
            by_trigger: Mutex::new(HashMap::new()),
            meta: Mutex::new(HashMap::new()),
        }
    }

    /// Wrap an externally-owned registry (useful for golden/seam assertions).
    pub fn with_registry(registry: SkillRegistry) -> Self {
        Self {
            compiler: SkillCompiler::new()
                .with_confidence_bar(0.0)
                .with_support_bar(0),
            registry,
            by_trigger: Mutex::new(HashMap::new()),
            meta: Mutex::new(HashMap::new()),
        }
    }

    /// The production registry this store binds to (for seam assertions).
    pub fn registry(&self) -> &SkillRegistry {
        &self.registry
    }

    /// Number of admitted (registered) production skills.
    pub fn skill_count(&self) -> usize {
        self.registry.all().len()
    }

    /// Map a 7D `Skill` into a `TrajectorySkill`. The verification outcome is
    /// derived from the 7D counts: only a skill with at least one verified
    /// success and no contradictory failures is treated as `Verified`.
    fn to_trajectory(skill: &AutonomySkill) -> TrajectorySkill {
        let outcome = if skill.verification_count > 0 && skill.failure_count == 0 {
            OutcomeSummary::Verified {
                evidence_ids: vec![format!("skill-admit:{}", skill.id)],
            }
        } else {
            OutcomeSummary::Unverified {
                reason: "skill reached admission without a verified trajectory".into(),
            }
        };
        let steps: Vec<TrajectoryStep> = skill
            .trajectory
            .iter()
            .map(|instr| TrajectoryStep {
                instruction: instr.clone(),
                capability: LEARNED_CAPABILITY.to_string(),
            })
            .collect();
        TrajectorySkill {
            goal: skill.trigger.action.clone(),
            outcome,
            steps,
            confidence: skill.confidence,
            support: skill.support_count as u32,
        }
    }
}

impl Default for ProductionSkillStore {
    fn default() -> Self {
        Self::new()
    }
}

impl SkillStore for ProductionSkillStore {
    fn admit(&self, skill: AutonomySkill) {
        // Dedup by trigger: one skill per condition — strengthen the existing
        // skill instead of registering a duplicate candidate.
        if self
            .by_trigger
            .lock()
            .expect("skill store poisoned")
            .contains_key(&skill.trigger)
        {
            self.strengthen(&skill.trigger);
            return;
        }

        let trajectory = Self::to_trajectory(&skill);
        let outcome = self.compiler.compile(&self.registry, &trajectory);

        // The compiler returns None iff the refusal gate fired (unverified /
        // empty / below bar). In that case nothing is registered — the
        // "never learn from garbage" guarantee is enforced here.
        let Some(registered) = outcome.skill else {
            return;
        };
        let id = registered.id().to_string();

        // Explicit admission gate: the compiled candidate is disabled until
        // deliberately enabled — the closed learning loop.
        let _ = self.registry.enable_learned(&id);

        self.by_trigger
            .lock()
            .expect("skill store poisoned")
            .insert(skill.trigger.clone(), id);
        self.meta
            .lock()
            .expect("skill store poisoned")
            .insert(skill.trigger.clone(), skill);
    }

    fn find_applicable(&self, trigger: &LessonCondition) -> Option<AutonomySkill> {
        // Production registry is the source of truth for existence + admission.
        let id = self
            .by_trigger
            .lock()
            .expect("skill store poisoned")
            .get(trigger)
            .cloned()?;
        let prod = self.registry.get(&id)?;
        if !prod.enabled {
            return None;
        }
        // 7D metadata layer owns the status enum semantics.
        let meta = self.meta.lock().expect("skill store poisoned");
        let sk = meta.get(trigger)?;
        if sk.status == SkillStatus::Active {
            Some(sk.clone())
        } else {
            None
        }
    }

    fn strengthen(&self, trigger: &LessonCondition) {
        let mut meta = self.meta.lock().expect("skill store poisoned");
        if let Some(sk) = meta.get_mut(trigger) {
            sk.support_count += 1;
            sk.confidence = (sk.confidence + 0.05).min(0.99);
            sk.updated_at_ms = now_ms();
        }
        // No production-side op: the registry keeps one compiled candidate;
        // corroboration is tracked in the autonomy meta map.
    }

    fn weaken(&self, trigger: &LessonCondition) {
        let mut meta = self.meta.lock().expect("skill store poisoned");
        let id = self
            .by_trigger
            .lock()
            .expect("skill store poisoned")
            .get(trigger)
            .cloned();
        if let Some(sk) = meta.get_mut(trigger) {
            sk.failure_count += 1;
            sk.confidence = (sk.confidence - 0.1).max(0.01);
            sk.updated_at_ms = now_ms();
            // Decay: once verified failures outweigh successes, disable; before
            // that, a badly-shaken confidence suspends. Both map to "not
            // currently applicable" in the production registry.
            let disabled = if sk.failure_count > sk.verification_count {
                sk.status = SkillStatus::Disabled;
                true
            } else if sk.confidence < 0.3 {
                sk.status = SkillStatus::Suspended;
                true
            } else {
                false
            };
            if disabled {
                if let Some(id) = id {
                    let _ = self.registry.disable(&id);
                }
            }
        }
    }

    fn disable(&self, trigger: &LessonCondition) {
        let mut meta = self.meta.lock().expect("skill store poisoned");
        if let Some(sk) = meta.get_mut(trigger) {
            sk.status = SkillStatus::Disabled;
        }
        if let Some(id) = self
            .by_trigger
            .lock()
            .expect("skill store poisoned")
            .remove(trigger)
        {
            let _ = self.registry.disable(&id);
        }
    }
}

impl SkillProvider for ProductionSkillStore {
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
