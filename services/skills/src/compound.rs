//! Skill compounding — the closed loop that turns a **verified successful
//! trajectory** into a reusable skill.
//!
//! The constitution's Reflection gap: trajectory → outcome → pattern →
//! candidate procedure → **verification** → skill → registry → reuse. The
//! machinery before this module exists (`ReflectionEngine`, `PatternDetector`,
//! the experience layer, the concrete `CodeTestVerifier`); the missing link is
//! *compounding* — turning a verified execution into a registered skill.
//!
//! ```text
//! verified trajectory (goal-execution: Evidence[] + VerifiedResult)
//!      ↓
//! TrajectorySkill (neutral input: goal, verified outcome + evidence ids,
//!                  steps with instruction + capability, confidence, support)
//!      ↓
//! SkillCompiler::compile
//!      │  refusal gate (the verification step, enforced here):
//!      │    • Unverified outcome → refused — a failure is never compounded
//!      │    • confidence / support below the bar → refused
//!      │    • no steps / no usable capability → refused
//!      ↓
//! Skill (manifest distilled from the trajectory, source = Learned, enabled)
//!      ↓
//! SkillRegistry.register  →  emits `skill.compounded`
//!      ↓
//! reuse: a later goal's SkillRegistry::select_for ranks and chooses it
//! ```
//!
//! **Decoupling:** the input is neutral — no planner, goal-execution, or
//! provider types. The caller (wherever the verified trajectory lives) maps
//! its own verified evidence into a [`TrajectorySkill`]. The compiler stays in
//! the skills crate's testable-in-isolation spirit.
//!
//! **Honesty:** compounding is gated, not automatic. An [`OutcomeSummary`]
//! that is anything but `Verified` is refused with a structured reason — FRIDAY
//! never learns from garbage, which is the whole point of putting verification
//! before skill compounding.

use chrono::Utc;
use serde::{Deserialize, Serialize};

use crate::error::SkillError;
use crate::events::SkillEvent;
use crate::manifest::{SkillManifest, FRIDAY_CORE_VERSION};
use crate::registry::{CapabilityStatus, Skill, SkillRegistry, SkillSource};

/// How the trajectory ended — the compiler's primary gate lives on this.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum OutcomeSummary {
    /// The trajectory passed real verification; the evidence that proved it.
    Verified {
        /// Evidence ids from the verification trail (audit provenance).
        evidence_ids: Vec<String>,
    },
    /// The trajectory failed or was never verified — never compounded.
    Unverified {
        /// Why (e.g. "test result: FAILED", "abandoned mid-run").
        reason: String,
    },
}

/// One action in the trajectory: what was done and with which capability.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TrajectoryStep {
    /// The instruction that was executed (human-readable, also becomes a
    /// natural-language action for future goal matching).
    pub instruction: String,
    /// The capability registry id that actually ran (`git.commit`,
    /// `web.search`, …).
    pub capability: String,
}

/// The neutral input to [`SkillCompiler`]: a trajectory plus the evidence
/// that it worked.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TrajectorySkill {
    /// The goal the trajectory pursued — becomes the skill's description and
    /// its primary action.
    pub goal: String,
    pub outcome: OutcomeSummary,
    /// The ordered actions that were executed and verified.
    pub steps: Vec<TrajectoryStep>,
    /// How strongly the trajectory succeeded [0.0, 1.0].
    pub confidence: f32,
    /// How many times this pattern has been observed and verified.
    pub support: u32,
}

/// Why compounding was refused. Empty `refusals` = compiled.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Refusal {
    pub code: RefusalCode,
    pub detail: String,
}

/// The structured refusal reasons (the compiler never fails silently).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RefusalCode {
    /// The trajectory did not pass verification.
    Unverified,
    /// Confidence below the compiler's bar.
    BelowConfidence,
    /// Support below the compiler's bar.
    BelowSupport,
    /// No steps — nothing to turn into a skill.
    NoSteps,
    /// No step carries a usable capability.
    NoCapabilities,
    /// The trajectory yields no natural-language actions — a skill with zero
    /// actions could never be matched by a later goal.
    NoActions,
    /// The skill could not be registered (a genuine registry error — NOT a
    /// verdict on the trajectory's verification).
    RegistrationFailed,
}

/// The audit trail of a compounded skill: where it came from.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SkillProvenance {
    /// The goal that produced the trajectory.
    pub goal: String,
    /// Evidence ids from the verified outcome.
    pub evidence_ids: Vec<String>,
    /// How many verified observations led to this skill.
    pub support: u32,
    /// When compounding happened.
    pub compiled_at_ms: u64,
}

