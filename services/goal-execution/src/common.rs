use crate::confidence::Confidence;
use crate::goal::Goal;
use friday_reflection::Outcome;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// ── Action ──────────────────────────────────────────────────────────

/// An action — an executable operation produced by Planning.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Action {
    pub id: String,
    pub instruction: String,
    pub capability: Capability,
    pub parameters: serde_json::Value,
    pub goal_id: String,
    pub step: u32,
    pub total_steps: u32,
    /// Confidence that this action will succeed.
    pub confidence: Confidence,
}

impl Action {
    pub fn single(
        instruction: impl Into<String>,
        capability: Capability,
        goal_id: impl Into<String>,
    ) -> Self {
        Self {
            id: format!("action_{}", uuid::Uuid::new_v4()),
            instruction: instruction.into(),
            capability,
            parameters: serde_json::json!({}),
            goal_id: goal_id.into(),
            step: 0,
            total_steps: 1,
            confidence: Confidence::medium(),
        }
    }

    pub fn step(
        instruction: impl Into<String>,
        capability: Capability,
        goal_id: impl Into<String>,
        step: u32,
        total_steps: u32,
    ) -> Self {
        Self {
            id: format!("action_{}", uuid::Uuid::new_v4()),
            instruction: instruction.into(),
            capability,
            parameters: serde_json::json!({}),
            goal_id: goal_id.into(),
            step,
            total_steps,
            confidence: Confidence::medium(),
        }
    }

    pub fn with_parameters(mut self, params: serde_json::Value) -> Self {
        self.parameters = params;
        self
    }

    pub fn is_first(&self) -> bool {
        self.step == 0
    }

    pub fn is_last(&self) -> bool {
        self.step + 1 >= self.total_steps
    }

    pub fn is_multi_step(&self) -> bool {
        self.total_steps > 1
    }
}

// ── Capability ──────────────────────────────────────────────────────

/// A capability — what a tool can do.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Capability {
    ExecuteCommand,
    ReadFile,
    WriteFile,
    DeleteFile,
    ListDirectory,
    OpenBrowser,
    SearchWeb,
    SearchMemory,
    Custom(String),
}

impl std::fmt::Display for Capability {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ExecuteCommand => write!(f, "execute_command"),
            Self::ReadFile => write!(f, "read_file"),
            Self::WriteFile => write!(f, "write_file"),
            Self::DeleteFile => write!(f, "delete_file"),
            Self::ListDirectory => write!(f, "list_directory"),
            Self::OpenBrowser => write!(f, "open_browser"),
            Self::SearchWeb => write!(f, "search_web"),
            Self::SearchMemory => write!(f, "search_memory"),
            Self::Custom(s) => write!(f, "{}", s),
        }
    }
}

// ── Expectation ─────────────────────────────────────────────────────

/// An expectation — what we believe will happen.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Expectation {
    pub id: String,
    pub description: String,
    pub expect_success: bool,
    pub expected_duration_ms: u64,
    pub expected_properties: HashMap<String, serde_json::Value>,
}

impl Expectation {
    pub fn success(description: impl Into<String>) -> Self {
        Self {
            id: format!("exp_{}", uuid::Uuid::new_v4()),
            description: description.into(),
            expect_success: true,
            expected_duration_ms: 0,
            expected_properties: HashMap::new(),
        }
    }

    pub fn failure(description: impl Into<String>) -> Self {
        Self {
            id: format!("exp_{}", uuid::Uuid::new_v4()),
            description: description.into(),
            expect_success: false,
            expected_duration_ms: 0,
            expected_properties: HashMap::new(),
        }
    }

    pub fn with_duration(mut self, ms: u64) -> Self {
        self.expected_duration_ms = ms;
        self
    }
}

// ── Risk Model ─────────────────────────────────────────────────────

/// Risk level for an action.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Risk {
    Low,
    Medium,
    High,
    Critical,
}

impl Risk {
    pub fn requires_approval(&self) -> bool {
        *self >= Risk::High
    }
}

impl std::fmt::Display for Risk {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Risk::Low => write!(f, "low"),
            Risk::Medium => write!(f, "medium"),
            Risk::High => write!(f, "high"),
            Risk::Critical => write!(f, "critical"),
        }
    }
}

/// Risk assessment for a single action.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionRisk {
    pub action_id: String,
    pub risk: Risk,
    pub reason: String,
}

impl ActionRisk {
    pub fn new(action_id: impl Into<String>, risk: Risk, reason: impl Into<String>) -> Self {
        Self {
            action_id: action_id.into(),
            risk,
            reason: reason.into(),
        }
    }
}

