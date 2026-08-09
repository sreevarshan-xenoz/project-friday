use std::sync::Arc;

use async_trait::async_trait;
use friday_core::event::Event;
use friday_core::plugin::{Plugin, PluginMetadata};

use crate::events;
use crate::execution::capability_registry::CapabilityRegistry;
use crate::execution::lifecycle::ExecutionBudget;
use crate::intent::{parse, Intent};
use crate::planning::planners::backend::ReasoningBackend;
use crate::planning::planners::context::{
    AvailableCapability, PlanContext, RiskPolicy, WorkspaceContext,
};
use crate::planning::planners::router::{PlannerKind, PlannerRouter};
use crate::planning::planners::rule::RulePlanner;
use crate::planning::planners::Planner;
use friday_investigation::{InvestigationEngine, InvestigationResult};
use friday_skills::SkillContextSource;

/// Per-plan telemetry record. Emitted as a `planning.telemetry` event so the
/// system can compare rule vs LLM planning on evidence (planning time, validation
/// failures, outcome) rather than intuition.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PlanningTelemetry {
    pub trace_id: String,
    pub planner: String,
    pub planning_ms: u64,
    pub task_count: usize,
    pub capabilities_offered: usize,
    pub validation_failures: usize,
    pub fell_back_to_rule: bool,
    pub success: bool,
}

/// The Intent Engine. Its only job is understanding the request: it turns free
/// text into a structured [`Intent`]. It performs no planning and no execution.
pub struct IntentEngine {
    meta: PluginMetadata,
}

impl Default for IntentEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl IntentEngine {
    pub fn new() -> Self {
        let meta = PluginMetadata::builder("intent", "Intent Engine")
            .description("Resolves a raw user request into a structured goal")
            .subscribes(&[events::GOAL_RAW])
            .emits(&[events::INTENT_CREATED])
            .build();
        Self { meta }
    }
}

#[async_trait]
impl Plugin for IntentEngine {
    fn metadata(&self) -> &PluginMetadata {
        &self.meta
    }

    async fn handle_event(&self, event: &Event) -> friday_core::Result<Vec<Event>> {
        let raw = event
            .payload
            .get("text")
            .and_then(|v| v.as_str())
            .or_else(|| event.payload.as_str())
            .unwrap_or("")
            .to_string();
        let intent = parse(&raw);
        tracing::info!(intent = %intent.intent, "intent resolved");
        Ok(vec![events::event(
            events::INTENT_CREATED,
            "intent",
            serde_json::to_value(&intent)?,
            event.trace_id,
        )])
    }
}

/// The Planning Engine. It owns a [`PlannerRouter`] and turns an [`Intent`] into
/// an executable [`TaskGraph`] — choosing rule vs LLM per goal. It never
/// executes anything, and the supervisor downstream cannot tell which planner
/// produced the graph.
pub struct PlanningEngine {
    meta: PluginMetadata,
    router: PlannerRouter,
    registry: Arc<CapabilityRegistry>,
    budget: ExecutionBudget,
    policy: RiskPolicy,
    workspace: WorkspaceContext,
    /// Max capabilities surfaced to the LLM planner via discovery (keeps prompts
    /// short). Ignored by the rule planner.
    discovery_limit: usize,
    /// Optional experience provider for evidence-enriched planning.
    experience: Option<Arc<dyn crate::experience::ExperienceProvider + Send + Sync>>,
    /// Optional skill source (the loaded [`SkillRegistry`](friday_skills::SkillRegistry)).
    /// When present, `context()` enriches every [`PlanContext`] with the skills
    /// ranked against the goal so the LLM planner reasons at the skill level.
    skills: Option<Arc<dyn SkillContextSource>>,
    /// Optional investigation engine. When present, `context()` runs it for
    /// LLM-routed goals and enriches every [`PlanContext`] with the channels
    /// the goal requires evidence from, so the LLM planner's prompt carries
    /// the EVIDENCE REQUIRED section. The rule planner ignores the field.
    investigation: Option<InvestigationEngine>,
}

