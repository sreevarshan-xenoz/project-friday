//! Skill learning — the runtime compounding seam (Skill compounding §3.3).
//!
//! The verified-experience producer (goal-execution) hands the neutral
//! [`TrajectorySkill`] to this service, which owns the two honest halves of
//! the loop:
//!
//! ```text
//! verified experience → SkillLearningService::learn
//!      │  honesty gate: at least one step capability must resolve against the
//!      │  LIVE planner registry — a trajectory nothing could ever execute is
//!      │  refused before it registers
//!      ↓
//! SkillCompiler::compile  →  registered CANDIDATE (disabled, not selectable)
//!      ↓  capability statuses re-resolved against the live registry
//!      │  (Resolved vs. SoftDeclared reflect reality, not the trajectory's claim)
//!      ↓
//! SkillLearningService::admit
//!      │  qualification pipeline (friday-skills) → explicit admission
//!      ↓
//! SkillRegistry::enable_learned  →  enabled, selectable by a later goal
//! ```
//!
//! **One registry, two roles:** the service operates on the *same*
//! [`SkillRegistry`] and planner [`CapabilityRegistry`] the wiring loaded —
//! via [`SkillEngineWiring::skill_learning`] — so compounding produces a skill
//! in the registry the planner actually selects from. There is deliberately no
//! second "learning store": candidates live alongside bundled skills and are
//! simply invisible to `select_for` until admitted.
//!
//! **Admission is explicit:** `learn` never makes a skill executable. Only
//! `admit` (which runs the qualification gate) flips a candidate live, and
//! only for `SkillSource::Learned` skills. A failure is never learned, and a
//! learned skill is never live before admission.

use std::sync::Arc;

use friday_planner::execution::capability_registry::CapabilityRegistry as PlannerCapabilityRegistry;
use friday_skills::{
    qualification::QualificationReport, Refusal, Skill, SkillCompiler, SkillProvenance,
    SkillRegistry, TrajectorySkill,
};

use crate::PlannerRegistryResolver;

/// Why a trajectory did not become a usable learned skill.
#[derive(Debug)]
pub enum LearnError {
    /// The compiler refused the trajectory — its refusal gate IS the
    /// verification step (Unverified / below-bar confidence / no steps / no
    /// usable capability / no natural-language actions).
    Refused { refusals: Vec<Refusal> },
    /// No step capability resolves against the live planner registry — this
    /// trajectory could never execute, so it is not even registered.
    NoUsableCapability { capabilities: Vec<String> },
    /// The candidate exists but explicit admission failed (the qualification
    /// gate or the registry refused it, e.g. a non-Learned or unknown id).
    Admission { skill_id: String, detail: String },
}

impl std::fmt::Display for LearnError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LearnError::Refused { refusals } => write!(
                f,
                "compounding refused: {:?}",
                refusals
                    .iter()
                    .map(|r| (r.code, r.detail.as_str()))
                    .collect::<Vec<_>>()
            ),
            LearnError::NoUsableCapability { capabilities } => write!(
                f,
                "no step capability resolves against the planner registry (used: {capabilities:?})"
            ),
            LearnError::Admission { skill_id, detail } => {
                write!(f, "admission of '{skill_id}' failed: {detail}")
            }
        }
    }
}

impl std::error::Error for LearnError {}

/// A compiled-but-not-yet-admitted learned skill: registered, disabled, and
/// invisible to the planner until [`SkillLearningService::admit`] is called.
#[derive(Debug, Clone)]
pub struct LearnedCandidate {
    /// The registered skill. `enabled == false` — a candidate, not a live
    /// skill.
    pub skill: Arc<Skill>,
    /// The audit trail of where it came from (goal, evidence ids, support).
    pub provenance: SkillProvenance,
}

/// The runtime compounding seam: `verified experience → SkillCompiler →
/// admission gate → SkillRegistry`.
pub struct SkillLearningService {
    registry: Arc<SkillRegistry>,
    planner_registry: Arc<PlannerCapabilityRegistry>,
    compiler: SkillCompiler,
}

