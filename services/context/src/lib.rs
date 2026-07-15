use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use friday_capabilities::model::Capability;
use friday_capabilities::orchestrator::{ServiceOrchestrator, ServiceState};
use friday_capabilities::registry::CapabilityRegistry;
use friday_core::memory::{ExecutionExperience, MemoryEngine, MemoryEntry, MemoryLayerKind};

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
}

pub struct ContextBuilder {
    registry: Arc<CapabilityRegistry>,
    orchestrator: Option<Arc<ServiceOrchestrator>>,
    memory: Arc<MemoryEngine>,
    experience_db_path: std::path::PathBuf,
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
        }
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
        })
    }
}