impl PlanningEngine {
    /// Construct with an explicit router, registry, budget and policy. Use this
    /// when an LLM backend is available (pass `Some(backend)` to enable the
    /// [`crate::planning::planners::LlmPlanner`]).
    pub fn with_router(
        router: PlannerRouter,
        registry: Arc<CapabilityRegistry>,
        budget: ExecutionBudget,
        policy: RiskPolicy,
        workspace: WorkspaceContext,
    ) -> Self {
        Self::with_router_and_limit(router, registry, budget, policy, workspace, 12)
    }

    /// Like [`PlanningEngine::with_router`] but with an explicit discovery limit.
    pub fn with_router_and_limit(
        router: PlannerRouter,
        registry: Arc<CapabilityRegistry>,
        budget: ExecutionBudget,
        policy: RiskPolicy,
        workspace: WorkspaceContext,
        discovery_limit: usize,
    ) -> Self {
        let meta = PluginMetadata::builder("planning", "Planning Engine")
            .description("Decomposes an intent into an executable task graph (rule or LLM)")
            .subscribes(&[events::INTENT_CREATED])
            .emits(&[events::PLAN_CREATED, events::PLANNING_TELEMETRY])
            .build();
        Self {
            meta,
            router,
            registry,
            budget,
            policy,
            workspace,
            discovery_limit,
            experience: None,
            skills: None,
            investigation: None,
        }
    }

    /// Set the experience provider for evidence-enriched planning.
    pub fn with_experience(
        mut self,
        provider: Arc<dyn crate::experience::ExperienceProvider + Send + Sync>,
    ) -> Self {
        self.experience = Some(provider);
        self
    }

    /// Attach a skill source so every plan's [`PlanContext`] carries the
    /// skills ranked against the goal (the loaded `SkillRegistry` implements
    /// [`SkillContextSource`]). The LLM planner then reasons at the skill
    /// level; the rule planner ignores the field entirely.
    pub fn with_skill_context(mut self, source: Arc<dyn SkillContextSource>) -> Self {
        self.skills = Some(source);
        self
    }

    /// Attach an investigation engine so every LLM-routed goal's
    /// [`PlanContext`] carries the channels it requires evidence from — the
    /// EVIDENCE REQUIRED section in the LLM planner's prompt. The rule
    /// planner ignores the field entirely; a broken or absent engine yields
    /// `None`, never a stranded goal.
    ///
    /// **Cost note:** `investigate` runs once per LLM-routed goal. That is
    /// free with the heuristic strategy (deterministic, returns immediately),
    /// but a model-backed [`LlmInvestigationStrategy`](friday_investigation::LlmInvestigationStrategy)
    /// pays a model call per LLM-routed goal — even one that turns out
    /// self-contained (empty plan renders nothing). Inherent to the seam (you
    /// must investigate to know); don't "optimize" the empty-plan rendering
    /// into a premature skip of the call.
    pub fn with_investigation_engine(mut self, engine: InvestigationEngine) -> Self {
        self.investigation = Some(engine);
        self
    }

    /// Default: rule-only planner, no LLM. Keeps `register_engines` deterministic
    /// and dependency-free.
    pub fn new() -> Self {
        Self::with_router(
            PlannerRouter::rule_only(Arc::new(RulePlanner::new())),
            Arc::new(CapabilityRegistry::new()),
            ExecutionBudget::default(),
            RiskPolicy::default(),
            WorkspaceContext::default(),
        )
    }

