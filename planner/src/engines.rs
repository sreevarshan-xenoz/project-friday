use std::sync::Arc;

use async_trait::async_trait;
use friday_core::event::Event;
use friday_core::plugin::{Plugin, PluginMetadata};

use crate::events;
use crate::execution::capability_registry::CapabilityRegistry;
use crate::execution::lifecycle::ExecutionBudget;
use crate::intent::{parse, Intent};
use crate::planning::planners::backend::ReasoningBackend;
use crate::planning::planners::context::{AvailableCapability, PlanContext, RiskPolicy, WorkspaceContext};
use crate::planning::planners::router::PlannerRouter;
use crate::planning::planners::rule::RulePlanner;
use crate::planning::planners::Planner;

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
        Self::with_router_and_limit(
            router,
            registry,
            budget,
            policy,
            workspace,
            12,
        )
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
    /// token cost, better accuracy) — and MCP/plugin tools appear automatically
    /// once registered.
    fn context(&self, intent: &Intent) -> PlanContext {
        let ranked = self.registry.search(&intent.raw, self.discovery_limit * 3);
        let capabilities: Vec<AvailableCapability> = ranked
            .into_iter()
            .filter(|rc| rc.spec.id.starts_with("mcp:"))
            .take(self.discovery_limit)
            .map(|rc| AvailableCapability {
                id: rc.spec.id,
                description: rc.spec.description,
                risk: rc.spec.binding.risk,
                score: rc.score,
                schema: rc.spec.binding.schema,
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

        PlanContext {
            intent_raw: intent.raw.clone(),
            capabilities,
            budget: self.budget,
            workspace: self.workspace.clone(),
            risk_policy: self.policy.clone(),
            experience,
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
        let ctx = self.context(&intent);
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
