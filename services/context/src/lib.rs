use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use friday_capabilities::model::Capability;
use friday_capabilities::orchestrator::{ServiceOrchestrator, ServiceState};
use friday_capabilities::registry::CapabilityRegistry;
use friday_core::memory::{ExecutionExperience, MemoryEngine, MemoryEntry, MemoryLayerKind};
use friday_planner::execution::{
    CapabilityRegistry as PlannerCapabilityRegistry, PolicyAuthority, Risk,
};
use friday_skills::{
    CapHealth, CapabilityHealthSource, CapabilityResolver, MemoryHit, RiskResolver,
    SkillContextSource, SkillMemorySource, SkillPolicy, SkillSummary,
};

pub mod skill_learning;
pub mod skills;

pub use friday_skills::SkillSource;
pub use skill_learning::{LearnError, LearnedCandidate, SkillLearningService};
pub use skills::{
    load_skill_engine, register_engines_with_skills, SkillDecision, SkillDecisionCapability,
    SkillDir, SkillEngineWiring, SkillPermission,
};

/// Structured, fully packaged context sent to the Intelligence Layer.
/// The LLM never performs retrieval; the Context Builder does.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentContext {
    pub timestamp: DateTime<Utc>,
    pub goal: String,
    pub capabilities: Vec<Capability>,
    pub healthy_services: Vec<String>,
    pub relevant_memories: Vec<MemoryEntry>,
    pub experiences: Vec<ExecutionExperience>,
    pub current_project: String,
    pub active_sessions: Vec<String>,
    pub active_windows: Vec<String>,
    /// Relevant skill summaries (Skill Engine M5.5) — populated when the
    /// builder is wired with a [`SkillContextSource`].
    #[serde(default)]
    pub skills: Vec<SkillSummary>,
    /// The deterministic best usable skill for the goal (M5 battle-tested),
    /// if any. Surfaced so the planner knows *which* skill is active — the
    /// ranked scores stay in the trace, this is just the chosen id.
    #[serde(default)]
    pub active_skill: Option<String>,
}

pub struct ContextBuilder {
    registry: Arc<CapabilityRegistry>,
    orchestrator: Option<Arc<ServiceOrchestrator>>,
    memory: Arc<MemoryEngine>,
    experience_db_path: std::path::PathBuf,
    skills: Option<Arc<dyn SkillContextSource>>,
}

impl ContextBuilder {
    pub fn new(
        registry: Arc<CapabilityRegistry>,
        orchestrator: Option<Arc<ServiceOrchestrator>>,
        memory: Arc<MemoryEngine>,
        experience_db_path: std::path::PathBuf,
    ) -> Self {
        Self {
            registry,
            orchestrator,
            memory,
            experience_db_path,
            skills: None,
        }
    }

    /// Wire a skill context source (e.g. the `SkillRegistry`) so relevant
    /// skill summaries are included in every built `AgentContext`.
    pub fn with_skills(mut self, skills: Arc<dyn SkillContextSource>) -> Self {
        self.skills = Some(skills);
        self
    }

