//! Runtime wiring for the Skill Engine (M5) into the planner boot path.
//!
//! This is where the architecture becomes real: the Skill Engine is loaded
//! against the *same* planner [`CapabilityRegistry`] the supervisor resolves,
//! and the supervisor's default allow-all policy is swapped for the
//! skill-aware [`SkillPolicyAuthority`]. The result is the permission model
//! from `specs/skill-engine.md` §7: a readonly skill covering a safe
//! capability auto-approves, a user-approved skill unlocks its capabilities
//! for the session, and everything else prompts.
//!
//! **Boundary:** approval unlocks only `Risk::Safe` / `Risk::Medium`
//! capabilities. `Risk::High` capabilities pause at the supervisor's metadata
//! gate unconditionally (they never consult the policy), so a skill approval
//! cannot make `terminal.exec` or `git.push` run without a human gate.
//!
//! Loading is deliberately resilient: a bad manifest fails that skill (and is
//! reported in [`LoadReport`]), never the boot; an unreadable directory is
//! recorded in [`SkillEngineWiring::dir_errors`] and the remaining
//! directories still load.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use friday_core::Friday;
use friday_planner::execution::GoalCompletionSink;
use friday_planner::execution::capability_registry::CapabilityRegistry as PlannerCapabilityRegistry;
use friday_planner::execution::PolicyAuthority;
use friday_planner::planning::planners::backend::ReasoningBackend;
use friday_skills::{
    CapabilityResolver, LoadReport, PermissionState, RiskResolver, SkillApprovals,
    SkillContextSource, SkillHealth, SkillLoader, SkillMemory, SkillMemorySource, SkillPolicy,
    SkillRegistry, SkillSource,
};

use crate::{
    skill_learning::SkillLearningService, PlannerRegistryResolver, RegistryHealthSource,
    RegistryRiskResolver, SkillLearningObserver, SkillPolicyAuthority,
};

/// One skill source directory to load: `(path, source)`.
pub type SkillDir = (PathBuf, SkillSource);

/// The loaded skill stack handed back from the wiring point. The caller uses
/// it to build `AgentContext` (`context_source()` → `with_skills`) and to run
/// the user approval flow (`approvals.approve("docker")` unlocks Docker).
pub struct SkillEngineWiring {
    /// The loaded skill registry. Implements [`SkillContextSource`], so it can
    /// be handed straight to `ContextBuilder::with_skills`.
    pub registry: Arc<SkillRegistry>,
    /// Session-scoped approval store consulted by the policy.
    pub approvals: Arc<SkillApprovals>,
    /// The skill-aware policy wired into the supervisor.
    pub policy: Arc<SkillPolicy>,
    /// Per-source load reports (scanned / loaded / failed counts).
    pub reports: Vec<LoadReport>,
    /// Source directories that could not be read at all (e.g. missing). The
    /// remaining directories still loaded; the boot never fails on these.
    pub dir_errors: Vec<String>,
    /// The planner capability registry the skills were resolved against (also
    /// the registry the supervisor gates on). Kept so [`SkillEngineWiring::reconcile`]
    /// and [`SkillEngineWiring::health`] read *live* state, not the load-time
    /// snapshot.
    pub planner_registry: Arc<PlannerCapabilityRegistry>,
    /// The runtime memory source (M8). `None` until the host attaches one via
    /// [`SkillEngineWiring::with_memory`]; skills then get a gated per-skill
    /// handle from [`SkillEngineWiring::memory_for`].
    pub memory: Option<Arc<dyn SkillMemorySource>>,
}

impl SkillEngineWiring {
    /// A boxed context source over the loaded registry, for
    /// `ContextBuilder::with_skills`.
    pub fn context_source(&self) -> Arc<dyn SkillContextSource> {
        self.registry.clone()
    }

    /// `true` when every source directory loaded without a single failed
    /// manifest and no directory was unreadable.
    pub fn all_loaded(&self) -> bool {
        self.dir_errors.is_empty() && self.reports.iter().all(|r| r.ok())
    }