/// The outcome of one [`SkillCompiler::compile`] call.
#[derive(Debug, Clone)]
pub struct CompileOutcome {
    /// The registered skill — `Some` iff compounding succeeded.
    pub skill: Option<Skill>,
    /// The provenance — `Some` iff compounding succeeded.
    pub provenance: Option<SkillProvenance>,
    /// Structured reasons, non-empty iff compounding was refused.
    pub refusals: Vec<Refusal>,
}

impl CompileOutcome {
    pub fn compiled(&self) -> bool {
        self.skill.is_some()
    }
}

/// The compounding gate: verified trajectory → registered [`Skill`].
///
/// Defaults: confidence ≥ 0.6, support ≥ 1, category `learned`, author
/// `FRIDAY`, version `1.0.0`. All tunable via builders.
pub struct SkillCompiler {
    min_confidence: f32,
    min_support: u32,
    category: String,
}

impl Default for SkillCompiler {
    fn default() -> Self {
        Self::new()
    }
}

impl SkillCompiler {
    pub fn new() -> Self {
        Self {
            min_confidence: 0.6,
            min_support: 1,
            category: "learned".to_string(),
        }
    }

    pub fn with_confidence_bar(mut self, min_confidence: f32) -> Self {
        self.min_confidence = min_confidence.clamp(0.0, 1.0);
        self
    }

    pub fn with_support_bar(mut self, min_support: u32) -> Self {
        self.min_support = min_support;
        self
    }

    pub fn with_category(mut self, category: impl Into<String>) -> Self {
        self.category = category.into();
        self
    }