    /// Build the complete AgentContext, retrieving and pruning to fit token limits.
    pub async fn build(&self, goal: &str) -> Result<AgentContext, String> {
        // 1. Fetch available and ranked capabilities
        let all_caps = self.registry.all();
        let mut capabilities = Vec::new();
        // Extract capabilities and filter only available ones
        for (_provider, cap) in all_caps {
            if self.registry.available(&cap.id) {
                capabilities.push(cap);
            }
        }
        // Deduplicate and rank by score (simplistic ranking for context formatting)
        capabilities.sort_by(|a, b| {
            let score_a = a.reliability * 100.0 - (a.latency_ms as f64 * 0.05);
            let score_b = b.reliability * 100.0 - (b.latency_ms as f64 * 0.05);
            score_b
                .partial_cmp(&score_a)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        capabilities.truncate(15); // Budget limit: Top 15 capabilities

        // 2. Fetch active/healthy services
        let mut healthy_services = Vec::new();
        if let Some(orch) = &self.orchestrator {
            for service in ["filesystem", "terminal", "git", "windows"] {
                if orch.state(service) == Some(ServiceState::Healthy) {
                    healthy_services.push(service.to_string());
                }
            }
        } else {
            // Fallback: read directly from registry
            for service in ["filesystem", "terminal", "git", "windows"] {
                if self.registry.available(service) {
                    healthy_services.push(service.to_string());
                }
            }
        }

        // 3. Retrieve relevant memories via lexical search/scanning
        let mut relevant_memories = Vec::new();
        if let Ok(mems) = self.memory.search(MemoryLayerKind::LongTerm, goal, 5).await {
            relevant_memories.extend(mems);
        }
        if let Ok(mems) = self.memory.search(MemoryLayerKind::Session, goal, 5).await {
            relevant_memories.extend(mems);
        }
        relevant_memories.truncate(8); // Budget limit: Top 8 memories

        // 4. Retrieve past experiences from ExperienceEngine database
        let mut experiences = Vec::new();
        if let Ok(engine) =
            friday_core::memory::experience::ExperienceEngine::open(&self.experience_db_path)
        {
            if let Ok(exps) = engine.query_by_intent(goal) {
                experiences = exps;
                experiences.truncate(3); // Budget limit: Top 3 relevant experiences
            }
        }

        // 5. Detect current workspace / project info
        let current_project = std::env::current_dir()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|_| "unknown".to_string());

        // 6. Gather session/window stubs (mocked or propped from active environment)
        let active_sessions = vec!["default-session".to_string()];
        let active_windows = vec!["VS Code".to_string(), "Terminal".to_string()];

        // 7. Relevant skills (Skill Engine) — empty when no source is wired.
        //    One `context_for` call feeds both the summaries and the chosen
        //    skill id so the active-skill selection stays consistent.
        let skill_ctx = self
            .skills
            .as_ref()
            .map(|source| source.context_for(goal, 5));
        let skills = skill_ctx
            .as_ref()
            .map(|ctx| ctx.relevant_skills.clone())
            .unwrap_or_default();
        let active_skill = skill_ctx.as_ref().and_then(|ctx| ctx.active_skill.clone());

        Ok(AgentContext {
            timestamp: Utc::now(),
            goal: goal.to_string(),
            capabilities,
            healthy_services,
            relevant_memories,
            experiences,
            current_project,
            active_sessions,
            active_windows,
            skills,
            active_skill,
        })
    }
}

/// Adapter from the planner's `CapabilityRegistry` to the Skill Engine's
/// abstract [`CapabilityResolver`]. `exists` = a canonical id resolves to a
/// binding.
pub struct PlannerRegistryResolver(pub Arc<PlannerCapabilityRegistry>);

impl CapabilityResolver for PlannerRegistryResolver {
    fn exists(&self, capability: &str) -> bool {
        self.0.resolve(capability).is_some()
    }
}

/// Adapter mapping the planner's [`Risk`] classification to the Skill Engine's
/// safe/unsafe question (`Risk::Safe` → safe).
pub struct RegistryRiskResolver(pub Arc<PlannerCapabilityRegistry>);

impl RiskResolver for RegistryRiskResolver {
    fn is_safe(&self, capability: &str) -> bool {
        self.0.risk_of(capability) == Risk::Safe
    }
}

/// Adapter from the runtime [`MemoryEngine`] to the Skill Engine's abstract
/// [`SkillMemorySource`] (M8). Stores land in the **LongTerm** layer; search
/// covers LongTerm + Session; keyed reads hit LongTerm. Wire it into the
/// wiring with `SkillEngineWiring::with_memory` so skills get a per-skill
/// gated handle via `SkillEngineWiring::memory_for`.
pub struct CoreMemorySource(pub Arc<MemoryEngine>);

#[async_trait::async_trait]
impl SkillMemorySource for CoreMemorySource {
    async fn search(&self, query: &str, limit: usize) -> Vec<MemoryHit> {
        let mut hits = Vec::new();
        for layer in [MemoryLayerKind::LongTerm, MemoryLayerKind::Session] {
            if let Ok(entries) = self.0.search(layer, query, limit).await {
                hits.extend(entries.into_iter().map(|e| MemoryHit {
                    key: e.key.clone(),
                    value: hit_value(&e.content),
                }));
            }
        }
        hits.truncate(limit);
        hits
    }

    async fn read(&self, key: &str) -> Option<MemoryHit> {
        self.0
            .retrieve(MemoryLayerKind::LongTerm, key)
            .await
            .ok()
            .flatten()
            .map(|e| MemoryHit {
                key: e.key.clone(),
                value: hit_value(&e.content),
            })
    }

    async fn store(&self, key: &str, value: &str) -> Result<(), String> {
        let entry = MemoryEntry::new(MemoryLayerKind::LongTerm, key, serde_json::json!(value));
        self.0.store(&entry).await.map_err(|e| e.to_string())
    }
}

/// Plain string contents round-trip as themselves (no JSON quoting); anything
/// richer (objects, numbers) serializes.
fn hit_value(content: &serde_json::Value) -> String {
    match content {
        serde_json::Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

/// Adapter from the planner registry's live `CapabilityHealth` map to the
/// Skill Engine's abstract [`CapabilityHealthSource`]. Wire this into
/// [`SkillEngineWiring::health`] / [`SkillEngineWiring::health_of`] so a
/// capability that goes `Offline` at runtime (MCP polling, plugin probes) is
/// immediately reflected as a skill-level `Warning`.
pub struct RegistryHealthSource(pub Arc<PlannerCapabilityRegistry>);

impl CapabilityHealthSource for RegistryHealthSource {
    fn health_of(&self, capability: &str) -> CapHealth {
        match self.0.health_of(capability) {
            friday_capabilities::spec::CapabilityHealth::Healthy => CapHealth::Healthy,
            friday_capabilities::spec::CapabilityHealth::Degraded => CapHealth::Degraded,
            friday_capabilities::spec::CapabilityHealth::Offline => CapHealth::Offline,
        }
    }
}

/// Adapter from [`SkillPolicy`] to the planner's [`PolicyAuthority`] trait, so
/// the Skill Engine can act as a permission policy source in the supervisor.
pub struct SkillPolicyAuthority(pub Arc<SkillPolicy>);

impl PolicyAuthority for SkillPolicyAuthority {
    fn allows(&self, tool: &str, action: &str) -> bool {
        let capability = format!("{tool}.{action}");
        // The Skill Engine is authoritative only for capabilities declared by a
        // loaded skill; everything else defers to the previous policy chain
        // (allow-all by default), so skills never silently break un-skill'd
        // tools (e.g. MCP servers the user registered at runtime).
        if !self.0.manages_capability(&capability) {
            return true;
        }
        self.0.allows_capability(&capability)
    }
}