    /// Live re-resolution: refresh every skill's [`CapabilityStatus`] against
    /// the current planner registry (a capability registered or removed at
    /// runtime now changes `is_usable()` instead of staying frozen at load).
    /// Returns the number of skills whose resolution status changed; a
    /// nonzero count is logged as a warning (skills reconciled against a
    /// drifted registry).
    pub fn reconcile(&self) -> usize {
        let changed = self
            .registry
            .reconcile(&PlannerRegistryResolver(Arc::clone(&self.planner_registry)));
        if changed > 0 {
            tracing::warn!(
                changed,
                "skill capabilities re-resolved against the live registry"
            );
        }
        changed
    }

    /// Health of every loaded skill (id, [`SkillHealth`]), sourced from the
    /// *live* registry health map — `set_health(Offline)` on the planner
    /// registry is reflected here immediately.
    pub fn health(&self) -> Vec<(String, SkillHealth)> {
        self.registry
            .health_summary(&RegistryHealthSource(Arc::clone(&self.planner_registry)))
    }

    /// Health of one loaded skill (see [`SkillEngineWiring::health`]).
    pub fn health_of(&self, skill_id: &str) -> Option<SkillHealth> {
        self.registry.health_of(
            skill_id,
            &RegistryHealthSource(Arc::clone(&self.planner_registry)),
        )
    }

    /// Attach the runtime memory source (M8). Skills then hold a per-skill
    /// [`SkillMemory`] handle via [`SkillEngineWiring::memory_for`] that only
    /// allows the keys they declared in `skill.json`.
    pub fn with_memory(mut self, source: Arc<dyn SkillMemorySource>) -> Self {
        self.memory = Some(source);
        self
    }

    /// A memory handle for one loaded skill, gated to that skill's declared
    /// `memory.reads` / `memory.writes` keys. `None` when no memory source is
    /// wired or the skill is not registered. The handle reads the skill's
    /// **live** registry state per operation (a skill disabled after the
    /// handle was created is denied immediately).
    pub fn memory_for(&self, skill_id: &str) -> Option<SkillMemory> {
        let source = self.memory.clone()?;
        self.registry.get(skill_id)?;
        Some(SkillMemory::new(
            source,
            Arc::clone(&self.registry),
            skill_id,
        ))
    }

    /// The runtime skill-learning seam (skill compounding §3.3): a
    /// [`SkillLearningService`] over the SAME registry and planner registry
    /// this wiring loaded. Verified trajectories compound into candidates via
    /// [`SkillLearningService::learn`]; explicit admission via
    /// [`SkillLearningService::admit`] makes them selectable — all inside the
    /// registry the supervisor and planner actually use.
    pub fn skill_learning(&self) -> SkillLearningService {
        SkillLearningService::new(
            Arc::clone(&self.registry),
            Arc::clone(&self.planner_registry),
        )
    }
}

/// Load every skill directory into a fresh [`SkillRegistry`], resolving each
/// declared capability against `planner_registry` (the live registry the
/// supervisor will gate and dispatch on). An unreadable directory is recorded
/// in [`SkillEngineWiring::dir_errors`] (and logged) instead of aborting, so
/// a missing user-skill directory never discards the bundled skills.
pub fn load_skill_engine(
    planner_registry: Arc<PlannerCapabilityRegistry>,
    skill_dirs: &[SkillDir],
) -> SkillEngineWiring {
    let registry = Arc::new(SkillRegistry::new());
    let resolver: Arc<dyn CapabilityResolver> =
        Arc::new(PlannerRegistryResolver(planner_registry.clone()));
    let loader = SkillLoader::new(Arc::clone(&registry), resolver);

    let mut reports = Vec::new();
    let mut dir_errors = Vec::new();
    for (dir, source) in skill_dirs {
        match loader.load_dir(dir, *source) {
            Ok(report) => reports.push(report),
            Err(err) => {
                tracing::warn!(
                    dir = %dir.display(),
                    error = %err,
                    "skill directory skipped"
                );
                dir_errors.push(format!("{}: {err}", dir.display()));
            }
        }
    }

    let approvals = Arc::new(SkillApprovals::new());
    let risk: Arc<dyn RiskResolver> = Arc::new(RegistryRiskResolver(planner_registry.clone()));
    let policy = Arc::new(SkillPolicy::new(
        Arc::clone(&registry),
        Arc::clone(&approvals),
        risk,
    ));

    SkillEngineWiring {
        registry,
        approvals,
        policy,
        reports,
        dir_errors,
        planner_registry,
        memory: None,
    }
}