/// Assess risk for a capability.
pub fn assess_risk(capability: &Capability) -> ActionRisk {
    let (risk, reason) = match capability {
        Capability::ReadFile | Capability::ListDirectory | Capability::SearchMemory => {
            (Risk::Low, "read-only operation".into())
        }
        Capability::SearchWeb | Capability::OpenBrowser => {
            (Risk::Low, "external query, no state change".into())
        }
        Capability::ExecuteCommand => (
            Risk::Medium,
            "command execution may have side effects".into(),
        ),
        Capability::WriteFile => (Risk::Medium, "modifies filesystem".into()),
        Capability::DeleteFile => (Risk::High, "destructive, irreversible".into()),
        Capability::Custom(name) => (Risk::Medium, format!("custom capability: {}", name)),
    };
    ActionRisk::new("unknown", risk, reason)
}

// ── Mission ─────────────────────────────────────────────────────────

/// A mission — coordinates multiple goals toward a higher-level outcome.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Mission {
    pub id: String,
    pub description: String,
    pub goal_descriptions: Vec<String>,
    pub completed: HashMap<usize, Outcome>,
    pub created_at_ms: u64,
    pub status: MissionStatus,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum MissionStatus {
    Created,
    Running,
    Complete,
    Aborted,
}

impl Mission {
    pub fn new(description: impl Into<String>, goal_descriptions: Vec<String>) -> Self {
        let complete = goal_descriptions.is_empty();
        Self {
            id: format!("mission_{}", uuid::Uuid::new_v4()),
            description: description.into(),
            goal_descriptions,
            completed: HashMap::new(),
            created_at_ms: chrono::Utc::now().timestamp_millis() as u64,
            status: if complete {
                MissionStatus::Complete
            } else {
                MissionStatus::Created
            },
        }
    }

    pub fn start(&mut self) {
        self.status = MissionStatus::Running;
    }

    pub fn complete_goal(&mut self, goal_index: usize, outcome: Outcome) {
        self.completed.insert(goal_index, outcome);
        if self.completed.len() >= self.goal_descriptions.len() {
            self.status = MissionStatus::Complete;
        }
    }

    pub fn abort(&mut self) {
        self.status = MissionStatus::Aborted;
    }

    pub fn next_goal(&self) -> Option<usize> {
        (0..self.goal_descriptions.len()).find(|&i| !self.completed.contains_key(&i))
    }

    pub fn goal_at(&self, index: usize) -> Option<Goal> {
        self.goal_descriptions.get(index).map(|desc| {
            Goal::new(desc.as_str()).with_context(format!(
                "Mission: {} (step {} of {})",
                self.description,
                index + 1,
                self.goal_descriptions.len()
            ))
        })
    }

    pub fn total_goals(&self) -> usize {
        self.goal_descriptions.len()
    }

    pub fn completed_goals(&self) -> usize {
        self.completed.len()
    }

    pub fn is_complete(&self) -> bool {
        self.status == MissionStatus::Complete
    }
}

// ── Execution Context ───────────────────────────────────────────────

/// Execution context — state that flows with every Tool invocation.
///
/// ```text
/// ExecutionContext {
///     mission_id,
///     goal_id,
///     environment,
///     variables,
///     timeout,
///     permissions,
/// }
/// ```
///
/// Every Tool receives this. It replaces passing many separate parameters.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ExecutionContext {
    pub mission_id: Option<String>,
    pub goal_id: String,
    pub working_directory: Option<String>,
    pub environment: HashMap<String, String>,
    pub variables: HashMap<String, serde_json::Value>,
    pub timeout_ms: Option<u64>,
    pub permissions: Vec<String>,
}

impl ExecutionContext {
    pub fn for_goal(goal_id: impl Into<String>) -> Self {
        Self {
            goal_id: goal_id.into(),
            ..Default::default()
        }
    }

    pub fn with_mission_id(mut self, mission_id: impl Into<String>) -> Self {
        self.mission_id = Some(mission_id.into());
        self
    }

    pub fn with_working_directory(mut self, dir: impl Into<String>) -> Self {
        self.working_directory = Some(dir.into());
        self
    }

    pub fn with_variable(mut self, key: impl Into<String>, value: serde_json::Value) -> Self {
        self.variables.insert(key.into(), value);
        self
    }

    pub fn with_timeout_ms(mut self, ms: u64) -> Self {
        self.timeout_ms = Some(ms);
        self
    }

    pub fn with_permissions(mut self, perms: Vec<String>) -> Self {
        self.permissions = perms;
        self
    }

    pub fn with_environment(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.environment.insert(key.into(), value.into());
        self
    }
}

// ── Trace ───────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct Trace {
    pub started_at_ms: u64,
    pub finished_at_ms: u64,
}

impl Trace {
    pub fn duration_ms(&self) -> u64 {
        self.finished_at_ms.saturating_sub(self.started_at_ms)
    }
}