    /// Compile a trajectory into a registered skill — or refuse, with
    /// reasons. The refusal gate IS the verification step of the compounding
    /// loop: an [`OutcomeSummary::Unverified`] trajectory is never learned.
    ///
    /// On success the skill is registered (enabled, `SkillSource::Learned`,
    /// every used capability resolved — they all ran), a unique slug id is
    /// chosen against the live registry, and `skill.compounded` is emitted on
    /// the registry's event sink. Note that [`SkillRegistry::register`] also
    /// emits `skill.loaded` / `skill.enabled` (accurate — the skill IS in the
    /// registry and active); `skill.compounded` is the learning-specific
    /// marker carrying the evidence trail.
    pub fn compile(
        &self,
        registry: &SkillRegistry,
        trajectory: &TrajectorySkill,
    ) -> CompileOutcome {
        let mut refusals = Vec::new();

        // ── Refusal gate — the verification step, enforced here ──
        match &trajectory.outcome {
            OutcomeSummary::Verified { evidence_ids } => {
                if evidence_ids.is_empty() {
                    refusals.push(Refusal {
                        code: RefusalCode::Unverified,
                        detail: "verified outcome carries no evidence ids".to_string(),
                    });
                }
            }
            OutcomeSummary::Unverified { reason } => {
                refusals.push(Refusal {
                    code: RefusalCode::Unverified,
                    detail: format!("trajectory was not verified: {reason}"),
                });
                return CompileOutcome {
                    skill: None,
                    provenance: None,
                    refusals,
                };
            }
        }
        if trajectory.confidence < self.min_confidence {
            refusals.push(Refusal {
                code: RefusalCode::BelowConfidence,
                detail: format!(
                    "confidence {:.2} below the {:.2} bar",
                    trajectory.confidence, self.min_confidence
                ),
            });
        }
        if trajectory.support < self.min_support {
            refusals.push(Refusal {
                code: RefusalCode::BelowSupport,
                detail: format!(
                    "support {} below the {} bar",
                    trajectory.support, self.min_support
                ),
            });
        }
        if trajectory.steps.is_empty() {
            refusals.push(Refusal {
                code: RefusalCode::NoSteps,
                detail: "trajectory has no steps to distill".to_string(),
            });
        }
        let capabilities = usable_capabilities(trajectory);
        if capabilities.is_empty() {
            refusals.push(Refusal {
                code: RefusalCode::NoCapabilities,
                detail: "no step carries a usable capability".to_string(),
            });
        }
        let actions = distilled_actions(trajectory);
        if actions.is_empty() {
            refusals.push(Refusal {
                code: RefusalCode::NoActions,
                detail: "trajectory yields no natural-language actions for goal matching"
                    .to_string(),
            });
        }

        if !refusals.is_empty() {
            return CompileOutcome {
                skill: None,
                provenance: None,
                refusals,
            };
        }

        // ── Distill the trajectory into a pure-data manifest ──
        let manifest = SkillManifest {
            id: slugify(&trajectory.goal), // id is finalized during registration
            name: String::new(),           //  (unique-id loop below)
            version: "1.0.0".to_string(),
            description: trajectory.goal.clone(),
            author: "FRIDAY".to_string(),
            category: self.category.clone(),
            icon: String::new(),
            capabilities: capabilities.clone(),
            actions,
            permissions: Vec::new(), // normal runtime gates apply
            readonly: false,
            dependencies: Vec::new(),
            platform: "Any".to_string(),
            min_core_version: FRIDAY_CORE_VERSION.to_string(),
            enabled: true,
            memory: Default::default(),
        };
        let capability_statuses: Vec<CapabilityStatus> = capabilities
            .into_iter()
            .map(CapabilityStatus::Resolved)
            .collect();
        let skill = Skill {
            manifest,
            source: SkillSource::Learned,
            loaded_at: Utc::now(),
            enabled: true,
            capabilities: capability_statuses,
        };

        // ── Register with collision-safe id selection ──
        // The registry is Send + Sync: between an id check and the insert a
        // concurrent compile could grab the same id. Instead of checking then
        // acting, register directly and RETRY with the next suffix on a
        // collision — a duplicate is never reported as a verdict on the
        // trajectory. A genuine non-collision registry error is reported
        // honestly as RegistrationFailed.
        let base = slugify(&trajectory.goal);
        let mut n = 1u32;
        let registered = loop {
            let id = if n == 1 {
                base.clone()
            } else {
                format!("{base}-{n}")
            };
            let mut candidate = skill.clone();
            candidate.manifest.id = id.clone();
            candidate.manifest.name = titleize(&id);
            match registry.register(candidate.clone()) {
                Ok(()) => break candidate,
                Err(SkillError::Duplicate(_)) => n += 1,
                Err(e) => {
                    return CompileOutcome {
                        skill: None,
                        provenance: None,
                        refusals: vec![Refusal {
                            code: RefusalCode::RegistrationFailed,
                            detail: format!("could not register: {e}"),
                        }],
                    };
                }
            }
        };

        let OutcomeSummary::Verified { evidence_ids } = &trajectory.outcome else {
            unreachable!("verified gate passed above");
        };
        let provenance = SkillProvenance {
            goal: trajectory.goal.clone(),
            evidence_ids: evidence_ids.clone(),
            support: trajectory.support,
            compiled_at_ms: chrono::Utc::now().timestamp_millis() as u64,
        };

        // Audit: skill.compounded carries the full learning trail.
        let sink = registry.event_sink();
        sink.emit(SkillEvent::Compounded {
            skill_id: registered.id().to_string(),
            goal: trajectory.goal.clone(),
            evidence_ids: evidence_ids.clone(),
        });

        CompileOutcome {
            skill: Some(registered),
            provenance: Some(provenance),
            refusals: Vec::new(),
        }
    }
}

// ── Distillation helpers ──────────────────────────────────────────

/// The capabilities actually used, deduped in first-use order.
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

/// The skill's natural-language actions: the goal first, then each step's
/// instruction — deduped in order. These drive future goal matching.
fn distilled_actions(trajectory: &TrajectorySkill) -> Vec<String> {
    let mut actions = Vec::new();
    let mut push = |a: &str| {
        let a = a.trim();
        if !a.is_empty() && !actions.iter().any(|x| x == a) {
            actions.push(a.to_string());
        }
    };
    push(&trajectory.goal);
    for step in &trajectory.steps {
        push(&step.instruction);
    }
    actions
}

/// Slugify a goal into a skill id following the skill-id rules (a-z, 0-9,
/// '-', '_'): lowercase, non-alphanumeric → '-', collapse runs, trim.
fn slugify(goal: &str) -> String {
    let mut out = String::with_capacity(goal.len());
    let mut last_dash = false;
    for c in goal.to_lowercase().chars() {
        if c.is_ascii_alphanumeric() || c == '_' {
            out.push(c);
            last_dash = false;
        } else if !last_dash && !out.is_empty() {
            out.push('-');
            last_dash = true;
        }
    }
    while out.ends_with('-') {
        out.pop();
    }
    if out.is_empty() {
        "learned-skill".to_string()
    } else {
        out
    }
}