/// Why a capability of the chosen skill runs, prompts, or defers — the
/// rendered form of [`PermissionState`] in the decision trace.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SkillPermission {
    /// A readonly skill covers a safe capability → runs without prompting.
    AutoApproved,
    /// The user approved a covering skill this session.
    Approved,
    /// No *enabled* skill declares the capability → the previous policy
    /// decides (allow-all by default), not the Skill Engine. Reserved for
    /// future non-chosen capability rendering: a chosen skill always declares
    /// its own capabilities, so its own rows never reach this state.
    Deferred,
    /// A covering skill exists but nothing auto-approves it yet.
    RequiresApproval,
}

/// One declared capability of the chosen skill with its permission state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillDecisionCapability {
    pub capability: String,
    pub permission: SkillPermission,
}

/// The auditable skill-selection chain for one goal — the explainability
/// surface (`Relevant → Chosen → Capabilities → Permission`).
#[derive(Debug, Clone)]
pub struct SkillDecision {
    pub goal: String,
    /// Every ranked candidate `(skill id, score)` in rank order (enabled
    /// skills only — the ones selection actually considered).
    pub candidates: Vec<(String, f64)>,
    /// Disabled skills that *would* have ranked for this goal, with scores.
    /// The auditable answer to "why wasn't X chosen?" when X is switched off.
    pub skipped: Vec<(String, f64)>,
    /// The deterministic best usable skill for the goal, if any.
    pub chosen: Option<String>,
    /// The chosen skill's declared capabilities with their permission state.
    pub capabilities: Vec<SkillDecisionCapability>,
}

impl SkillEngineWiring {
    /// Decide the best skill for `goal` and explain the choice end-to-end.
    /// Emits the `[Skill Decision]` trace with the full chain — ranked
    /// candidates with scores, the chosen skill, and each capability's
    /// permission state — so a goal's selection is auditable the way a
    /// human would debug it. Scores and decisions live in the trace, never in
    /// the LLM context.
    pub fn decision_for(&self, goal: &str) -> SkillDecision {
        let selected = self.registry.select_for(goal, 8);
        let candidates: Vec<(String, f64)> = selected
            .as_ref()
            .map(|s| {
                s.candidates
                    .iter()
                    .map(|r| (r.skill.id().to_string(), r.score))
                    .collect()
            })
            .unwrap_or_default();
        // "Preferred but off": disabled skills ranked against the same goal,
        // so the trace can explain a skip instead of hiding it.
        let skipped: Vec<(String, f64)> = self
            .registry
            .search_disabled(goal, 8)
            .into_iter()
            .map(|r| (r.skill.id().to_string(), r.score))
            .collect();
        let chosen = selected.as_ref().map(|s| s.skill.id().to_string());
        let capabilities: Vec<SkillDecisionCapability> = selected
            .as_ref()
            .map(|s| {
                s.skill
                    .manifest
                    .capabilities
                    .iter()
                    .map(|cap| SkillDecisionCapability {
                        capability: cap.clone(),
                        permission: match self.policy.permission_for(cap) {
                            PermissionState::AutoApproved => SkillPermission::AutoApproved,
                            PermissionState::Approved => SkillPermission::Approved,
                            PermissionState::Unmanaged => SkillPermission::Deferred,
                            PermissionState::PromptRequired => SkillPermission::RequiresApproval,
                        },
                    })
                    .collect()
            })
            .unwrap_or_default();

        tracing::info!(
            goal = %goal,
            chosen = ?chosen,
            ranked = ?candidates,
            skipped_disabled = ?skipped,
            permissions = ?capabilities
                .iter()
                .map(|c| (&c.capability, &c.permission))
                .collect::<Vec<_>>(),
            "[Skill Decision] relevant → chosen → capabilities → permission"
        );

        SkillDecision {
            goal: goal.to_string(),
            candidates,
            skipped,
            chosen,
            capabilities,
        }
    }
}

