//! The agent data model: what a worker is asked to do ([`AgentSpec`]), what it
//! reports back ([`AgentResult`]), and the lifecycle states in between
//! ([`AgentState`]).
//!
//! Design rule: the **planner orchestrates, the model never owns the
//! architecture**. A spec describes the *constraints* of a worker (goal,
//! allowed skills, routing preferences, budget) — it never names a provider.
//! Which model serves the worker is decided by the routing layer, exactly like
//! every other request.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::context::AgentAbort;

/// Routing *preferences* an agent's work benefits from — constraints, never
/// provider names. The fabric decides which provider actually serves the
/// worker, so swapping Gemma for Qwen/Llama/DeepSeek changes nothing here.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ModelCapability {
    Json,
    Tools,
    Vision,
    Local,
    LongContext,
    Coding,
    Reasoning,
    Fast,
}

/// What one worker agent is asked to do.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentSpec {
    /// Unique worker id within the run (also the orchestrator's handle key).
    pub id: String,
    /// The task the worker must accomplish, in natural language.
    pub goal: String,
    /// Skill ids the worker may use. Empty = all loaded skills. The runtime
    /// hands the driver a *scoped* skill registry — a worker physically cannot
    /// reach a skill outside this list (and its dependencies).
    #[serde(default)]
    pub allowed_skills: Vec<String>,
    /// Routing preferences (constraints, not model names).
    #[serde(default)]
    pub preferred_models: Vec<ModelCapability>,
    /// Memory keys/prefixes the worker's private memory may hold. Empty =
    /// no private memory. Workers never touch global long-term memory — only
    /// the planner promotes what becomes permanent.
    #[serde(default)]
    pub memory_scope: Vec<String>,
    /// Hard time limit. 0 = the runtime's default.
    #[serde(default)]
    pub time_limit_ms: u64,
    /// Step budget. 0 = unlimited.
    #[serde(default)]
    pub max_steps: u32,
    /// Per-capability JSON parameters, keyed by capability id
    /// (e.g. `{"web.fetch": {"url": "..."}}`). The driver looks up the params
    /// for the capability it is about to execute.
    #[serde(default)]
    pub params: serde_json::Map<String, serde_json::Value>,
    /// Working directory for the worker's actions. Defaults to the process cwd.
    #[serde(default)]
    pub cwd: Option<PathBuf>,
}

impl AgentSpec {
    /// Create a spec with sane defaults; only the id and goal are required.
    pub fn new(id: impl Into<String>, goal: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            goal: goal.into(),
            allowed_skills: Vec::new(),
            preferred_models: Vec::new(),
            memory_scope: Vec::new(),
            time_limit_ms: 0,
            max_steps: 0,
            params: serde_json::Map::new(),
            cwd: None,
        }
    }

    pub fn with_allowed_skills(mut self, skills: &[&str]) -> Self {
        self.allowed_skills = skills.iter().map(|s| s.to_string()).collect();
        self
    }

    pub fn with_preferred_models(mut self, models: &[ModelCapability]) -> Self {
        self.preferred_models = models.to_vec();
        self
    }

    pub fn with_memory_scope(mut self, keys: &[&str]) -> Self {
        self.memory_scope = keys.iter().map(|s| s.to_string()).collect();
        self
    }

    pub fn with_time_limit(mut self, ms: u64) -> Self {
        self.time_limit_ms = ms;
        self
    }

    pub fn with_max_steps(mut self, steps: u32) -> Self {
        self.max_steps = steps;
        self
    }

    pub fn with_param(mut self, capability: &str, params: serde_json::Value) -> Self {
        self.params.insert(capability.to_string(), params);
        self
    }

    pub fn with_cwd(mut self, cwd: PathBuf) -> Self {
        self.cwd = Some(cwd);
        self
    }
}

/// Lifecycle state of a worker agent. `Pending` → `Running` → one terminal
/// state. `Succeeded` is the only success; everything else is an observable,
/// auditable failure mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AgentState {
    Pending,
    Running,
    Succeeded,
    Failed,
    Cancelled,
    TimedOut,
    BudgetExhausted,
}

impl AgentState {
    pub fn as_str(&self) -> &'static str {
        match self {
            AgentState::Pending => "pending",
            AgentState::Running => "running",
            AgentState::Succeeded => "succeeded",
            AgentState::Failed => "failed",
            AgentState::Cancelled => "cancelled",
            AgentState::TimedOut => "timed_out",
            AgentState::BudgetExhausted => "budget_exhausted",
        }
    }

    /// `true` once the worker can never transition again.
    pub fn is_terminal(&self) -> bool {
        !matches!(self, AgentState::Pending | AgentState::Running)
    }

    pub fn is_success(&self) -> bool {
        matches!(self, AgentState::Succeeded)
    }
}