    /// Helper to build the grounding context for a planner from the registry.
    /// Uses capability **discovery** (`registry.search`) so the LLM sees only the
    /// top-`discovery_limit` most relevant capabilities (shorter prompts, lower
    /// token cost, better accuracy) — native *and* MCP/plugin tools appear
    /// automatically once registered. No provenance filter is applied: a goal
    /// may legitimately need native tools, and discovery already excludes
    /// offline capabilities.
    ///
    /// Async because the investigation seam is async (a model-backed strategy
    /// is genuinely async); the default heuristic returns immediately.
    async fn context(&self, intent: &Intent) -> PlanContext {
        let ranked = self.registry.search(&intent.raw, self.discovery_limit * 3);
        let mut capabilities: Vec<AvailableCapability> = ranked
            .into_iter()
            .take(self.discovery_limit)
            .map(|rc| AvailableCapability {
                id: rc.spec.id,
                description: rc.spec.description,
                risk: rc.spec.binding.risk,
                score: rc.score,
                schema: rc.spec.binding.schema,
                preferred: false,
            })
            .collect();

        // Build experience context if a provider is wired.
        let experience = self.experience.as_ref().map(|provider| {
            let cap_ids: Vec<String> = capabilities.iter().map(|c| c.id.clone()).collect();
            crate::experience::context::ExperienceContextBuilder::build(
                provider.as_ref(),
                crate::experience::context::PlanningGoal {
                    id: uuid::Uuid::new_v4(),
                    title: intent.intent.clone(),
                    description: intent.raw.clone(),
                },
                cap_ids.clone(),
                vec![],
                &cap_ids,
            )
        });

        // Skill-aware context: the skills ranked against this goal (top
        // `discovery_limit`, consistent with capability discovery). Advisory —
        // a broken skill setup yields `None`, never a stranded goal. Gated to
        // goals the router will hand to the LLM planner: the rule planner
        // ignores `skills`, so ranking for a rule-planned goal is wasted work.
        let skills = if matches!(self.router.kind_for(intent), PlannerKind::Llm) {
            self.skills
                .as_ref()
                .map(|source| source.context_for(&intent.raw, self.discovery_limit))
        } else {
            None
        };

        // Investigation outcome: the channels this goal requires evidence
        // from, wrapped in the always-present `InvestigationResult`. Advisory
        // like skills — a missing/broken engine yields `None`, never a
        // stranded goal. Gated to LLM-routed goals for the same reason as
        // skills (the rule planner ignores it; a model-backed strategy is not
        // free).
        let investigation = if matches!(self.router.kind_for(intent), PlannerKind::Llm) {
            match &self.investigation {
                Some(engine) => Some(InvestigationResult::Complete(
                    engine.investigate(&intent.raw).await,
                )),
                None => None,
            }
        } else {
            None
        };

        // Skill-first capability scoping: when a skill is chosen for the goal,
        // its declared capabilities lead the offered list (marked `preferred`,
        // ensured present even if discovery ranked them lower) so the model
        // plans *through* the skill. Discovery still follows, so a goal that
        // needs cross-skill capabilities keeps them reachable. Capabilities a
        // skill declares but that are not registered are skipped (offering an
        // unexecutable id would poison the plan).
        if let Some(skill_ctx) = &skills {
            if let Some(active) = &skill_ctx.active_skill {
                if let Some(summary) = skill_ctx.relevant_skills.iter().find(|s| &s.id == active) {
                    for cap_id in &summary.capabilities {
                        if let Some(cap) = capabilities.iter_mut().find(|c| &c.id == cap_id) {
                            cap.preferred = true;
                        } else if let Some(spec) = self.registry.spec(cap_id) {
                            capabilities.push(AvailableCapability {
                                id: spec.id,
                                description: spec.description,
                                risk: spec.binding.risk,
                                score: 1.0,
                                schema: spec.binding.schema,
                                preferred: true,
                            });
                        }
                    }
                    // Stable sort: preferred first, then discovery score.
                    capabilities.sort_by(|a, b| {
                        b.preferred.cmp(&a.preferred).then_with(|| {
                            b.score
                                .partial_cmp(&a.score)
                                .unwrap_or(std::cmp::Ordering::Equal)
                        })
                    });
                }
            }
        }

        PlanContext {
            intent_raw: intent.raw.clone(),
            capabilities,
            budget: self.budget,
            workspace: self.workspace.clone(),
            risk_policy: self.policy.clone(),
            experience,
            skills,
            investigation,
        }
    }
}