/// Fallback when skill loading is unavailable: identical to the planner's
/// default allow-all policy (see `friday_planner`'s private `AllowAllPolicy`),
/// so a broken skill setup never strands a goal.
struct AllowAllFallback;

impl PolicyAuthority for AllowAllFallback {
    fn allows(&self, _tool: &str, _action: &str) -> bool {
        true
    }
}

/// Boot the full planner stack with the Skill Engine wired in:
///
/// 1. registers the planner engines (rule/LLM + supervisor) exactly like
///    [`friday_planner::register_engines_with_llm`],///    2. loads `skill_dirs` against the live capability registry,
///    3. swaps the supervisor's policy for the skill-aware
///    [`SkillPolicyAuthority`],
///    4. wires the loaded registry as the planning engine's skill context
///    source, so every LLM plan's `PlanContext` carries the skills ranked
///    against the goal (`specs/skill-engine.md` §6 plan-time view).
///
/// Returns the shared capability registry (for MCP/plugin registration) and
/// the [`SkillEngineWiring`]. A `None` wiring means *every* skill directory
/// was unreadable and the supervisor runs with the allow-all policy (a
/// warning is logged, the boot never fails); partial failures are surfaced in
/// `wiring.dir_errors` with the remaining directories still loaded.
pub async fn register_engines_with_skills(
    friday: &mut Friday,
    backend: Option<Box<dyn ReasoningBackend>>,
    skill_dirs: &[SkillDir],
) -> Result<(Arc<PlannerCapabilityRegistry>, Option<SkillEngineWiring>), String> {
    let holder: Arc<Mutex<Option<SkillEngineWiring>>> = Arc::new(Mutex::new(None));
    let holder_for_policy = Arc::clone(&holder);

    let registry = friday_planner::register_engines_with_policy(friday, backend, move |registry| {
        let wiring = load_skill_engine(Arc::clone(&registry), skill_dirs);
        if wiring.registry.all().is_empty() {
            // Nothing loaded — skill policy would gate nothing anyway.
            // Fall back to the allow-all policy so skills can't strand a
            // goal via an effectively-empty registry.
            if !wiring.dir_errors.is_empty() {
                tracing::warn!(
                    dir_errors = ?wiring.dir_errors,
                    "skill engine loaded no skills; supervisor falls back to allow-all policy"
                );
            }
            return friday_planner::PolicyBinding {
                policy: Arc::new(AllowAllFallback),
                skills: None,
                investigation: None,
                completion_sink: None,
            };
        }
        let completion_sink: Arc<dyn GoalCompletionSink> =
            Arc::new(SkillLearningObserver::new(Arc::new(wiring.skill_learning())));
        let policy: Arc<dyn PolicyAuthority> =
            Arc::new(SkillPolicyAuthority(Arc::clone(&wiring.policy)));
        // The loaded registry also feeds the planning engine: every goal's
        // `PlanContext` (and thus the LLM planner's prompt) now carries the
        // skills ranked against it. One registry, two consumers.
        let skills: Option<Arc<dyn SkillContextSource>> =
            Some(Arc::clone(&wiring.registry) as Arc<dyn SkillContextSource>);
        *holder_for_policy.lock().expect("skill wiring poisoned") = Some(wiring);
        friday_planner::PolicyBinding {
            policy,
            skills,
            investigation: None,
            completion_sink: Some(completion_sink),
        }
    })
    .await
    .map_err(|e| e.to_string())?;

    let wiring = holder.lock().expect("skill wiring poisoned").take();
    if let Some(wiring) = &wiring {
        if !wiring.all_loaded() {
            tracing::warn!(
                loaded = wiring.reports.iter().map(|r| r.loaded).sum::<usize>(),
                failed = wiring.reports.iter().map(|r| r.failed.len()).sum::<usize>(),
                dir_errors = wiring.dir_errors.len(),
                "skill engine loaded with some failures"
            );
        }
    }

    Ok((registry, wiring))
}