impl SkillLearningService {
    pub fn new(
        registry: Arc<SkillRegistry>,
        planner_registry: Arc<PlannerCapabilityRegistry>,
    ) -> Self {
        Self {
            registry,
            planner_registry,
            compiler: SkillCompiler::new(),
        }
    }

    /// The shared registry the service compounds into — the same one the
    /// planner selects from. Exposed so callers can inspect/select the
    /// candidates without a second handle.
    pub fn registry_handle(&self) -> Arc<SkillRegistry> {
        Arc::clone(&self.registry)
    }

    /// Compile a verified trajectory into a registered **candidate** — or
    /// refuse it. Two honest gates run here:
    ///
    /// 1. **The capability gate:** at least one step capability must resolve
    ///    against the live planner registry. A trajectory whose capabilities
    ///    could never execute is refused *before* anything is registered.
    /// 2. **The compiler gate:** [`SkillCompiler::compile`] enforces the
    ///    verification step (Verified outcome + evidence, confidence/support
    ///    bars, non-empty steps and actions).
    ///
    /// On success the skill is registered disabled with
    /// `SkillSource::Learned`, and its capability statuses are re-resolved
    /// against the live registry so `Resolved`/`SoftDeclared` reflect reality
    /// instead of the trajectory's claim. It is NOT selectable until admitted.
    pub fn learn(&self, trajectory: &TrajectorySkill) -> Result<LearnedCandidate, LearnError> {
        let capabilities = usable_capabilities(trajectory);
        let resolvable: Vec<&String> = capabilities
            .iter()
            .filter(|cap| self.planner_registry.resolve(cap).is_some())
            .collect();
        if resolvable.is_empty() {
            return Err(LearnError::NoUsableCapability { capabilities });
        }

        let outcome = self.compiler.compile(&self.registry, trajectory);
        if !outcome.compiled() {
            return Err(LearnError::Refused {
                refusals: outcome.refusals,
            });
        }
        let skill = outcome.skill.expect("compiled above");

        // Honest resolution: refresh every status against the live planner
        // registry so a claimed-but-unbound capability is SoftDeclared, not
        // blindly Resolved.
        self.registry
            .reconcile(&PlannerRegistryResolver(Arc::clone(&self.planner_registry)));

        Ok(LearnedCandidate {
            skill: Arc::new(skill),
            provenance: outcome.provenance.expect("compiled above"),
        })
    }

    /// Explicitly admit a learned candidate: run the qualification pipeline
    /// against the live planner registry, and only when it passes flip the
    /// candidate into an enabled, selectable skill. Returns the qualification
    /// report so a refused admission is fully inspectable. Admission applies
    /// only to `SkillSource::Learned` skills (the registry enforces that); an
    /// unknown or non-Learned id is a [`LearnError::Admission`].
    pub fn admit(&self, skill_id: &str) -> Result<QualificationReport, LearnError> {
        let report = friday_skills::qualify(
            &self.registry,
            &PlannerRegistryResolver(Arc::clone(&self.planner_registry)),
            skill_id,
        );
        if report.passed {
            self.registry.enable_learned(skill_id).map_err(|e| {
                LearnError::Admission {
                    skill_id: skill_id.to_string(),
                    detail: e.to_string(),
                }
            })?;
        }
        Ok(report)
    }
}

/// The capabilities the trajectory actually uses, deduped in first-use order
/// (mirrors the compiler's distillation so the pre-registration gate sees the
/// same set it will register).
fn usable_capabilities(trajectory: &TrajectorySkill) -> Vec<String> {
    let mut caps: Vec<String> = Vec::new();
    for step in &trajectory.steps {
        let cap = step.capability.trim();
        if cap.is_empty() || caps.iter().any(|c| c == cap) {
            continue;
        }
        caps.push(cap.to_string());
    }
    caps
}