impl Default for PlanningEngine {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Plugin for PlanningEngine {
    fn metadata(&self) -> &PluginMetadata {
        &self.meta
    }

    async fn handle_event(&self, event: &Event) -> friday_core::Result<Vec<Event>> {
        let intent: Intent = serde_json::from_value(event.payload.clone())
            .map_err(|e| friday_core::FridayError::Internal(format!("invalid intent: {e}")))?;
        let ctx = self.context(&intent).await;
        let caps_offered = ctx.capabilities.len();
        let planner = self.router.select(&intent);
        let kind = self.router.kind_for(&intent);
        let start = std::time::Instant::now();

        let plan_result = planner.create_plan(&intent, &ctx).await;
        let (graph, validation_failures, fell_back) = match plan_result {
            Ok(g) => (g, 0usize, false),
            Err(e) => {
                tracing::error!(error = %e, "planning failed; falling back to rule planner");
                // Deterministic fallback so the goal is never stranded.
                let fb = RulePlanner::new().create_plan(&intent, &ctx).await;
                match fb {
                    Ok(g) => (g, count_validation_issues(&e), true),
                    Err(fe) => {
                        return Err(friday_core::FridayError::Internal(format!(
                            "planning failed: {fe}"
                        )));
                    }
                }
            }
        };
        let planning_ms = start.elapsed().as_millis() as u64;

        tracing::info!(
            plan_id = %graph.plan_id,
            tasks = graph.tasks.len(),
            planner = ?kind,
            planning_ms = planning_ms,
            "plan created"
        );

        let telemetry = PlanningTelemetry {
            trace_id: event.trace_id.to_string(),
            planner: format!("{kind:?}"),
            planning_ms,
            task_count: graph.tasks.len(),
            capabilities_offered: caps_offered,
            validation_failures,
            fell_back_to_rule: fell_back,
            success: true,
        };

        Ok(vec![
            events::event(
                events::PLAN_CREATED,
                "planning",
                serde_json::to_value(&graph)?,
                event.trace_id,
            ),
            events::event(
                events::PLANNING_TELEMETRY,
                "planning",
                serde_json::to_value(&telemetry)?,
                event.trace_id,
            ),
        ])
    }
}

/// Best-effort extraction of the number of validation issues from a plan error,
/// for telemetry. Returns 0 when the error was not a validation error (e.g. a
/// backend/parse failure).
pub fn count_validation_issues(e: &crate::planning::planners::PlanError) -> usize {
    match e {
        crate::planning::planners::PlanError::Validation(issues) => issues.len(),
        _ => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::intent::parse;

    fn engine_with_natives() -> PlanningEngine {
        let registry = Arc::new(CapabilityRegistry::new());
        crate::execution::native_caps::register_native_builtins(&registry);
        PlanningEngine::with_router(
            PlannerRouter::rule_only(Arc::new(RulePlanner::new())),
            registry,
            ExecutionBudget::default(),
            RiskPolicy::default(),
            WorkspaceContext::default(),
        )
    }

    #[tokio::test]
    async fn llm_context_surfaces_relevant_native_capabilities() {
        // Regression: the LLM planner must see native capabilities. An
        // `mcp:`-only filter previously offered an EMPTY capability list to
        // any setup without MCP servers, so the model could never emit a
        // valid plan (every plan fell back to the rule planner).
        let engine = engine_with_natives();
        let intent = parse("Check the git status and list the files");
        let ctx = engine.context(&intent).await;

        assert!(
            !ctx.capabilities.is_empty(),
            "LLM context must include native capabilities"
        );
        assert!(ctx.capabilities.len() <= engine.discovery_limit);
        let ids: Vec<&str> = ctx.capabilities.iter().map(|c| c.id.as_str()).collect();
        assert!(ids.contains(&"git.status"), "ids: {ids:?}");
        assert!(ids.contains(&"filesystem.list_directory"), "ids: {ids:?}");
    }

    fn skill(id: &str) -> friday_skills::Skill {
        friday_skills::Skill {
            manifest: friday_skills::SkillManifest {
                id: id.to_string(),
                name: id.to_string(),
                version: "1.0.0".to_string(),
                description: format!("{id} skill"),
                author: "test".to_string(),
                category: "developer".to_string(),
                icon: String::new(),
                capabilities: vec!["filesystem.read_file".to_string()],
                actions: vec!["read files".to_string()],
                permissions: vec!["filesystem.read_file".to_string()],
                readonly: true,
                dependencies: vec![],
                platform: "Any".to_string(),
                min_core_version: "0.0.0".to_string(),
                enabled: true,
                memory: Default::default(),
            },
            source: friday_skills::SkillSource::Builtin,
            loaded_at: chrono::Utc::now(),
            enabled: true,
            capabilities: vec![friday_skills::CapabilityStatus::Resolved(
                "filesystem.read_file".to_string(),
            )],
        }
    }

    /// An LLM-enabled engine (the skill context only feeds LLM-routed goals).
    fn llm_engine() -> PlanningEngine {
        let registry = Arc::new(CapabilityRegistry::new());
        crate::execution::native_caps::register_native_builtins(&registry);
        PlanningEngine::with_router(
            PlannerRouter::new(
                Arc::new(crate::planning::planners::rule::RulePlanner::new()),
                Some(Arc::new(crate::planning::planners::llm::LlmPlanner::new(
                    Box::new(crate::planning::planners::backend::MockBackend::new("{}")),
                    Arc::clone(&registry),
                    ExecutionBudget::default(),
                    RiskPolicy::default(),
                ))),
            ),
            registry,
            ExecutionBudget::default(),
            RiskPolicy::default(),
            WorkspaceContext::default(),
        )
    }

    #[tokio::test]
    async fn context_carries_skills_ranked_against_goal() {
        // A wired skill source enriches an LLM-routed PlanContext with the
        // skills ranked against the goal + the deterministic best usable skill.
        let registry = friday_skills::SkillRegistry::new();
        registry.register(skill("reader")).unwrap();

        let engine = llm_engine().with_skill_context(Arc::new(registry));
        let intent = parse("read my files");
        let ctx = engine.context(&intent).await;

        let skills = ctx.skills.expect("skill context must be populated");
        assert!(skills.relevant_skills.iter().any(|s| s.id == "reader"));
        assert_eq!(skills.active_skill.as_deref(), Some("reader"));
    }

    #[tokio::test]
    async fn context_without_skill_source_is_graceful() {
        // No source → `skills` is None; a broken skill setup never strands a
        // goal (the planner keeps working capability-only).
        let engine = llm_engine();
        let intent = parse("read my files");
        let ctx = engine.context(&intent).await;
        assert!(ctx.skills.is_none());
    }

    #[tokio::test]
    async fn context_carries_investigation_for_llm_goals() {
        // A wired investigation engine enriches an LLM-routed PlanContext
        // with the channels the goal requires evidence from, wrapped in the
        // always-present InvestigationResult.
        let engine = llm_engine().with_investigation_engine(InvestigationEngine::heuristic());
        // `research` is a complex keyword → LLM-routed, and the heuristic's
        // web trigger ("research", 0.7) fires the web channel. Coupled to the
        // heuristic's keyword table: if those change, assert on
        // `needs_investigation()` + `Complete` instead of the specific channel.
        let intent = parse("Research the latest Rust async docs");
        let ctx = engine.context(&intent).await;

        let result = ctx.investigation.expect("investigation must be populated");
        let plan = result.plan().expect("heuristic always completes");
        assert!(
            plan.channels().iter().any(|c| c.label() == "web"),
            "web channel must be required: {:?}",
            plan.channels()
                .iter()
                .map(|c| c.label())
                .collect::<Vec<_>>()
        );
    }

    #[tokio::test]
    async fn context_without_investigation_engine_is_graceful() {
        // No engine → `investigation` is None; a missing seam never strands a
        // goal (the LLM planner plans capability-only).
        let engine = llm_engine();
        let intent = parse("Research the latest Rust async docs");
        let ctx = engine.context(&intent).await;
        assert!(ctx.investigation.is_none());
    }

    #[tokio::test]
    async fn context_skips_investigation_for_rule_planned_goals() {
        // A rule-routed goal (trivial intent) must not pay for an
        // investigation pass it will ignore — same gate as skills.
        let engine = llm_engine().with_investigation_engine(InvestigationEngine::heuristic());
        let intent = parse("Open Chrome and search Tesla"); // trivial → rule
        let ctx = engine.context(&intent).await;
        assert!(
            ctx.investigation.is_none(),
            "rule goals must skip the investigation pass"
        );
    }

    #[tokio::test]
    async fn context_prefers_chosen_skill_capabilities() {
        // Skill-first scoping: the chosen skill's declared capabilities lead
        // the offered list (marked preferred) so the model plans *through*
        // the skill; other discovery-ranked capabilities stay reachable.
        let registry = friday_skills::SkillRegistry::new();
        registry.register(skill("reader")).unwrap();

        let engine = llm_engine().with_skill_context(Arc::new(registry));
        let intent = parse("read my files");
        let ctx = engine.context(&intent).await;

        assert_eq!(
            ctx.skills.as_ref().unwrap().active_skill.as_deref(),
            Some("reader")
        );
        assert_eq!(
            ctx.capabilities[0].id,
            "filesystem.read_file",
            "chosen skill's capability must lead the list: {:?}",
            ctx.capabilities
                .iter()
                .map(|c| c.id.as_str())
                .collect::<Vec<_>>()
        );
        assert!(ctx.capabilities[0].preferred);
        // Non-declared capabilities are not filtered out.
        assert!(ctx.capabilities.len() > 1);
        assert!(
            ctx.capabilities.iter().skip(1).any(|c| !c.preferred),
            "discovery-ranked capabilities must remain after the preferred ones"
        );
    }

    #[tokio::test]
    async fn context_skips_skill_ranking_for_rule_planned_goals() {
        // A rule-routed goal (trivial intent) must not pay for a skill ranking
        // pass it will ignore — the reviewer's per-goal waste finding.
        let registry = friday_skills::SkillRegistry::new();
        registry.register(skill("reader")).unwrap();

        let engine = llm_engine().with_skill_context(Arc::new(registry));
        let intent = parse("Open Chrome and search Tesla"); // trivial → rule
        let ctx = engine.context(&intent).await;
        assert!(ctx.skills.is_none(), "rule goals must skip the skill pass");
    }
}

/// Build a [`PlanningEngine`] wired to the given capability registry and an
/// optional LLM backend (e.g. an adapter over `InferenceFabric`). When `backend`
/// is `None` the engine is rule-only; when `Some`, the router uses the LLM
/// planner for non-trivial intents.
pub fn planning_engine_with(
    registry: Arc<CapabilityRegistry>,
    backend: Option<Box<dyn ReasoningBackend>>,
    budget: ExecutionBudget,
    policy: RiskPolicy,
    workspace: WorkspaceContext,
) -> PlanningEngine {
    let rule = Arc::new(RulePlanner::new());
    let llm = backend.map(|b| {
        Arc::new(crate::planning::planners::llm::LlmPlanner::new(
            b,
            registry.clone(),
            budget,
            policy.clone(),
        )) as Arc<dyn Planner>
    });
    PlanningEngine::with_router(
        PlannerRouter::new(rule, llm),
        registry,
        budget,
        policy,
        workspace,
    )
}