impl std::fmt::Display for AgentState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// One observable step a worker took (a capability executed, a capability
/// skipped for approval, a decision made) — the audit trail a planner merges.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentStep {
    pub index: u32,
    /// What the step did (usually a capability id, e.g. `git.status`).
    pub description: String,
    /// Outcome: `ok`, `requires-approval`, `skipped`, or `failed: <error>`.
    pub outcome: String,
    pub duration_ms: u64,
}

impl AgentStep {
    pub fn ok(index: u32, description: impl Into<String>, duration_ms: u64) -> Self {
        Self {
            index,
            description: description.into(),
            outcome: "ok".to_string(),
            duration_ms,
        }
    }

    pub fn skipped(index: u32, description: impl Into<String>, reason: impl Into<String>) -> Self {
        Self {
            index,
            description: description.into(),
            outcome: reason.into(),
            duration_ms: 0,
        }
    }

    pub fn failed(
        index: u32,
        description: impl Into<String>,
        error: impl Into<String>,
        duration_ms: u64,
    ) -> Self {
        Self {
            index,
            description: description.into(),
            outcome: format!("failed: {}", error.into()),
            duration_ms,
        }
    }
}

/// What a worker reports back to the orchestrator.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentResult {
    pub agent_id: String,
    pub state: AgentState,
    /// A human/planner-readable summary of what the worker did.
    pub summary: String,
    pub steps: Vec<AgentStep>,
    pub duration_ms: u64,
    pub error: Option<String>,
    /// The worker's structured observation payload — a generic JSON artifact
    /// the worker observed (e.g. a command's raw output). The runtime treats
    /// it as **opaque**: the parent (director) is who interprets it into a
    /// world-model observation. `None` when the worker produced nothing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub observation: Option<serde_json::Value>,
}

impl AgentResult {
    pub fn succeeded(
        agent_id: impl Into<String>,
        summary: impl Into<String>,
        steps: Vec<AgentStep>,
    ) -> Self {
        Self {
            agent_id: agent_id.into(),
            state: AgentState::Succeeded,
            summary: summary.into(),
            steps,
            duration_ms: 0,
            error: None,
            observation: None,
        }
    }

    pub fn failed(
        agent_id: impl Into<String>,
        summary: impl Into<String>,
        steps: Vec<AgentStep>,
        error: impl Into<String>,
    ) -> Self {
        Self {
            agent_id: agent_id.into(),
            state: AgentState::Failed,
            summary: summary.into(),
            steps,
            duration_ms: 0,
            error: Some(error.into()),
            observation: None,
        }
    }

    /// Build a result for an aborted worker (cancelled / timed out / budget
    /// exhausted). Used by the runtime when it has to override the driver.
    pub fn aborted(
        agent_id: impl Into<String>,
        reason: AgentAbort,
        summary: impl Into<String>,
        steps: Vec<AgentStep>,
        duration_ms: u64,
    ) -> Self {
        let state = match reason {
            AgentAbort::Cancelled => AgentState::Cancelled,
            AgentAbort::TimedOut => AgentState::TimedOut,
            AgentAbort::BudgetExhausted => AgentState::BudgetExhausted,
        };
        Self {
            agent_id: agent_id.into(),
            state,
            summary: summary.into(),
            steps,
            duration_ms,
            error: Some(reason.as_str().to_string()),
            observation: None,
        }
    }

    /// Attach the worker's structured observation payload. The runtime never
    /// interprets it — a parent director folds it into the world model.
    pub fn with_observation(mut self, observation: serde_json::Value) -> Self {
        self.observation = Some(observation);
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn agent_result_carries_an_optional_structured_observation() {
        let plain = AgentResult::succeeded("a.1", "did the work", vec![]);
        assert!(plain.observation.is_none(), "no observation by default");

        let with_obs = plain.clone().with_observation(serde_json::json!({ "stdout": "ok" }));
        assert_eq!(with_obs.observation.as_ref().unwrap()["stdout"], "ok");

        // Round-trip preserves the payload.
        let json = serde_json::to_string(&with_obs).unwrap();
        let restored: AgentResult = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.observation, with_obs.observation);

        // A result without a payload serializes without the key and legacy
        // records deserialize to `None`.
        let plain_json = serde_json::to_string(&plain).unwrap();
        assert!(!plain_json.contains("observation"), "{plain_json}");
        let legacy: AgentResult = serde_json::from_str(&plain_json).unwrap();
        assert!(legacy.observation.is_none());
    }
}