/// Titleize a slug into a human name (mirrors skill-forge's titleize).
fn titleize(id: &str) -> String {
    let mut out = String::with_capacity(id.len());
    let mut capitalize = true;
    for c in id.chars() {
        if c == '-' || c == '_' {
            out.push(' ');
            capitalize = true;
        } else if capitalize {
            out.extend(c.to_uppercase());
            capitalize = false;
        } else {
            out.push(c);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::VecSink;
    use std::sync::Arc;

    fn verified_trajectory(goal: &str) -> TrajectorySkill {
        TrajectorySkill {
            goal: goal.to_string(),
            outcome: OutcomeSummary::Verified {
                evidence_ids: vec!["ev_1".to_string()],
            },
            steps: vec![
                TrajectoryStep {
                    instruction: format!("run cargo test for {goal}"),
                    capability: "terminal.execute".to_string(),
                },
                TrajectoryStep {
                    instruction: "read the test summary".to_string(),
                    capability: "filesystem.read".to_string(),
                },
            ],
            confidence: 0.9,
            support: 3,
        }
    }

    #[test]
    fn verified_trajectory_compiles_and_registers() {
        let registry = SkillRegistry::new();
        let compiler = SkillCompiler::new();
        let outcome = compiler.compile(&registry, &verified_trajectory("run friday tests"));

        assert!(outcome.compiled(), "refusals: {:?}", outcome.refusals);
        let skill = outcome.skill.expect("registered");
        assert_eq!(skill.source, SkillSource::Learned);
        assert!(skill.enabled);
        assert!(skill.is_usable());
        // Capabilities deduped, in first-use order, all resolved (they ran).
        assert_eq!(
            skill.resolved_capabilities(),
            vec!["terminal.execute", "filesystem.read"]
        );
        // Actions: goal first, then each step instruction.
        assert_eq!(skill.manifest.actions[0], "run friday tests");
        assert_eq!(skill.manifest.actions.len(), 3);
        assert_eq!(skill.manifest.category, "learned");
        // Provenance carries the evidence trail.
        let provenance = outcome.provenance.expect("provenance");
        assert_eq!(provenance.evidence_ids, vec!["ev_1"]);
        assert_eq!(provenance.goal, "run friday tests");
        assert_eq!(provenance.support, 3);
    }

    #[test]
    fn unverified_trajectory_is_refused_with_reason() {
        let registry = SkillRegistry::new();
        let mut trajectory = verified_trajectory("deploy the service");
        trajectory.outcome = OutcomeSummary::Unverified {
            reason: "test result: FAILED".to_string(),
        };

        let outcome = SkillCompiler::new().compile(&registry, &trajectory);
        assert!(!outcome.compiled());
        assert!(outcome.skill.is_none());
        assert_eq!(outcome.refusals.len(), 1);
        assert_eq!(outcome.refusals[0].code, RefusalCode::Unverified);
        assert!(
            outcome.refusals[0].detail.contains("FAILED"),
            "{}",
            outcome.refusals[0].detail
        );
        assert!(registry.all().is_empty(), "nothing was registered");
    }

    #[test]
    fn verified_outcome_without_evidence_is_refused() {
        let registry = SkillRegistry::new();
        let mut trajectory = verified_trajectory("ship the release");
        trajectory.outcome = OutcomeSummary::Verified {
            evidence_ids: vec![],
        };
        let outcome = SkillCompiler::new().compile(&registry, &trajectory);
        assert!(!outcome.compiled());
        assert_eq!(outcome.refusals[0].code, RefusalCode::Unverified);
    }

    #[test]
    fn below_confidence_and_support_are_refused() {
        let registry = SkillRegistry::new();
        let mut trajectory = verified_trajectory("optimize the query");
        trajectory.confidence = 0.4;
        let outcome = SkillCompiler::new().compile(&registry, &trajectory);
        assert!(!outcome.compiled());
        assert!(outcome
            .refusals
            .iter()
            .any(|r| r.code == RefusalCode::BelowConfidence));

        let mut trajectory = verified_trajectory("optimize the query");
        trajectory.support = 0;
        let outcome = SkillCompiler::new().compile(&registry, &trajectory);
        assert!(!outcome.compiled());
        assert!(outcome
            .refusals
            .iter()
            .any(|r| r.code == RefusalCode::BelowSupport));
    }

    #[test]
    fn no_steps_or_no_capabilities_refuse() {
        let registry = SkillRegistry::new();
        let mut trajectory = verified_trajectory("do nothing");
        trajectory.steps = vec![];
        let outcome = SkillCompiler::new().compile(&registry, &trajectory);
        assert!(!outcome.compiled());
        assert!(outcome
            .refusals
            .iter()
            .any(|r| r.code == RefusalCode::NoSteps));

        let mut trajectory = verified_trajectory("do nothing");
        trajectory.steps = vec![TrajectoryStep {
            instruction: "think hard".to_string(),
            capability: "".to_string(),
        }];
        let outcome = SkillCompiler::new().compile(&registry, &trajectory);
        assert!(!outcome.compiled());
        assert!(outcome
            .refusals
            .iter()
            .any(|r| r.code == RefusalCode::NoCapabilities));
    }

    #[test]
    fn duplicate_goal_gets_a_unique_slug_id() {
        let registry = SkillRegistry::new();
        let compiler = SkillCompiler::new();
        let first = compiler.compile(&registry, &verified_trajectory("run friday tests"));
        let second = compiler.compile(&registry, &verified_trajectory("run friday tests"));

        let id1 = first.skill.expect("first registered").id().to_string();
        let id2 = second.skill.expect("second registered").id().to_string();
        assert_eq!(id1, "run-friday-tests");
        assert_eq!(id2, "run-friday-tests-2");
        assert_eq!(registry.all().len(), 2);
    }

    #[test]
    fn slugify_follows_skill_id_rules() {
        assert_eq!(
            slugify("Run cargo test on the Friday project!"),
            "run-cargo-test-on-the-friday-project"
        );
        assert_eq!(slugify("  spaced  out  "), "spaced-out");
        assert_eq!(slugify("!!!"), "learned-skill");
        // '_' is a valid skill-id char — it survives, like '-'.
        assert_eq!(slugify("CamelCase_Underscore"), "camelcase_underscore");
    }

    #[test]
    fn compounding_emits_the_compounded_event_with_the_evidence_trail() {
        let sink = Arc::new(VecSink::new());
        let registry = SkillRegistry::with_sink(sink.clone());
        let outcome =
            SkillCompiler::new().compile(&registry, &verified_trajectory("backup the logs"));

        assert!(outcome.compiled());
        let events = sink.events.lock().expect("sink");
        let compounded: Vec<_> = events
            .iter()
            .filter(|e| matches!(e, SkillEvent::Compounded { .. }))
            .collect();
        assert_eq!(compounded.len(), 1, "exactly one skill.compounded event");
        if let SkillEvent::Compounded {
            skill_id,
            goal,
            evidence_ids,
        } = compounded[0]
        {
            assert_eq!(skill_id, "backup-the-logs");
            assert_eq!(goal, "backup the logs");
            assert_eq!(
                evidence_ids,
                &vec!["ev_1".to_string()],
                "the trail, not a count"
            );
        }
    }

    #[test]
    fn no_natural_language_actions_refuses() {
        let registry = SkillRegistry::new();
        let mut trajectory = verified_trajectory("   ");
        trajectory.steps = vec![TrajectoryStep {
            instruction: "   ".to_string(),
            capability: "terminal.execute".to_string(),
        }];
        let outcome = SkillCompiler::new().compile(&registry, &trajectory);
        assert!(!outcome.compiled());
        assert!(outcome
            .refusals
            .iter()
            .any(|r| r.code == RefusalCode::NoActions));
        assert!(
            registry.all().is_empty(),
            "an unrankable skill never registers"
        );
    }

    /// The constitution acceptance: a later goal *uses* the learned skill.
    #[test]
    fn later_goal_selects_the_learned_skill() {
        let registry = SkillRegistry::new();
        let compiler = SkillCompiler::new();
        let outcome = compiler.compile(&registry, &verified_trajectory("run friday tests"));
        assert!(outcome.compiled());

        // A NEW goal, phrased differently, but lexically related.
        let selected = registry.select_for("please run the friday test suite", 5);
        assert!(selected.is_some(), "a matching later goal finds the skill");
        assert_eq!(selected.unwrap().skill.id(), "run-friday-tests");
    }

    /// Honesty at the registry level: a refused trajectory never appears.
    #[test]
    fn refused_trajectory_never_reaches_reuse() {
        let registry = SkillRegistry::new();
        let mut trajectory = verified_trajectory("deploy broken code");
        trajectory.outcome = OutcomeSummary::Unverified {
            reason: "panicked".to_string(),
        };
        assert!(!SkillCompiler::new()
            .compile(&registry, &trajectory)
            .compiled());
        assert!(registry.select_for("deploy broken code", 5).is_none());
    }
}
