//! # FRIDAY Goal Execution
//!
//! The execution pipeline: **Goal → Planning → Action → Tool → Verification → Outcome → Reflection**.
//!
//! # Architecture
//!
//! ```text
//! Mission
//!   ├── Goal A ──→ Think ──→ Evaluate ──→ Choose ──→ Execute ──→ Verify ──→ Reflect
//!   ├── Goal B ──→ Think ──→ Evaluate ──→ Choose ──→ Execute ──→ Verify ──→ Reflect
//!   └── Goal C
//!
//! PlanningContext → Planning → Vec<PlanCandidate> → PlanEvaluator → RankedPlan
//!   (Goal, Knowledge, Policies, Constraints, WorldState)
//!
//! ExecutionContext → ToolResult
//!   (mission_id, goal_id, environment, variables, timeout, permissions)
//! ```
//!
//! # Responsibilities
//!
//! | Component        | Owns                                              |
//! | ---------------- | ------------------------------------------------- |
//! | Runtime          | *When* things happen (scheduling, phases)         |
//! | Mission          | *Coordination* (multi-goal sequencing)            |
//! | PlanningContext  | *Inputs* (Goal, Knowledge, Policies, WorldState)  |
//! | Planning         | *Hypotheses* (Context → candidate plans)          |
//! | PlanEvaluator    | *Judgment* (candidates → ranked plan)             |
//! | WorldState       | *Environment* (current state snapshot)            |
//! | WorldModel       | *Transitions* (state changes, preconditions)      |
//! | Simulator        | *Prediction* (simulate plans, no execution)       |
//! | RiskModel        | *Safety* (risk assessment per action)             |
//! | Confidence       | *Certainty* (validated belief strength)           |
//! | ConfidenceEngine | *Propagation* (reason under uncertainty)          |
//! | TraceRecorder    | *Audit assembly* (events → CognitiveTrace)        |
//! | CognitiveTrace   | *Audit* (full decision trail)                     |
//! | ExecutionContext | *State* (mission, environment, variables)        |
//! | Goal Engine      | *How* to execute (Action → Tool → Outcome)        |
//! | Tool Registry    | *Which* tool handles a capability                 |
//! | Tool             | *Implementation* (terminal, filesystem)           |
//! | Verification     | *Interpretation* (tool output → verified result)  |
//! | Reflection       | *Evaluation* (Expected vs Observed)               |
//! | Learning         | *Adaptation* (Insights → Policies)                |
//!
//! # Invariants
//!
//! - Every Goal execution produces exactly **one Outcome**.
//! - Tool output ≠ success. Verification interprets execution results.
//! - Planning produces candidate plans, never executes.
//! - Learning produces Policies. Planning consumes Policies.
//! - Policies are immutable evidence-backed records.
//! - Mission coordinates Goals. Planning operates per Goal.
//! - Every Tool receives an ExecutionContext.
//! - PlanEvaluator is deterministic: same candidates → same ranking.
//! - Every action produces a StateTransition (immutable audit trail).
//! - Preconditions are checked before execution; postconditions applied after.
//! - CognitiveTrace records every decision node: input, output, world state, timing.
//! - Simulation never calls tools. Pure prediction.
//! - TraceRecorder decouples tracing from execution logic.
//! - Every action has a Risk level. Critical risk requires user approval.
//! - Confidence is a first-class value: Goal, Action, WorldState, Plan, Verification all carry it.
//! - ConfidenceEngine propagates confidence through rules (multiply, average, discount, cap, min).

use async_trait::async_trait;
use friday_knowledge::{KnowledgeEngine, KnowledgeRecord};
use friday_memory::Episode;
use friday_reflection::{Insight, InsightKind, Outcome, OutcomeKind, ReflectionEngine};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt;
use std::ops;

// ── Confidence ─────────────────────────────────────────────────────

/// Confidence — a validated [0.0, 1.0] value that represents belief strength.
///
/// Unlike raw `f32`, Confidence:
/// - Is always clamped to [0.0, 1.0]
/// - Has semantic constructors (none, low, medium, high, certain)
/// - Supports arithmetic (multiply, combine)
/// - Is a first-class reasoning value throughout the runtime
///
/// ```text
/// Confidence::none()   = 0.0   (no belief)
/// Confidence::low()    = 0.25  (weak belief)
/// Confidence::medium() = 0.5   (moderate belief)
/// Confidence::high()   = 0.75  (strong belief)
/// Confidence::certain() = 1.0  (absolute certainty)
/// ```
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Serialize, Deserialize)]
pub struct Confidence(f32);

impl Confidence {
    /// No confidence at all.
    pub fn none() -> Self {
        Self(0.0)
    }

    /// Low confidence.
    pub fn low() -> Self {
        Self(0.25)
    }

    /// Medium confidence.
    pub fn medium() -> Self {
        Self(0.5)
    }

    /// High confidence.
    pub fn high() -> Self {
        Self(0.75)
    }

    /// Absolute certainty.
    pub fn certain() -> Self {
        Self(1.0)
    }

    /// Create from a raw f32, clamped to [0.0, 1.0].
    pub fn from_raw(value: f32) -> Self {
        Self(value.clamp(0.0, 1.0))
    }

    /// Create from a ratio (numerator / denominator).
    pub fn from_ratio(numerator: u32, denominator: u32) -> Self {
        if denominator == 0 {
            Self(0.0)
        } else {
            Self((numerator as f32 / denominator as f32).clamp(0.0, 1.0))
        }
    }

    /// Get the raw value.
    pub fn value(&self) -> f32 {
        self.0
    }

    /// Combine two confidences using independent evidence multiplication:
    /// `P(A and B) = P(A) * P(B)`. Useful when two beliefs are independent.
    pub fn and(self, other: Confidence) -> Confidence {
        Confidence::from_raw(self.0 * other.0)
    }

    /// Combine two confidences using consensus averaging:
    /// `(P(A) + P(B)) / 2`. Useful when two sources agree.
    pub fn or(self, other: Confidence) -> Confidence {
        Confidence::from_raw((self.0 + other.0) / 2.0)
    }

    /// Apply a discount factor (multiply by a value in [0.0, 1.0]).
    pub fn discount(self, factor: f32) -> Confidence {
        Confidence::from_raw(self.0 * factor.clamp(0.0, 1.0))
    }

    /// Classify into a semantic level.
    pub fn level(&self) -> ConfidenceLevel {
        if self.0 < 0.1 {
            ConfidenceLevel::None
        } else if self.0 < 0.35 {
            ConfidenceLevel::Low
        } else if self.0 < 0.65 {
            ConfidenceLevel::Medium
        } else if self.0 < 0.9 {
            ConfidenceLevel::High
        } else {
            ConfidenceLevel::Certain
        }
    }

    /// Whether this confidence meets a minimum threshold.
    pub fn meets(&self, threshold: Confidence) -> bool {
        self.0 >= threshold.0
    }
}

impl Default for Confidence {
    fn default() -> Self {
        Self::none()
    }
}

impl From<f32> for Confidence {
    fn from(v: f32) -> Self {
        Self::from_raw(v)
    }
}

impl From<Confidence> for f32 {
    fn from(c: Confidence) -> Self {
        c.0
    }
}

impl fmt::Display for Confidence {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:.0}%", self.0 * 100.0)
    }
}

impl ops::Mul for Confidence {
    type Output = Confidence;
    fn mul(self, rhs: Confidence) -> Confidence {
        self.and(rhs)
    }
}

/// Semantic classification of confidence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum ConfidenceLevel {
    None,
    Low,
    Medium,
    High,
    Certain,
}

// ── Goal ────────────────────────────────────────────────────────────

/// A goal — what the user wants to achieve.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Goal {
    pub id: String,
    pub description: String,
    pub context: Option<String>,
    pub created_at_ms: u64,
    /// Confidence that this goal is achievable.
    pub confidence: Confidence,
}

impl Goal {
    pub fn new(description: impl Into<String>) -> Self {
        Self {
            id: format!("goal_{}", uuid::Uuid::new_v4()),
            description: description.into(),
            context: None,
            created_at_ms: chrono::Utc::now().timestamp_millis() as u64,
            confidence: Confidence::medium(),
        }
    }

    pub fn with_context(mut self, context: impl Into<String>) -> Self {
        self.context = Some(context.into());
        self
    }

    pub fn with_confidence(mut self, confidence: Confidence) -> Self {
        self.confidence = confidence;
        self
    }
}

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

// ── World State ─────────────────────────────────────────────────────

/// World state — a snapshot of the current environment.
///
/// ```text
/// WorldState {
///     working_directory,
///     files,
///     processes,
///     environment,
///     variables,
///     available_tools,
///     mission_progress,
///     network_available,
///     os,
/// }
/// ```
///
/// Planning reasons against WorldState instead of raw knowledge.
/// This lets the planner ask: "Given what exists right now,
/// what's the best plan?"
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct WorldState {
    /// Current working directory.
    pub working_directory: Option<String>,
    /// Known files (path → metadata).
    pub files: HashMap<String, FileMeta>,
    /// Running processes.
    pub processes: Vec<ProcessInfo>,
    /// Environment variables.
    pub environment: HashMap<String, String>,
    /// Shared variables from previous executions.
    pub variables: HashMap<String, serde_json::Value>,
    /// Available tool capabilities.
    pub available_tools: Vec<Capability>,
    /// Mission progress (if in a mission).
    pub mission_progress: Option<MissionProgress>,
    /// Whether network is available.
    pub network_available: bool,
    /// Operating system.
    pub os: String,
    /// Confidence that this snapshot is accurate.
    pub confidence: Confidence,
}

/// Metadata about a file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileMeta {
    pub path: String,
    pub exists: bool,
    pub size_bytes: Option<u64>,
    pub is_directory: bool,
    pub modified_at_ms: Option<u64>,
}

/// Information about a running process.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcessInfo {
    pub pid: u32,
    pub name: String,
    pub command: String,
}

/// Progress within a mission.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MissionProgress {
    pub mission_id: String,
    pub total_goals: usize,
    pub completed_goals: usize,
    pub current_goal_index: usize,
}

impl WorldState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_working_directory(mut self, dir: impl Into<String>) -> Self {
        self.working_directory = Some(dir.into());
        self
    }

    pub fn with_file(mut self, path: impl Into<String>, meta: FileMeta) -> Self {
        self.files.insert(path.into(), meta);
        self
    }

    pub fn with_tool(mut self, cap: Capability) -> Self {
        self.available_tools.push(cap);
        self
    }

    pub fn with_variable(mut self, key: impl Into<String>, value: serde_json::Value) -> Self {
        self.variables.insert(key.into(), value);
        self
    }

    pub fn with_environment(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.environment.insert(key.into(), value.into());
        self
    }

    pub fn with_network(mut self, available: bool) -> Self {
        self.network_available = available;
        self
    }

    pub fn with_confidence(mut self, confidence: Confidence) -> Self {
        self.confidence = confidence;
        self
    }

    pub fn with_mission_progress(mut self, progress: MissionProgress) -> Self {
        self.mission_progress = Some(progress);
        self
    }

    /// Check if a file exists in the world state.
    pub fn file_exists(&self, path: &str) -> bool {
        self.files
            .get(path)
            .map(|m| m.exists)
            .unwrap_or(false)
    }

    /// Check if a tool capability is available.
    pub fn has_tool(&self, cap: &Capability) -> bool {
        self.available_tools.contains(cap)
    }

    /// Compute the difference between two world states.
    pub fn diff(&self, other: &WorldState) -> WorldStateDiff {
        let mut added_files = Vec::new();
        let mut removed_files = Vec::new();
        let mut modified_files = Vec::new();

        for (path, meta) in &other.files {
            match self.files.get(path) {
                None => added_files.push(path.clone()),
                Some(old) => {
                    if old.size_bytes != meta.size_bytes
                        || old.modified_at_ms != meta.modified_at_ms
                        || old.exists != meta.exists
                    {
                        modified_files.push(path.clone());
                    }
                }
            }
        }

        for path in self.files.keys() {
            if !other.files.contains_key(path) {
                removed_files.push(path.clone());
            }
        }

        let mut added_vars = Vec::new();
        let mut removed_vars = Vec::new();
        let mut changed_vars = Vec::new();

        for (key, val) in &other.variables {
            match self.variables.get(key) {
                None => added_vars.push(key.clone()),
                Some(old) => {
                    if old != val {
                        changed_vars.push(key.clone());
                    }
                }
            }
        }

        for key in self.variables.keys() {
            if !other.variables.contains_key(key) {
                removed_vars.push(key.clone());
            }
        }

        WorldStateDiff {
            added_files,
            removed_files,
            modified_files,
            added_vars,
            removed_vars,
            changed_vars,
            working_directory_changed: self.working_directory != other.working_directory,
            network_changed: self.network_available != other.network_available,
        }
    }
}

/// A diff between two world states — what changed.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct WorldStateDiff {
    pub added_files: Vec<String>,
    pub removed_files: Vec<String>,
    pub modified_files: Vec<String>,
    pub added_vars: Vec<String>,
    pub removed_vars: Vec<String>,
    pub changed_vars: Vec<String>,
    pub working_directory_changed: bool,
    pub network_changed: bool,
}

impl WorldStateDiff {
    pub fn is_empty(&self) -> bool {
        self.added_files.is_empty()
            && self.removed_files.is_empty()
            && self.modified_files.is_empty()
            && self.added_vars.is_empty()
            && self.removed_vars.is_empty()
            && self.changed_vars.is_empty()
            && !self.working_directory_changed
            && !self.network_changed
    }
}

// ── Preconditions & Postconditions ──────────────────────────────────

/// A precondition — what an action requires from the world state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Precondition {
    FileExists { path: String },
    FileNotExists { path: String },
    ToolAvailable { capability: Capability },
    VariableEquals { key: String, value: serde_json::Value },
    NetworkAvailable,
    Custom(String),
}

impl Precondition {
    pub fn check(&self, state: &WorldState) -> bool {
        match self {
            Precondition::FileExists { path } => state.file_exists(path),
            Precondition::FileNotExists { path } => !state.file_exists(path),
            Precondition::ToolAvailable { capability } => state.has_tool(capability),
            Precondition::VariableEquals { key, value } => {
                state.variables.get(key.as_str()) == Some(value)
            }
            Precondition::NetworkAvailable => state.network_available,
            Precondition::Custom(_) => true,
        }
    }

    pub fn describe(&self) -> String {
        match self {
            Precondition::FileExists { path } => format!("file exists: {}", path),
            Precondition::FileNotExists { path } => format!("file not exists: {}", path),
            Precondition::ToolAvailable { capability } => format!("tool available: {}", capability),
            Precondition::VariableEquals { key, .. } => format!("variable {} equals expected", key),
            Precondition::NetworkAvailable => "network available".into(),
            Precondition::Custom(desc) => desc.clone(),
        }
    }
}

/// A postcondition — what an action promises to change.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Postcondition {
    FileCreated { path: String },
    FileDeleted { path: String },
    FileModified { path: String },
    VariableSet { key: String },
    NetworkStateChanged,
    Custom(String),
}

impl Postcondition {
    pub fn describe(&self) -> String {
        match self {
            Postcondition::FileCreated { path } => format!("file created: {}", path),
            Postcondition::FileDeleted { path } => format!("file deleted: {}", path),
            Postcondition::FileModified { path } => format!("file modified: {}", path),
            Postcondition::VariableSet { key } => format!("variable set: {}", key),
            Postcondition::NetworkStateChanged => "network state changed".into(),
            Postcondition::Custom(desc) => desc.clone(),
        }
    }
}

// ── State Transition ────────────────────────────────────────────────

/// An immutable record of a world state change — the audit trail.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StateTransition {
    pub id: String,
    pub action_id: String,
    pub goal_id: String,
    pub before: WorldState,
    pub after: WorldState,
    pub diff: WorldStateDiff,
    pub preconditions_met: bool,
    pub preconditions_failed: Vec<String>,
    pub occurred_at_ms: u64,
}

impl StateTransition {
    pub fn new(
        action_id: impl Into<String>,
        goal_id: impl Into<String>,
        before: WorldState,
        after: WorldState,
        preconditions: &[Precondition],
    ) -> Self {
        let diff = before.diff(&after);
        let mut failed = Vec::new();
        for pre in preconditions {
            if !pre.check(&before) {
                failed.push(pre.describe());
            }
        }
        let met = failed.is_empty();

        Self {
            id: format!("trans_{}", uuid::Uuid::new_v4()),
            action_id: action_id.into(),
            goal_id: goal_id.into(),
            before,
            after,
            diff,
            preconditions_met: met,
            preconditions_failed: failed,
            occurred_at_ms: chrono::Utc::now().timestamp_millis() as u64,
        }
    }

    pub fn is_clean(&self) -> bool {
        self.preconditions_met && self.diff.is_empty()
    }
}

// ── World Model ─────────────────────────────────────────────────────

/// The World Model — owns the current world state, applies transitions,
/// and maintains a full history of state changes.
///
/// ```text
/// WorldModel
///   state:    WorldState        (current snapshot)
///   history:  Vec<StateTransition>  (immutable audit trail)
/// ```
///
/// The World Model is the single source of truth for environment state.
/// Actions declare preconditions; the model checks them before execution.
/// After execution, postconditions are applied and a transition is recorded.
pub struct WorldModel {
    state: WorldState,
    history: Vec<StateTransition>,
}

impl WorldModel {
    pub fn new(initial: WorldState) -> Self {
        Self {
            state: initial,
            history: Vec::new(),
        }
    }

    /// Current world state snapshot.
    pub fn state(&self) -> &WorldState {
        &self.state
    }

    /// Full transition history (immutable audit trail).
    pub fn history(&self) -> &[StateTransition] {
        &self.history
    }

    /// Check if all preconditions are met for the current state.
    pub fn check_preconditions(&self, preconditions: &[Precondition]) -> Vec<String> {
        preconditions
            .iter()
            .filter(|p| !p.check(&self.state))
            .map(|p| p.describe())
            .collect()
    }

    /// Record a transition (after execution). Returns the transition.
    pub fn record_transition(
        &mut self,
        action_id: impl Into<String>,
        goal_id: impl Into<String>,
        new_state: WorldState,
        preconditions: &[Precondition],
    ) -> StateTransition {
        let before = self.state.clone();
        let transition =
            StateTransition::new(action_id, goal_id, before, new_state.clone(), preconditions);
        self.state = new_state;
        self.history.push(transition.clone());
        transition
    }

    /// Apply postconditions by mutating the current state.
    pub fn apply_postconditions(&mut self, postconditions: &[Postcondition]) {
        for post in postconditions {
            match post {
                Postcondition::FileCreated { path } => {
                    self.state.files.insert(
                        path.clone(),
                        FileMeta {
                            path: path.clone(),
                            exists: true,
                            size_bytes: None,
                            is_directory: false,
                            modified_at_ms: Some(chrono::Utc::now().timestamp_millis() as u64),
                        },
                    );
                }
                Postcondition::FileDeleted { path } => {
                    if let Some(meta) = self.state.files.get_mut(path) {
                        meta.exists = false;
                    }
                }
                Postcondition::FileModified { path } => {
                    if let Some(meta) = self.state.files.get_mut(path) {
                        meta.modified_at_ms =
                            Some(chrono::Utc::now().timestamp_millis() as u64);
                    }
                }
                Postcondition::VariableSet { key } => {
                    self.state
                        .variables
                        .entry(key.clone())
                        .or_insert_with(|| serde_json::json!(null));
                }
                Postcondition::NetworkStateChanged => {
                    self.state.network_available = !self.state.network_available;
                }
                Postcondition::Custom(_) => {}
            }
        }
    }

    /// Set the working directory.
    pub fn set_working_directory(&mut self, dir: impl Into<String>) {
        self.state.working_directory = Some(dir.into());
    }

    /// Set a variable.
    pub fn set_variable(&mut self, key: impl Into<String>, value: serde_json::Value) {
        self.state.variables.insert(key.into(), value);
    }
}

// ── Cognitive Trace ─────────────────────────────────────────────────

/// The kind of a trace node.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TraceNodeKind {
    Planning,
    Evaluation,
    Execution,
    Verification,
    Reflection,
    Learning,
    WorldStateChange,
}

/// A single node in a cognitive trace — an immutable record of one decision.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraceNode {
    pub id: String,
    pub kind: TraceNodeKind,
    pub parent_id: Option<String>,
    pub input: serde_json::Value,
    pub output: serde_json::Value,
    pub world_state_snapshot: Option<WorldState>,
    pub confidence: Option<f32>,
    pub policies_used: Vec<String>,
    pub started_at_ms: u64,
    pub finished_at_ms: u64,
}

impl TraceNode {
    pub fn new(
        kind: TraceNodeKind,
        input: serde_json::Value,
        output: serde_json::Value,
    ) -> Self {
        let now = chrono::Utc::now().timestamp_millis() as u64;
        Self {
            id: format!("node_{}", uuid::Uuid::new_v4()),
            kind,
            parent_id: None,
            input,
            output,
            world_state_snapshot: None,
            confidence: None,
            policies_used: Vec::new(),
            started_at_ms: now,
            finished_at_ms: now,
        }
    }

    pub fn with_parent(mut self, parent_id: impl Into<String>) -> Self {
        self.parent_id = Some(parent_id.into());
        self
    }

    pub fn with_world_state(mut self, state: WorldState) -> Self {
        self.world_state_snapshot = Some(state);
        self
    }

    pub fn with_confidence(mut self, confidence: f32) -> Self {
        self.confidence = Some(confidence.clamp(0.0, 1.0));
        self
    }

    pub fn with_policies(mut self, policies: Vec<String>) -> Self {
        self.policies_used = policies;
        self
    }

    pub fn with_timing(mut self, started_at_ms: u64, finished_at_ms: u64) -> Self {
        self.started_at_ms = started_at_ms;
        self.finished_at_ms = finished_at_ms;
        self
    }

    pub fn duration_ms(&self) -> u64 {
        self.finished_at_ms.saturating_sub(self.started_at_ms)
    }
}

/// A complete cognitive trace — the full audit trail for a goal execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CognitiveTrace {
    pub goal_id: String,
    pub nodes: Vec<TraceNode>,
}

impl CognitiveTrace {
    pub fn new(goal_id: impl Into<String>) -> Self {
        Self {
            goal_id: goal_id.into(),
            nodes: Vec::new(),
        }
    }

    pub fn push(&mut self, node: TraceNode) {
        self.nodes.push(node);
    }

    pub fn nodes_of_kind(&self, kind: &TraceNodeKind) -> Vec<&TraceNode> {
        self.nodes.iter().filter(|n| n.kind == *kind).collect()
    }

    pub fn total_duration_ms(&self) -> u64 {
        self.nodes.iter().map(|n| n.duration_ms()).sum()
    }

    /// Format the trace as a human-readable tree.
    pub fn display(&self) -> String {
        let mut lines = Vec::new();
        lines.push(format!("Goal: {}", self.goal_id));
        for (i, node) in self.nodes.iter().enumerate() {
            let prefix = if i == self.nodes.len() - 1 {
                "└── "
            } else {
                "├── "
            };
            let confidence_str = node
                .confidence
                .map(|c| format!(" [confidence: {:.2}]", c))
                .unwrap_or_default();
            let duration_str = format!(" ({}ms)", node.duration_ms());
            lines.push(format!(
                "{}{:?}{}{}",
                prefix, node.kind, confidence_str, duration_str
            ));
        }
        lines.join("\n")
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
        Capability::ExecuteCommand => {
            (Risk::Medium, "command execution may have side effects".into())
        }
        Capability::WriteFile => {
            (Risk::Medium, "modifies filesystem".into())
        }
        Capability::DeleteFile => {
            (Risk::High, "destructive, irreversible".into())
        }
        Capability::Custom(name) => {
            (Risk::Medium, format!("custom capability: {}", name))
        }
    };
    ActionRisk::new("unknown", risk, reason)
}

// ── Simulation ─────────────────────────────────────────────────────

/// The result of simulating a plan against a world state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimulationResult {
    /// Whether the simulation completed without contradictions.
    pub feasible: bool,
    /// Projected world state after all actions complete.
    pub projected_world: WorldState,
    /// Overall confidence in the simulation [0.0, 1.0].
    pub confidence: f32,
    /// Per-action risk assessments.
    pub risks: Vec<ActionRisk>,
    /// Aggregate risk level (highest individual risk).
    pub aggregate_risk: Risk,
    /// Estimated cost (normalized 0.0–1.0).
    pub expected_cost: f32,
    /// Estimated duration in milliseconds.
    pub expected_duration_ms: u64,
    /// Preconditions that failed during simulation.
    pub failed_preconditions: Vec<String>,
    /// Postconditions expected to be met.
    pub expected_postconditions: Vec<String>,
    /// Per-step simulation traces.
    pub step_results: Vec<StepResult>,
}

/// The result of simulating a single step.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StepResult {
    pub step_index: usize,
    pub action_id: String,
    pub preconditions_met: bool,
    pub failed_preconditions: Vec<String>,
    pub projected_effects: Vec<String>,
    pub risk: Risk,
}

/// A simulator — predicts plan outcomes without executing.
///
/// ```text
/// Simulator trait:
///   simulate(plan, world) -> SimulationResult
///
/// Never calls tools. Pure prediction.
/// ```
pub trait Simulator: Send + Sync {
    /// Simulate a plan against a world state.
    fn simulate(&self, plan: &Plan, world: &WorldState) -> SimulationResult;
}

/// Default simulator — uses preconditions/postconditions and capability risk.
pub struct DefaultSimulator;

impl DefaultSimulator {
    pub fn new() -> Self {
        Self
    }
}

impl Default for DefaultSimulator {
    fn default() -> Self {
        Self::new()
    }
}

impl Simulator for DefaultSimulator {
    fn simulate(&self, plan: &Plan, world: &WorldState) -> SimulationResult {
        let projected = world.clone();
        let mut all_risks = Vec::new();
        let mut failed_preconditions = Vec::new();
        let mut expected_postconditions = Vec::new();
        let mut step_results = Vec::new();
        let mut feasible = true;
        let mut total_cost: f32 = 0.0;

        for (i, action) in plan.actions.iter().enumerate() {
            let risk = assess_risk(&action.capability);
            all_risks.push(ActionRisk::new(&action.id, risk.risk, &risk.reason));
            total_cost += match risk.risk {
                Risk::Low => 0.1,
                Risk::Medium => 0.3,
                Risk::High => 0.7,
                Risk::Critical => 1.0,
            };

            // Check if capability is available
            if !projected.has_tool(&action.capability) {
                let msg = format!("tool not available: {}", action.capability);
                failed_preconditions.push(msg.clone());
                feasible = false;
                step_results.push(StepResult {
                    step_index: i,
                    action_id: action.id.clone(),
                    preconditions_met: false,
                    failed_preconditions: vec![msg],
                    projected_effects: vec![],
                    risk: risk.risk,
                });
                continue;
            }

            // Simulate the effect: infer what this action would produce
            let effects = infer_effects(&action.capability, &action.instruction);
            for effect in &effects {
                expected_postconditions.push(effect.clone());
            }

            step_results.push(StepResult {
                step_index: i,
                action_id: action.id.clone(),
                preconditions_met: true,
                failed_preconditions: vec![],
                projected_effects: effects,
                risk: risk.risk,
            });
        }

        let aggregate_risk = all_risks
            .iter()
            .map(|r| r.risk)
            .max()
            .unwrap_or(Risk::Low);

        let confidence = if failed_preconditions.is_empty() {
            0.9
        } else {
            0.3
        };

        let cost = if plan.actions.is_empty() {
            0.0
        } else {
            (total_cost / plan.actions.len() as f32).clamp(0.0, 1.0)
        };

        SimulationResult {
            feasible,
            projected_world: projected,
            confidence,
            risks: all_risks,
            aggregate_risk,
            expected_cost: cost,
            expected_duration_ms: plan.estimated_duration_ms,
            failed_preconditions,
            expected_postconditions,
            step_results,
        }
    }
}

/// Infer the effects of an action based on its capability.
fn infer_effects(capability: &Capability, instruction: &str) -> Vec<String> {
    match capability {
        Capability::ReadFile => vec![format!("read: {}", instruction)],
        Capability::WriteFile => vec![format!("written: {}", instruction)],
        Capability::DeleteFile => vec![format!("deleted: {}", instruction)],
        Capability::ListDirectory => vec![format!("listed: {}", instruction)],
        Capability::ExecuteCommand => vec![format!("executed: {}", instruction)],
        Capability::OpenBrowser => vec![format!("opened: {}", instruction)],
        Capability::SearchWeb => vec![format!("searched: {}", instruction)],
        Capability::SearchMemory => vec![format!("queried memory: {}", instruction)],
        Capability::Custom(name) => vec![format!("custom({}): {}", name, instruction)],
    }
}

// ── Trace Recorder ─────────────────────────────────────────────────

/// A trace event — emitted by engines, assembled by TraceRecorder.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TraceEvent {
    PlanningStarted { goal_id: String },
    PlanGenerated { goal_id: String, candidate_count: usize },
    SimulationStarted { goal_id: String, plan_id: String },
    SimulationFinished { goal_id: String, plan_id: String, feasible: bool, confidence: f32 },
    EvaluationStarted { goal_id: String },
    EvaluationFinished { goal_id: String, chosen_plan_id: String, score: f32 },
    ExecutionStarted { goal_id: String, action_id: String },
    ExecutionFinished { goal_id: String, action_id: String, success: bool },
    VerificationStarted { goal_id: String, action_id: String },
    VerificationFinished { goal_id: String, action_id: String, verified: bool },
    ReflectionStarted { goal_id: String },
    ReflectionFinished { goal_id: String, insight_count: usize },
    WorldStateChanged { goal_id: String, transition_id: String },
}

/// A trace recorder — assembles a CognitiveTrace from emitted events.
///
/// ```text
/// Engines emit TraceEvents
///         │
///         ▼
/// TraceRecorder
///         │
///         ▼
///   CognitiveTrace
/// ```
///
/// This decouples tracing from execution logic.
pub struct TraceRecorder {
    trace: CognitiveTrace,
    event_log: Vec<TraceEvent>,
}

impl TraceRecorder {
    pub fn new(goal_id: impl Into<String>) -> Self {
        Self {
            trace: CognitiveTrace::new(goal_id),
            event_log: Vec::new(),
        }
    }

    /// Record a trace event and update the cognitive trace.
    pub fn record(&mut self, event: TraceEvent) {
        self.event_log.push(event.clone());

        let node = match &event {
            TraceEvent::PlanningStarted { .. } => {
                TraceNode::new(TraceNodeKind::Planning, serde_json::json!({}), serde_json::json!({}))
            }
            TraceEvent::PlanGenerated { candidate_count, .. } => {
                TraceNode::new(
                    TraceNodeKind::Planning,
                    serde_json::json!({}),
                    serde_json::json!({ "candidate_count": candidate_count }),
                )
            }
            TraceEvent::SimulationStarted { plan_id, .. } => {
                TraceNode::new(
                    TraceNodeKind::Execution,
                    serde_json::json!({ "plan_id": plan_id }),
                    serde_json::json!({}),
                )
            }
            TraceEvent::SimulationFinished { feasible, confidence, .. } => {
                TraceNode::new(
                    TraceNodeKind::Execution,
                    serde_json::json!({}),
                    serde_json::json!({ "feasible": feasible, "confidence": confidence }),
                ).with_confidence(*confidence)
            }
            TraceEvent::EvaluationFinished { score, .. } => {
                TraceNode::new(
                    TraceNodeKind::Evaluation,
                    serde_json::json!({}),
                    serde_json::json!({ "score": score }),
                ).with_confidence(*score)
            }
            TraceEvent::ExecutionStarted { action_id, .. } => {
                TraceNode::new(
                    TraceNodeKind::Execution,
                    serde_json::json!({ "action_id": action_id }),
                    serde_json::json!({}),
                )
            }
            TraceEvent::ExecutionFinished { success, .. } => {
                TraceNode::new(
                    TraceNodeKind::Execution,
                    serde_json::json!({}),
                    serde_json::json!({ "success": success }),
                )
            }
            TraceEvent::VerificationFinished { verified, .. } => {
                TraceNode::new(
                    TraceNodeKind::Verification,
                    serde_json::json!({}),
                    serde_json::json!({ "verified": verified }),
                )
            }
            TraceEvent::ReflectionFinished { insight_count, .. } => {
                TraceNode::new(
                    TraceNodeKind::Reflection,
                    serde_json::json!({}),
                    serde_json::json!({ "insight_count": insight_count }),
                )
            }
            _ => {
                TraceNode::new(TraceNodeKind::Planning, serde_json::json!({}), serde_json::json!({}))
            }
        };

        self.trace.push(node);
    }

    /// Get the assembled cognitive trace.
    pub fn trace(&self) -> &CognitiveTrace {
        &self.trace
    }

    /// Consume the recorder and return the trace.
    pub fn into_trace(self) -> CognitiveTrace {
        self.trace
    }

    /// Get the raw event log.
    pub fn events(&self) -> &[TraceEvent] {
        &self.event_log
    }
}

// ── Confidence Engine ──────────────────────────────────────────────

/// A confidence propagation rule.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ConfidenceRule {
    /// Multiply two sources (independent evidence).
    Multiply { sources: Vec<String> },
    /// Average multiple sources (consensus).
    Average { sources: Vec<String> },
    /// Apply a fixed discount to a source.
    Discount { source: String, factor: f32 },
    /// Cap at a maximum value.
    Cap { source: String, max: Confidence },
    /// Minimum of multiple sources.
    Min { sources: Vec<String> },
}

/// A confidence engine — propagates confidence across the reasoning pipeline.
///
/// ```text
/// Inputs:  named confidence values
/// Rules:   propagation rules
/// Outputs: computed confidence values
/// ```
///
/// The ConfidenceEngine doesn't store beliefs. It computes how confidence
/// propagates through the system given named inputs and rules.
pub struct ConfidenceEngine {
    rules: Vec<(String, ConfidenceRule)>,
}

impl ConfidenceEngine {
    pub fn new() -> Self {
        Self { rules: Vec::new() }
    }

    pub fn add_rule(mut self, name: impl Into<String>, rule: ConfidenceRule) -> Self {
        self.rules.push((name.into(), rule));
        self
    }

    /// Evaluate all rules against the given named confidence values.
    pub fn evaluate(&self, inputs: &HashMap<String, Confidence>) -> HashMap<String, Confidence> {
        let default_conf = Confidence::none();
        let mut outputs = HashMap::new();

        for (name, rule) in &self.rules {
            let result = match rule {
                ConfidenceRule::Multiply { sources } => {
                    let mut acc = Confidence::certain();
                    for src in sources {
                        let val = inputs.get(src.as_str()).unwrap_or(&default_conf);
                        acc = acc.and(*val);
                    }
                    acc
                }
                ConfidenceRule::Average { sources } => {
                    if sources.is_empty() {
                        Confidence::none()
                    } else {
                        let sum: f32 = sources
                            .iter()
                            .map(|s| inputs.get(s.as_str()).unwrap_or(&default_conf).value())
                            .sum();
                        Confidence::from_raw(sum / sources.len() as f32)
                    }
                }
                ConfidenceRule::Discount { source, factor } => {
                    let base = inputs.get(source.as_str()).unwrap_or(&default_conf);
                    base.discount(*factor)
                }
                ConfidenceRule::Cap { source, max } => {
                    let base = inputs.get(source.as_str()).unwrap_or(&default_conf);
                    Confidence::from_raw(base.value().min(max.value()))
                }
                ConfidenceRule::Min { sources } => {
                    let mut best = Confidence::certain();
                    for s in sources {
                        let val = inputs.get(s.as_str()).unwrap_or(&default_conf);
                        if val.value() < best.value() {
                            best = *val;
                        }
                    }
                    if sources.is_empty() { Confidence::none() } else { best }
                }
            };
            outputs.insert(name.clone(), result);
        }

        outputs
    }
}

impl Default for ConfidenceEngine {
    fn default() -> Self {
        Self::new()
    }
}

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

// ── Tool ────────────────────────────────────────────────────────────

/// A tool — a capability implementation.
#[async_trait]
pub trait Tool: Send + Sync {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    fn capabilities(&self) -> Vec<Capability>;
    async fn execute(&self, action: &Action, context: &ExecutionContext) -> ToolResult;
}

/// Result of a tool execution (raw, unverified).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResult {
    pub success: bool,
    pub output: String,
    pub error: Option<String>,
    pub duration_ms: u64,
    pub metadata: serde_json::Value,
}

impl ToolResult {
    pub fn success(output: impl Into<String>) -> Self {
        Self {
            success: true,
            output: output.into(),
            error: None,
            duration_ms: 0,
            metadata: serde_json::json!({}),
        }
    }

    pub fn error(message: impl Into<String>) -> Self {
        Self {
            success: false,
            output: String::new(),
            error: Some(message.into()),
            duration_ms: 0,
            metadata: serde_json::json!({}),
        }
    }

    pub fn with_duration_ms(mut self, ms: u64) -> Self {
        self.duration_ms = ms;
        self
    }
}

// ── Tool Registry ───────────────────────────────────────────────────

/// A registry of swappable tools. Resolves Capability → Best Tool.
pub struct ToolRegistry {
    tools: Vec<Box<dyn Tool>>,
}

impl ToolRegistry {
    pub fn new() -> Self {
        Self { tools: Vec::new() }
    }

    pub fn register(&mut self, tool: Box<dyn Tool>) {
        self.tools.push(tool);
    }

    pub fn resolve(&self, capability: &Capability) -> Option<&dyn Tool> {
        self.tools
            .iter()
            .find(|t| t.capabilities().contains(capability))
            .map(|t| t.as_ref())
    }

    pub fn get(&self, name: &str) -> Option<&dyn Tool> {
        self.tools.iter().find(|t| t.name() == name).map(|t| t.as_ref())
    }

    pub fn list(&self) -> Vec<&str> {
        self.tools.iter().map(|t| t.name()).collect()
    }

    pub fn capabilities(&self) -> Vec<Capability> {
        let mut caps: std::collections::HashSet<Capability> = std::collections::HashSet::new();
        for tool in &self.tools {
            for cap in tool.capabilities() {
                caps.insert(cap);
            }
        }
        caps.into_iter().collect()
    }

    pub fn is_empty(&self) -> bool {
        self.tools.is_empty()
    }

    pub fn len(&self) -> usize {
        self.tools.len()
    }
}

impl Default for ToolRegistry {
    fn default() -> Self {
        Self::new()
    }
}

// ── Verification ────────────────────────────────────────────────────

/// Verification — interprets tool output into a verified result.
pub struct Verification {
    verifiers: Vec<Box<dyn Verifier>>,
}

impl Verification {
    pub fn new() -> Self {
        Self {
            verifiers: Vec::new(),
        }
    }

    pub fn register(mut self, verifier: Box<dyn Verifier>) -> Self {
        self.verifiers.push(verifier);
        self
    }

    pub fn verify(&self, action: &Action, result: &ToolResult) -> VerifiedResult {
        let mut checks = Vec::new();
        let mut all_passed = true;

        for verifier in &self.verifiers {
            let check = verifier.verify(action, result);
            if !check.passed {
                all_passed = false;
            }
            checks.push(check);
        }

        let verified = if self.verifiers.is_empty() {
            result.success
        } else {
            all_passed
        };

        let confidence = if checks.is_empty() {
            Confidence::medium()
        } else {
            let passed = checks.iter().filter(|c| c.passed).count();
            Confidence::from_ratio(passed as u32, checks.len() as u32)
        };

        VerifiedResult {
            verified,
            tool_success: result.success,
            checks,
            confidence,
        }
    }
}

impl Default for Verification {
    fn default() -> Self {
        Self::new()
    }
}

/// A single verification check.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Check {
    pub name: String,
    pub passed: bool,
    pub detail: String,
}

/// The result of verification — tool output interpreted.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerifiedResult {
    pub verified: bool,
    pub tool_success: bool,
    pub checks: Vec<Check>,
    /// Confidence in the verification result.
    pub confidence: Confidence,
}

/// A verifier — checks one aspect of a tool result.
#[async_trait]
pub trait Verifier: Send + Sync {
    fn name(&self) -> &str;
    fn verify(&self, action: &Action, result: &ToolResult) -> Check;
}

// ── Policy ──────────────────────────────────────────────────────────

/// A policy — an immutable, evidence-backed record that informs Planning.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Policy {
    pub id: String,
    pub kind: PolicyKind,
    pub statement: String,
    pub capability: Option<Capability>,
    pub config: serde_json::Value,
    pub confidence: f32,
    pub support: u32,
    pub created_from: Vec<String>,
    pub expires_at: Option<u64>,
    pub created_at_ms: u64,
}

impl Policy {
    pub fn prefer_tool(
        capability: Capability,
        preferred_tool: impl Into<String>,
        confidence: f32,
        support: u32,
    ) -> Self {
        let tool_name = preferred_tool.into();
        Self {
            id: format!("policy_{}", uuid::Uuid::new_v4()),
            kind: PolicyKind::PreferTool,
            statement: format!("Prefer {} for {}", tool_name, capability),
            capability: Some(capability),
            config: serde_json::json!({ "preferred_tool": tool_name }),
            confidence,
            support,
            created_from: Vec::new(),
            expires_at: None,
            created_at_ms: chrono::Utc::now().timestamp_millis() as u64,
        }
    }

    pub fn retry(
        capability: Capability,
        max_retries: u32,
        confidence: f32,
        support: u32,
    ) -> Self {
        Self {
            id: format!("policy_{}", uuid::Uuid::new_v4()),
            kind: PolicyKind::Retry,
            statement: format!("Retry {} up to {} times", capability, max_retries),
            capability: Some(capability),
            config: serde_json::json!({ "max_retries": max_retries }),
            confidence,
            support,
            created_from: Vec::new(),
            expires_at: None,
            created_at_ms: chrono::Utc::now().timestamp_millis() as u64,
        }
    }

    pub fn sequence(
        from: Capability,
        then: Capability,
        confidence: f32,
        support: u32,
    ) -> Self {
        Self {
            id: format!("policy_{}", uuid::Uuid::new_v4()),
            kind: PolicyKind::Sequence,
            statement: format!("{} usually follows {}", then, from),
            capability: Some(from),
            config: serde_json::json!({ "then": then }),
            confidence,
            support,
            created_from: Vec::new(),
            expires_at: None,
            created_at_ms: chrono::Utc::now().timestamp_millis() as u64,
        }
    }

    pub fn is_expired(&self, now_ms: u64) -> bool {
        match self.expires_at {
            Some(exp) => now_ms > exp,
            None => false,
        }
    }

    pub fn with_created_from(mut self, insight_ids: Vec<String>) -> Self {
        self.created_from = insight_ids;
        self
    }

    pub fn with_expires_at(mut self, expires_at_ms: u64) -> Self {
        self.expires_at = Some(expires_at_ms);
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PolicyKind {
    PreferTool,
    Retry,
    Sequence,
    Skip,
    Custom(String),
}

// ── Learning Engine ─────────────────────────────────────────────────

/// Learning — consumes Insights, produces Policies.
pub struct LearningEngine {
    policies: Vec<Policy>,
}

impl LearningEngine {
    pub fn new() -> Self {
        Self {
            policies: Vec::new(),
        }
    }

    pub fn learn(&mut self, insights: &[Insight]) -> Vec<Policy> {
        let mut new_policies = Vec::new();

        for insight in insights {
            if let Some(policy) = self.derive_policy(insight) {
                new_policies.push(policy);
            }
        }

        self.policies.extend(new_policies.clone());
        new_policies
    }

    fn derive_policy(&self, insight: &Insight) -> Option<Policy> {
        match insight.kind {
            InsightKind::Lesson => {
                Some(Policy::retry(
                    Capability::ExecuteCommand,
                    1,
                    insight.confidence,
                    1,
                ).with_created_from(vec![insight.id.clone()]))
            }
            InsightKind::Reinforcement => None,
            InsightKind::Warning => None,
            InsightKind::Optimization => None,
        }
    }

    pub fn policies(&self) -> &[Policy] {
        &self.policies
    }

    pub fn policy_count(&self) -> usize {
        self.policies.len()
    }

    pub fn policies_for(&self, capability: &Capability) -> Vec<&Policy> {
        self.policies
            .iter()
            .filter(|p| p.capability.as_ref() == Some(capability))
            .collect()
    }

    pub fn prune_expired(&mut self, now_ms: u64) {
        self.policies.retain(|p| !p.is_expired(now_ms));
    }
}

impl Default for LearningEngine {
    fn default() -> Self {
        Self::new()
    }
}

// ── Plan ────────────────────────────────────────────────────────────

/// A plan — a structured representation of how to achieve a goal.
///
/// ```text
/// Plan {
///     id,
///     goal_id,
///     strategy,
///     actions,
///     estimated_cost,
///     estimated_duration_ms,
///     confidence,
///     prerequisites,
/// }
/// ```
///
/// A plan wraps actions with metadata that the evaluator can reason about.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Plan {
    pub id: String,
    pub goal_id: String,
    /// Human-readable strategy description.
    pub strategy: String,
    /// The ordered actions in this plan.
    pub actions: Vec<Action>,
    /// Estimated cost (arbitrary units, lower = cheaper).
    pub estimated_cost: f64,
    /// Estimated duration in milliseconds.
    pub estimated_duration_ms: u64,
    /// Confidence that this plan will succeed [0.0, 1.0].
    pub confidence: f32,
    /// Prerequisites that must be true for this plan to work.
    pub prerequisites: Vec<String>,
    /// The source of this plan (which generator produced it).
    pub source: PlanSource,
}

/// Where a plan came from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum PlanSource {
    /// Direct mapping from goal to actions (default).
    Direct,
    /// Generated from a sequencing policy.
    PolicyDriven,
    /// Generated from knowledge patterns.
    KnowledgeBased,
    /// Generated from simulation.
    Simulated,
    /// Custom source.
    Custom(String),
}

impl Plan {
    /// Create a plan from a single action (simplest case).
    pub fn from_action(action: Action, strategy: impl Into<String>) -> Self {
        let strategy = strategy.into();
        Self {
            id: format!("plan_{}", uuid::Uuid::new_v4()),
            goal_id: action.goal_id.clone(),
            strategy,
            actions: vec![action],
            estimated_cost: 1.0,
            estimated_duration_ms: 1000,
            confidence: 0.5,
            prerequisites: Vec::new(),
            source: PlanSource::Direct,
        }
    }

    /// Create a plan from multiple actions.
    pub fn from_actions(
        actions: Vec<Action>,
        strategy: impl Into<String>,
        source: PlanSource,
    ) -> Self {
        let goal_id = actions.first().map(|a| a.goal_id.clone()).unwrap_or_default();
        let strategy = strategy.into();
        let cost = actions.len() as f64;
        let duration = actions.len() as u64 * 1000;
        Self {
            id: format!("plan_{}", uuid::Uuid::new_v4()),
            goal_id,
            strategy,
            actions,
            estimated_cost: cost,
            estimated_duration_ms: duration,
            confidence: 0.5,
            prerequisites: Vec::new(),
            source,
        }
    }

    pub fn with_confidence(mut self, confidence: f32) -> Self {
        self.confidence = confidence;
        self
    }

    pub fn with_cost(mut self, cost: f64) -> Self {
        self.estimated_cost = cost;
        self
    }

    pub fn with_duration_ms(mut self, ms: u64) -> Self {
        self.estimated_duration_ms = ms;
        self
    }

    pub fn with_prerequisites(mut self, prereqs: Vec<String>) -> Self {
        self.prerequisites = prereqs;
        self
    }

    /// Number of actions in this plan.
    pub fn action_count(&self) -> usize {
        self.actions.len()
    }

    /// Whether this plan has prerequisites.
    pub fn has_prerequisites(&self) -> bool {
        !self.prerequisites.is_empty()
    }
}

// ── Planning Context ────────────────────────────────────────────────

/// Planning context — the complete inputs to a planning decision.
///
/// ```text
/// PlanningContext
///   ├── Goal
///   ├── Knowledge
///   ├── Policies
///   ├── WorldState
///   └── Constraints
/// ```
///
/// Planning is a deterministic transformation:
///
/// ```text
/// PlanningContext → Planning → Vec<PlanCandidate>
/// ```
#[derive(Debug, Clone)]
pub struct PlanningContext {
    pub goal: Goal,
    pub knowledge: Vec<KnowledgeRecord>,
    pub policies: Vec<Policy>,
    pub world_state: WorldState,
    pub constraints: Constraints,
}

/// Constraints on planning.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Constraints {
    pub max_actions: Option<u32>,
    pub timeout_ms: Option<u64>,
    pub required_permissions: Vec<String>,
    pub excluded_capabilities: Vec<Capability>,
}

impl PlanningContext {
    pub fn new(goal: Goal) -> Self {
        Self {
            goal,
            knowledge: Vec::new(),
            policies: Vec::new(),
            world_state: WorldState::new(),
            constraints: Constraints::default(),
        }
    }

    pub fn with_knowledge(mut self, knowledge: Vec<KnowledgeRecord>) -> Self {
        self.knowledge = knowledge;
        self
    }

    pub fn with_policies(mut self, policies: Vec<Policy>) -> Self {
        self.policies = policies;
        self
    }

    pub fn with_world_state(mut self, world_state: WorldState) -> Self {
        self.world_state = world_state;
        self
    }

    pub fn with_constraints(mut self, constraints: Constraints) -> Self {
        self.constraints = constraints;
        self
    }
}

// ── Plan Candidate ──────────────────────────────────────────────────

/// A plan candidate — a plan with preliminary evaluation.
///
/// Generated by Planning, consumed by PlanEvaluator.
#[derive(Debug, Clone)]
pub struct PlanCandidate {
    pub plan: Plan,
    /// Preliminary score from the planner [0.0, 1.0].
    pub preliminary_score: f32,
}

// ── Ranked Plan ─────────────────────────────────────────────────────

/// A ranked plan — the output of PlanEvaluator.
///
/// ```text
/// PlanEvaluator
///   input:  Vec<PlanCandidate>
///   output: RankedPlan
/// ```
#[derive(Debug, Clone)]
pub struct RankedPlan {
    /// The chosen plan.
    pub plan: Plan,
    /// Final evaluation score [0.0, 1.0].
    pub score: f32,
    /// All candidates that were considered, sorted by score descending.
    pub all_candidates: Vec<PlanCandidate>,
    /// Why this plan was chosen (human-readable).
    pub rationale: String,
}

// ── Plan Evaluator ──────────────────────────────────────────────────

/// Plan evaluator — scores and ranks candidate plans.
///
/// ```text
/// Vec<PlanCandidate> → PlanEvaluator → RankedPlan
/// ```
///
/// The evaluator considers:
/// - Success probability (confidence)
/// - Cost (lower is better)
/// - Time (shorter is better)
/// - Policy alignment
/// - Prerequisites met
/// - World state compatibility
///
/// The evaluator is deterministic: same candidates → same ranking.
pub struct PlanEvaluator;

impl PlanEvaluator {
    pub fn new() -> Self {
        Self
    }

    /// Evaluate and rank candidate plans. Returns the best plan.
    ///
    /// Panics if `candidates` is empty.
    pub fn evaluate(&self, candidates: &[PlanCandidate], world: &WorldState) -> RankedPlan {
        assert!(!candidates.is_empty(), "Cannot evaluate empty candidates");

        let mut scored: Vec<(PlanCandidate, f32)> = candidates
            .iter()
            .map(|c| {
                let score = self.score(c, world);
                (c.clone(), score)
            })
            .collect();

        scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

        let best = scored.remove(0);
        let rationale = self.explain(&best.0.plan, best.1, world);

        let all_candidates: Vec<PlanCandidate> = scored
            .into_iter()
            .map(|(c, _)| c)
            .collect();

        RankedPlan {
            plan: best.0.plan,
            score: best.1,
            all_candidates,
            rationale,
        }
    }

    /// Score a single candidate against the world state.
    fn score(&self, candidate: &PlanCandidate, world: &WorldState) -> f32 {
        let plan = &candidate.plan;

        // Base score from plan confidence
        let mut score = plan.confidence;

        // Cost penalty: more actions = higher cost
        let cost_factor = 1.0 / (1.0 + plan.actions.len() as f32 * 0.1);
        score *= cost_factor;

        // Prerequisite penalty: unmet prerequisites reduce score
        let prereqs_met = plan
            .prerequisites
            .iter()
            .all(|p| self.prerequisite_met(p, world));
        if !prereqs_met {
            score *= 0.3; // heavy penalty for unmet prerequisites
        }

        // World state bonus: if tools are available
        let tools_available = plan.actions.iter().all(|a| world.has_tool(&a.capability));
        if tools_available {
            score *= 1.1; // slight bonus
        } else {
            score *= 0.5; // penalty for missing tools
        }

        // Clamp to [0.0, 1.0]
        score.clamp(0.0, 1.0)
    }

    /// Check if a prerequisite is met in the world state.
    fn prerequisite_met(&self, prereq: &str, world: &WorldState) -> bool {
        // Simple prefix-based matching
        if let Some(path) = prereq.strip_prefix("file_exists:") {
            return world.file_exists(path);
        }
        if let Some(tool) = prereq.strip_prefix("has_tool:") {
            // Parse capability from string
            return world.available_tools.iter().any(|c| c.to_string() == tool);
        }
        // Default: assume met
        true
    }

    /// Explain why a plan was chosen.
    fn explain(&self, plan: &Plan, score: f32, world: &WorldState) -> String {
        let tools_ok = plan
            .actions
            .iter()
            .all(|a| world.has_tool(&a.capability));
        let prereqs_ok = plan
            .prerequisites
            .iter()
            .all(|p| self.prerequisite_met(p, world));

        format!(
            "Plan '{}' scored {:.2}. Actions: {}. Tools available: {}. Prerequisites met: {}. Source: {:?}.",
            plan.strategy, score, plan.action_count(), tools_ok, prereqs_ok, plan.source
        )
    }
}

impl Default for PlanEvaluator {
    fn default() -> Self {
        Self::new()
    }
}

// ── Planning ────────────────────────────────────────────────────────

/// Planning — generates candidate plans from a PlanningContext.
///
/// ```text
/// PlanningContext → Planning → Vec<PlanCandidate>
/// ```
///
/// Planning generates multiple hypotheses. PlanEvaluator chooses the best.
/// Planning is declarative: it produces candidates, never executes.
pub struct Planning {
    #[allow(dead_code)]
    knowledge_engine: KnowledgeEngine,
}

impl Planning {
    pub fn new() -> Self {
        Self {
            knowledge_engine: KnowledgeEngine::new(),
        }
    }

    /// Generate candidate plans from a planning context.
    ///
    /// This produces multiple candidates for the evaluator to rank.
    pub fn think(&self, context: &PlanningContext) -> Vec<PlanCandidate> {
        let mut candidates = Vec::new();

        // Candidate 1: Direct plan (single action)
        candidates.push(self.plan_direct(context));

        // Candidate 2: Policy-driven plan (if sequencing policy exists)
        if let Some(policy_candidate) = self.plan_from_policy(context) {
            candidates.push(policy_candidate);
        }

        // Candidate 3: Knowledge-aware plan (if knowledge suggests a pattern)
        if let Some(knowledge_candidate) = self.plan_from_knowledge(context) {
            candidates.push(knowledge_candidate);
        }

        candidates
    }

    /// Simple single-action plan.
    fn plan_direct(&self, context: &PlanningContext) -> PlanCandidate {
        let capability = self.infer_capability(&context.goal.description, &context.knowledge);
        let action = Action::single(&context.goal.description, capability, &context.goal.id);
        let plan = Plan::from_action(action, "Direct action from goal description");
        PlanCandidate {
            preliminary_score: 0.5,
            plan,
        }
    }

    /// Plan driven by a sequencing policy.
    fn plan_from_policy(&self, context: &PlanningContext) -> Option<PlanCandidate> {
        let seq_policy = context.policies.iter().find(|p| p.kind == PolicyKind::Sequence)?;

        let from_cap = seq_policy.capability.clone().unwrap_or(Capability::ExecuteCommand);
        let then_cap: Capability = serde_json::from_value(
            seq_policy.config.get("then").cloned().unwrap_or(serde_json::json!("execute_command")),
        )
        .unwrap_or(Capability::ExecuteCommand);

        let seq_label = format!("Sequence: {} then {}", from_cap, then_cap);

        let actions = vec![
            Action::step(&context.goal.description, from_cap, &context.goal.id, 0, 2),
            Action::step(&context.goal.description, then_cap, &context.goal.id, 1, 2),
        ];

        let mut plan = Plan::from_actions(
            actions,
            seq_label,
            PlanSource::PolicyDriven,
        );
        plan.confidence = seq_policy.confidence;

        Some(PlanCandidate {
            preliminary_score: seq_policy.confidence,
            plan,
        })
    }

    /// Plan derived from knowledge patterns.
    fn plan_from_knowledge(&self, context: &PlanningContext) -> Option<PlanCandidate> {
        if context.knowledge.is_empty() {
            return None;
        }

        // Find the most supported knowledge record
        let best = context
            .knowledge
            .iter()
            .max_by_key(|k| k.support)?;

        if best.support < 2 {
            return None; // need at least 2 observations
        }

        let capability = self.infer_capability(&best.label, &context.knowledge);
        let action = Action::single(&best.label, capability, &context.goal.id);
        let mut plan = Plan::from_action(action, format!("Based on knowledge: {}", best.label));
        plan.source = PlanSource::KnowledgeBased;
        plan.confidence = best.confidence;
        plan.estimated_cost = 0.5; // knowledge-derived plans are cheaper

        Some(PlanCandidate {
            preliminary_score: best.confidence,
            plan,
        })
    }

    /// Legacy entry point: single plan (backward compatible).
    pub fn plan(&self, context: &PlanningContext) -> Vec<Action> {
        let candidates = self.think(context);
        candidates
            .into_iter()
            .next()
            .map(|c| c.plan.actions)
            .unwrap_or_default()
    }

    fn infer_capability(&self, description: &str, _knowledge: &[KnowledgeRecord]) -> Capability {
        let desc_lower = description.to_lowercase();
        if desc_lower.contains("cargo")
            || desc_lower.contains("build")
            || desc_lower.contains("test")
            || desc_lower.contains("run")
        {
            Capability::ExecuteCommand
        } else if desc_lower.contains("read") || desc_lower.contains("open") {
            Capability::ReadFile
        } else if desc_lower.contains("write") || desc_lower.contains("create") {
            Capability::WriteFile
        } else if desc_lower.contains("list") || desc_lower.contains("ls") {
            Capability::ListDirectory
        } else if desc_lower.contains("delete") || desc_lower.contains("remove") {
            Capability::DeleteFile
        } else if desc_lower.contains("search") || desc_lower.contains("find") {
            Capability::SearchWeb
        } else {
            Capability::ExecuteCommand
        }
    }
}

impl Default for Planning {
    fn default() -> Self {
        Self::new()
    }
}

// ── Goal Engine ─────────────────────────────────────────────────────

/// The Goal Engine — owns how an Action becomes an Outcome.
///
/// **Invariant**: Every execution produces exactly one Outcome.
pub struct GoalEngine {
    registry: ToolRegistry,
    verification: Verification,
    knowledge_engine: KnowledgeEngine,
    reflection_engine: ReflectionEngine,
}

impl GoalEngine {
    pub fn new() -> Self {
        Self {
            registry: ToolRegistry::new(),
            verification: Verification::new(),
            knowledge_engine: KnowledgeEngine::new(),
            reflection_engine: ReflectionEngine::new(),
        }
    }

    pub fn with_registry(mut self, registry: ToolRegistry) -> Self {
        self.registry = registry;
        self
    }

    pub fn with_verification(mut self, verification: Verification) -> Self {
        self.verification = verification;
        self
    }

    pub async fn execute(
        &mut self,
        goal: &Goal,
        action: &Action,
        expectation: &Expectation,
        context: &ExecutionContext,
    ) -> GoalReport {
        let started_at = chrono::Utc::now().timestamp_millis() as u64;
        let mut cognitive_trace = CognitiveTrace::new(&goal.id);

        // Trace: Execution start
        let exec_node = TraceNode::new(
            TraceNodeKind::Execution,
            serde_json::json!({
                "action_id": action.id,
                "instruction": action.instruction,
                "capability": action.capability.to_string(),
            }),
            serde_json::json!({}),
        ).with_timing(started_at, started_at);
        cognitive_trace.push(exec_node);

        let tool = self.registry.resolve(&action.capability);
        let (tool_result, tool_name) = match tool {
            Some(t) => {
                let result = t.execute(action, context).await;
                (result, t.name().to_string())
            }
            None => (
                ToolResult::error(format!("no tool for capability: {}", action.capability)),
                String::new(),
            ),
        };

        let tool_finished = chrono::Utc::now().timestamp_millis() as u64;

        // Trace: Verification
        let verified = self.verification.verify(action, &tool_result);
        let verify_node = TraceNode::new(
            TraceNodeKind::Verification,
            serde_json::json!({
                "tool_success": verified.tool_success,
                "checks": verified.checks.iter().map(|c| serde_json::json!({
                    "name": c.name,
                    "passed": c.passed,
                })).collect::<Vec<_>>(),
            }),
            serde_json::json!({
                "verified": verified.verified,
            }),
        ).with_timing(tool_finished, chrono::Utc::now().timestamp_millis() as u64);
        cognitive_trace.push(verify_node);

        // Trace: Reflection
        let outcome = self.produce_outcome(goal, expectation, &verified, started_at);
        let episodes = self.create_episodes(goal, action, &tool_name, expectation, &verified);
        let insights = self.reflect(&episodes, &outcome);

        let reflect_node = TraceNode::new(
            TraceNodeKind::Reflection,
            serde_json::json!({
                "outcome_kind": format!("{:?}", outcome.kind),
                "episodes": episodes.len(),
            }),
            serde_json::json!({
                "insights": insights.len(),
                "insight_kinds": insights.iter().map(|i| format!("{:?}", i.kind)).collect::<Vec<_>>(),
            }),
        ).with_timing(
            chrono::Utc::now().timestamp_millis() as u64,
            chrono::Utc::now().timestamp_millis() as u64,
        );
        cognitive_trace.push(reflect_node);

        GoalReport {
            goal: goal.clone(),
            action: action.clone(),
            expectation: expectation.clone(),
            tool: tool_name,
            verification: verified,
            outcome,
            insights,
            episodes,
            trace: Trace {
                started_at_ms: started_at,
                finished_at_ms: chrono::Utc::now().timestamp_millis() as u64,
            },
            cognitive_trace,
        }
    }

    fn produce_outcome(
        &self,
        goal: &Goal,
        expectation: &Expectation,
        verified: &VerifiedResult,
        started_at: u64,
    ) -> Outcome {
        let kind = if (verified.verified && expectation.expect_success)
            || (!verified.verified && !expectation.expect_success)
        {
            OutcomeKind::TaskCompleted
        } else if verified.verified && !expectation.expect_success {
            OutcomeKind::UnexpectedSuccess
        } else {
            OutcomeKind::Error
        };

        let duration_ms = chrono::Utc::now().timestamp_millis() as u64 - started_at;

        Outcome {
            id: format!("outcome_{}", uuid::Uuid::new_v4()),
            kind,
            context: goal.description.clone(),
            detail: serde_json::json!({
                "expectation": expectation.description,
                "expected_success": expectation.expect_success,
                "tool_success": verified.tool_success,
                "verified": verified.verified,
                "checks": verified.checks,
                "duration_ms": duration_ms,
            }),
            occurred_at_ms: chrono::Utc::now().timestamp_millis() as u64,
        }
    }

    fn create_episodes(
        &self,
        goal: &Goal,
        action: &Action,
        tool_name: &str,
        expectation: &Expectation,
        verified: &VerifiedResult,
    ) -> Vec<Episode> {
        let mut episodes = Vec::new();

        episodes.push(Episode::from_event(friday_cognitive::WorkspaceEvent::new(
            friday_cognitive::workspace::Topic::GoalCreated,
            serde_json::json!({
                "goal_id": goal.id,
                "description": goal.description,
                "action_id": action.id,
                "instruction": action.instruction,
                "capability": action.capability.to_string(),
            }),
        )));

        episodes.push(Episode::from_event(friday_cognitive::WorkspaceEvent::new(
            friday_cognitive::workspace::Topic::ToolExecuted,
            serde_json::json!({
                "goal_id": goal.id,
                "action_id": action.id,
                "tool": tool_name,
                "tool_success": verified.tool_success,
                "verified": verified.verified,
            }),
        )));

        episodes.push(Episode::from_event(friday_cognitive::WorkspaceEvent::new(
            friday_cognitive::workspace::Topic::OutcomeObserved,
            serde_json::json!({
                "goal_id": goal.id,
                "action_id": action.id,
                "verified": verified.verified,
                "expected_success": expectation.expect_success,
            }),
        )));

        episodes
    }

    fn reflect(&mut self, episodes: &[Episode], outcome: &Outcome) -> Vec<Insight> {
        let knowledge = self.knowledge_engine.analyze(episodes);

        if knowledge.is_empty() {
            let synthetic = KnowledgeRecord {
                id: format!("k_goal_{}", outcome.id),
                kind: friday_knowledge::KnowledgeKind::Workflow,
                label: outcome.context.clone(),
                description: format!("Goal execution: {}", outcome.context),
                pattern: vec![outcome.context.clone()],
                support: 1,
                confidence: 1.0,
                derived_at_ms: chrono::Utc::now().timestamp_millis() as u64,
            };
            return self.reflection_engine.reflect_all(&[synthetic], outcome);
        }

        self.reflection_engine.reflect_all(&knowledge, outcome)
    }

    pub fn registry(&self) -> &ToolRegistry {
        &self.registry
    }

    pub fn reflection_engine(&self) -> &ReflectionEngine {
        &self.reflection_engine
    }
}

impl Default for GoalEngine {
    fn default() -> Self {
        Self::new()
    }
}

// ── Goal Report ─────────────────────────────────────────────────────

/// A complete audit of one goal execution.
#[derive(Debug, Clone)]
pub struct GoalReport {
    pub goal: Goal,
    pub action: Action,
    pub expectation: Expectation,
    pub tool: String,
    pub verification: VerifiedResult,
    pub outcome: Outcome,
    pub insights: Vec<Insight>,
    pub episodes: Vec<Episode>,
    pub trace: Trace,
    pub cognitive_trace: CognitiveTrace,
}

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

impl GoalReport {
    pub fn success(&self) -> bool {
        self.outcome.kind == OutcomeKind::TaskCompleted
            || self.outcome.kind == OutcomeKind::MissionSucceeded
    }

    pub fn unexpected(&self) -> bool {
        self.outcome.kind == OutcomeKind::UnexpectedSuccess
            || self.outcome.kind == OutcomeKind::UnexpectedFailure
    }
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
            status: if complete { MissionStatus::Complete } else { MissionStatus::Created },
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

// ── Mock Tool ───────────────────────────────────────────────────────

pub struct MockTool {
    name: String,
    capabilities: Vec<Capability>,
    should_succeed: bool,
    output: String,
}

impl MockTool {
    pub fn success(name: impl Into<String>, output: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            capabilities: vec![Capability::ExecuteCommand],
            should_succeed: true,
            output: output.into(),
        }
    }

    pub fn failure(name: impl Into<String>, error: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            capabilities: vec![Capability::ExecuteCommand],
            should_succeed: false,
            output: error.into(),
        }
    }

    pub fn with_capabilities(mut self, caps: Vec<Capability>) -> Self {
        self.capabilities = caps;
        self
    }
}

#[async_trait]
impl Tool for MockTool {
    fn name(&self) -> &str { &self.name }
    fn description(&self) -> &str { "Mock tool for testing" }
    fn capabilities(&self) -> Vec<Capability> { self.capabilities.clone() }

    async fn execute(&self, _action: &Action, _context: &ExecutionContext) -> ToolResult {
        if self.should_succeed {
            ToolResult::success(&self.output)
        } else {
            ToolResult::error(&self.output)
        }
    }
}

// ── Mock Verifier ───────────────────────────────────────────────────

pub struct MockVerifier {
    name: String,
    should_pass: bool,
}

impl MockVerifier {
    pub fn pass(name: impl Into<String>) -> Self {
        Self { name: name.into(), should_pass: true }
    }

    pub fn fail(name: impl Into<String>) -> Self {
        Self { name: name.into(), should_pass: false }
    }
}

impl Verifier for MockVerifier {
    fn name(&self) -> &str { &self.name }

    fn verify(&self, _action: &Action, _result: &ToolResult) -> Check {
        Check {
            name: self.name.clone(),
            passed: self.should_pass,
            detail: if self.should_pass { "check passed".into() } else { "check failed".into() },
        }
    }
}

// ── Tests ───────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── Goal ────────────────────────────────────────────────────

    #[test]
    fn goal_creation() {
        let goal = Goal::new("open notepad");
        assert_eq!(goal.description, "open notepad");
        assert!(goal.context.is_none());
        assert!(goal.id.starts_with("goal_"));
    }

    #[test]
    fn goal_with_context() {
        let goal = Goal::new("build project").with_context("before commit");
        assert_eq!(goal.context.unwrap(), "before commit");
    }

    // ── Action ──────────────────────────────────────────────────

    #[test]
    fn action_single() {
        let goal = Goal::new("build project");
        let action = Action::single("cargo build", Capability::ExecuteCommand, &goal.id);
        assert_eq!(action.instruction, "cargo build");
        assert_eq!(action.capability, Capability::ExecuteCommand);
        assert!(action.is_first());
        assert!(action.is_last());
        assert!(!action.is_multi_step());
    }

    #[test]
    fn action_multi_step() {
        let goal = Goal::new("build project");
        let a1 = Action::step("cargo fmt", Capability::ExecuteCommand, &goal.id, 0, 3);
        let a3 = Action::step("cargo build", Capability::ExecuteCommand, &goal.id, 2, 3);
        assert!(a1.is_first());
        assert!(!a1.is_last());
        assert!(a3.is_last());
    }

    // ── Capability ──────────────────────────────────────────────

    #[test]
    fn capability_display() {
        assert_eq!(Capability::ExecuteCommand.to_string(), "execute_command");
        assert_eq!(Capability::ReadFile.to_string(), "read_file");
    }

    // ── Expectation ─────────────────────────────────────────────

    #[test]
    fn expectation_success() {
        let exp = Expectation::success("notepad will open");
        assert!(exp.expect_success);
    }

    #[test]
    fn expectation_failure() {
        let exp = Expectation::failure("build will fail");
        assert!(!exp.expect_success);
    }

    // ── World State ─────────────────────────────────────────────

    #[test]
    fn world_state_file_exists() {
        let world = WorldState::new()
            .with_file("/tmp/test.txt", FileMeta {
                path: "/tmp/test.txt".into(),
                exists: true,
                size_bytes: Some(100),
                is_directory: false,
                modified_at_ms: None,
            });

        assert!(world.file_exists("/tmp/test.txt"));
        assert!(!world.file_exists("/tmp/missing.txt"));
    }

    #[test]
    fn world_state_has_tool() {
        let world = WorldState::new()
            .with_tool(Capability::ExecuteCommand)
            .with_tool(Capability::ReadFile);

        assert!(world.has_tool(&Capability::ExecuteCommand));
        assert!(world.has_tool(&Capability::ReadFile));
        assert!(!world.has_tool(&Capability::DeleteFile));
    }

    // ── Plan ────────────────────────────────────────────────────

    #[test]
    fn plan_from_action() {
        let goal = Goal::new("cargo build");
        let action = Action::single("cargo build", Capability::ExecuteCommand, &goal.id);
        let plan = Plan::from_action(action, "Direct build");

        assert_eq!(plan.action_count(), 1);
        assert_eq!(plan.source, PlanSource::Direct);
        assert!(!plan.has_prerequisites());
    }

    #[test]
    fn plan_from_actions() {
        let goal = Goal::new("build project");
        let actions = vec![
            Action::step("cargo fmt", Capability::ExecuteCommand, &goal.id, 0, 2),
            Action::step("cargo build", Capability::ExecuteCommand, &goal.id, 1, 2),
        ];
        let plan = Plan::from_actions(actions, "Format then build", PlanSource::PolicyDriven);

        assert_eq!(plan.action_count(), 2);
        assert_eq!(plan.source, PlanSource::PolicyDriven);
        assert_eq!(plan.estimated_cost, 2.0);
    }

    // ── Execution Context ───────────────────────────────────────

    #[test]
    fn execution_context_for_goal() {
        let ctx = ExecutionContext::for_goal("g1");
        assert_eq!(ctx.goal_id, "g1");
        assert!(ctx.mission_id.is_none());
    }

    // ── Tool Result ─────────────────────────────────────────────

    #[test]
    fn tool_result_success() {
        let r = ToolResult::success("hello");
        assert!(r.success);
        assert_eq!(r.output, "hello");
    }

    #[test]
    fn tool_result_error() {
        let r = ToolResult::error("not found");
        assert!(!r.success);
    }

    // ── Tool Registry ───────────────────────────────────────────

    #[test]
    fn registry_resolve_by_capability() {
        let mut reg = ToolRegistry::new();
        reg.register(Box::new(MockTool::success("terminal", "ok")));
        reg.register(Box::new(
            MockTool::success("filesystem", "ok")
                .with_capabilities(vec![Capability::ReadFile]),
        ));

        assert_eq!(reg.resolve(&Capability::ExecuteCommand).unwrap().name(), "terminal");
        assert_eq!(reg.resolve(&Capability::ReadFile).unwrap().name(), "filesystem");
        assert!(reg.resolve(&Capability::DeleteFile).is_none());
    }

    // ── Verification ────────────────────────────────────────────

    #[test]
    fn verification_no_verifiers_trusts_tool() {
        let verification = Verification::new();
        let action = Action::single("echo hi", Capability::ExecuteCommand, "g1");
        let result = ToolResult::success("output");
        let verified = verification.verify(&action, &result);
        assert!(verified.verified);
    }

    #[test]
    fn verification_failing_verifier() {
        let verification = Verification::new()
            .register(Box::new(MockVerifier::fail("checksum")));
        let action = Action::single("download", Capability::ExecuteCommand, "g1");
        let result = ToolResult::success("content");
        let verified = verification.verify(&action, &result);
        assert!(!verified.verified);
        assert!(verified.tool_success);
    }

    // ── Policy ──────────────────────────────────────────────────

    #[test]
    fn policy_prefer_tool() {
        let p = Policy::prefer_tool(Capability::ReadFile, "filesystem", 0.9, 5);
        assert_eq!(p.kind, PolicyKind::PreferTool);
        assert!(p.statement.contains("filesystem"));
    }

    #[test]
    fn policy_retry() {
        let p = Policy::retry(Capability::ExecuteCommand, 2, 0.8, 3);
        assert_eq!(p.kind, PolicyKind::Retry);
    }

    #[test]
    fn policy_immutable_with_evidence() {
        let p = Policy::retry(Capability::ExecuteCommand, 1, 0.8, 3)
            .with_created_from(vec!["i1".into()])
            .with_expires_at(1000);
        assert_eq!(p.created_from, vec!["i1"]);
        assert!(!p.is_expired(999));
        assert!(p.is_expired(1001));
    }

    // ── Learning Engine ─────────────────────────────────────────

    #[test]
    fn learning_from_lesson_produces_retry_policy() {
        let mut engine = LearningEngine::new();
        let insight = Insight {
            id: "i1".into(),
            kind: InsightKind::Lesson,
            knowledge_id: "k1".into(),
            outcome_id: "o1".into(),
            statement: "cargo build failed".into(),
            confidence: 0.8,
            derived_at_ms: 1000,
        };
        let policies = engine.learn(&[insight]);
        assert_eq!(policies.len(), 1);
        assert_eq!(policies[0].kind, PolicyKind::Retry);
    }

    #[test]
    fn learning_prune_expired() {
        let mut engine = LearningEngine::new();
        engine.learn(&[Insight {
            id: "i1".into(),
            kind: InsightKind::Lesson,
            knowledge_id: "k1".into(),
            outcome_id: "o1".into(),
            statement: "failed".into(),
            confidence: 0.8,
            derived_at_ms: 1000,
        }]);
        assert_eq!(engine.policy_count(), 1);

        let policy_id = engine.policies()[0].id.clone();
        engine.policies.iter_mut().for_each(|p| {
            if p.id == policy_id { p.expires_at = Some(100); }
        });

        engine.prune_expired(50);
        assert_eq!(engine.policy_count(), 1);
        engine.prune_expired(150);
        assert_eq!(engine.policy_count(), 0);
    }

    // ── Plan Evaluator ──────────────────────────────────────────

    #[test]
    fn evaluator_prefers_higher_confidence() {
        let evaluator = PlanEvaluator::new();
        let world = WorldState::new()
            .with_tool(Capability::ExecuteCommand);

        let goal = Goal::new("cargo build");
        let action = Action::single("cargo build", Capability::ExecuteCommand, &goal.id);

        let c1 = PlanCandidate {
            plan: Plan::from_action(action.clone(), "Low confidence").with_confidence(0.3),
            preliminary_score: 0.3,
        };
        let c2 = PlanCandidate {
            plan: Plan::from_action(action, "High confidence").with_confidence(0.9),
            preliminary_score: 0.9,
        };

        let ranked = evaluator.evaluate(&[c1, c2], &world);
        assert!(ranked.score > 0.5);
        assert!(ranked.plan.strategy.contains("High confidence"));
    }

    #[test]
    fn evaluator_penalizes_unmet_prerequisites() {
        let evaluator = PlanEvaluator::new();
        let world = WorldState::new();

        let goal = Goal::new("cargo build");
        let action = Action::single("cargo build", Capability::ExecuteCommand, &goal.id);

        let plan_with_prereq = Plan::from_action(action, "Needs file")
            .with_prerequisites(vec!["file_exists:/tmp/required.txt".into()])
            .with_confidence(0.9);

        let plan_no_prereq = Plan::from_action(
            Action::single("cargo build", Capability::ExecuteCommand, &goal.id),
            "No prerequisites",
        ).with_confidence(0.7);

        let ranked = evaluator.evaluate(
            &[
                PlanCandidate { plan: plan_with_prereq, preliminary_score: 0.9 },
                PlanCandidate { plan: plan_no_prereq, preliminary_score: 0.7 },
            ],
            &world,
        );

        // Plan with unmet prereqs should lose despite higher confidence
        assert!(ranked.plan.strategy.contains("No prerequisites"));
    }

    #[test]
    fn evaluator_penalizes_missing_tools() {
        let evaluator = PlanEvaluator::new();
        let world = WorldState::new(); // no tools

        let goal = Goal::new("read file");
        let action = Action::single("read file", Capability::ReadFile, &goal.id);

        let plan = Plan::from_action(action, "Read without tool").with_confidence(0.9);
        let ranked = evaluator.evaluate(
            &[PlanCandidate { plan, preliminary_score: 0.9 }],
            &world,
        );

        // Score should be penalized
        assert!(ranked.score < 0.5);
    }

    #[test]
    fn evaluator_empty_panics() {
        let evaluator = PlanEvaluator::new();
        let world = WorldState::new();
        let result = std::panic::catch_unwind(|| {
            evaluator.evaluate(&[], &world);
        });
        assert!(result.is_err());
    }

    // ── Planning (think) ───────────────────────────────────────

    #[test]
    fn think_generates_candidates() {
        let planning = Planning::new();
        let goal = Goal::new("cargo build");
        let ctx = PlanningContext::new(goal);
        let candidates = planning.think(&ctx);

        // Should generate at least 1 candidate (direct)
        assert!(!candidates.is_empty());
        assert_eq!(candidates[0].plan.source, PlanSource::Direct);
    }

    #[test]
    fn think_with_policy_generates_more_candidates() {
        let planning = Planning::new();
        let goal = Goal::new("cargo build");
        let policy = Policy::sequence(
            Capability::ExecuteCommand,
            Capability::ReadFile,
            0.8,
            5,
        );
        let ctx = PlanningContext::new(goal).with_policies(vec![policy]);
        let candidates = planning.think(&ctx);

        // Should generate 2 candidates (direct + policy)
        assert!(candidates.len() >= 2);
    }

    #[test]
    fn planning_legacy_plan_still_works() {
        let planning = Planning::new();
        let goal = Goal::new("cargo build");
        let ctx = PlanningContext::new(goal);
        let actions = planning.plan(&ctx);

        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0].capability, Capability::ExecuteCommand);
    }

    // ── Full Pipeline (Deliberative) ───────────────────────────

    #[tokio::test]
    async fn deliberative_pipeline() {
        let mut reg = ToolRegistry::new();
        reg.register(Box::new(MockTool::success("cargo", "Build complete")));
        let planning = Planning::new();
        let evaluator = PlanEvaluator::new();
        let mut engine = GoalEngine::new().with_registry(reg);
        let mut learning = LearningEngine::new();

        let goal = Goal::new("cargo build");
        let world = WorldState::new()
            .with_tool(Capability::ExecuteCommand);

        // 1. Think: generate candidates
        let ctx = PlanningContext::new(goal.clone()).with_world_state(world.clone());
        let candidates = planning.think(&ctx);
        assert!(!candidates.is_empty());

        // 2. Evaluate: choose best plan
        let ranked = evaluator.evaluate(&candidates, &world);
        assert!(ranked.score > 0.0);
        assert!(!ranked.plan.actions.is_empty());

        // 3. Execute
        let exp = Expectation::success("build will succeed");
        let exec_ctx = ExecutionContext::for_goal(&goal.id);
        let report = engine.execute(&goal, &ranked.plan.actions[0], &exp, &exec_ctx).await;
        assert!(report.success());

        // 4. Learn
        let policies = learning.learn(&report.insights);
        assert!(policies.is_empty());
    }

    // ── Mission ─────────────────────────────────────────────────

    #[test]
    fn mission_progression() {
        let mut mission = Mission::new(
            "prepare release",
            vec!["format".into(), "test".into(), "build".into()],
        );
        mission.start();
        assert_eq!(mission.next_goal(), Some(0));

        let outcome = Outcome {
            id: "o1".into(),
            kind: OutcomeKind::TaskCompleted,
            context: "format".into(),
            detail: serde_json::json!({}),
            occurred_at_ms: 1000,
        };
        mission.complete_goal(0, outcome);
        assert_eq!(mission.completed_goals(), 1);
        assert_eq!(mission.next_goal(), Some(1));
    }

    // ── WorldState Diff ─────────────────────────────────────────

    #[test]
    fn diff_detects_added_file() {
        let a = WorldState::new();
        let b = WorldState::new().with_file(
            "new.txt",
            FileMeta { path: "new.txt".into(), exists: true, size_bytes: Some(100), is_directory: false, modified_at_ms: None },
        );
        let diff = a.diff(&b);
        assert_eq!(diff.added_files, vec!["new.txt".to_string()]);
        assert!(!diff.is_empty());
    }

    #[test]
    fn diff_detects_modified_file() {
        let a = WorldState::new().with_file(
            "f.txt",
            FileMeta { path: "f.txt".into(), exists: true, size_bytes: Some(10), is_directory: false, modified_at_ms: None },
        );
        let b = WorldState::new().with_file(
            "f.txt",
            FileMeta { path: "f.txt".into(), exists: true, size_bytes: Some(20), is_directory: false, modified_at_ms: None },
        );
        let diff = a.diff(&b);
        assert_eq!(diff.modified_files, vec!["f.txt".to_string()]);
    }

    #[test]
    fn diff_same_state_is_empty() {
        let a = WorldState::new().with_variable("k", serde_json::json!(1));
        let b = WorldState::new().with_variable("k", serde_json::json!(1));
        assert!(a.diff(&b).is_empty());
    }

    #[test]
    fn diff_detects_variable_change() {
        let a = WorldState::new().with_variable("k", serde_json::json!(1));
        let b = WorldState::new().with_variable("k", serde_json::json!(2));
        let diff = a.diff(&b);
        assert_eq!(diff.changed_vars, vec!["k".to_string()]);
    }

    // ── Preconditions ───────────────────────────────────────────

    #[test]
    fn precondition_file_exists_passes() {
        let state = WorldState::new().with_file(
            "x.txt",
            FileMeta { path: "x.txt".into(), exists: true, size_bytes: None, is_directory: false, modified_at_ms: None },
        );
        assert!(Precondition::FileExists { path: "x.txt".into() }.check(&state));
    }

    #[test]
    fn precondition_file_exists_fails() {
        let state = WorldState::new();
        assert!(!Precondition::FileExists { path: "x.txt".into() }.check(&state));
    }

    #[test]
    fn precondition_file_not_exists_passes() {
        let state = WorldState::new();
        assert!(Precondition::FileNotExists { path: "x.txt".into() }.check(&state));
    }

    #[test]
    fn precondition_tool_available() {
        let state = WorldState::new().with_tool(Capability::ReadFile);
        assert!(Precondition::ToolAvailable { capability: Capability::ReadFile }.check(&state));
        assert!(!Precondition::ToolAvailable { capability: Capability::WriteFile }.check(&state));
    }

    #[test]
    fn precondition_variable_equals() {
        let state = WorldState::new().with_variable("mode", serde_json::json!("fast"));
        assert!(Precondition::VariableEquals { key: "mode".into(), value: serde_json::json!("fast") }.check(&state));
        assert!(!Precondition::VariableEquals { key: "mode".into(), value: serde_json::json!("slow") }.check(&state));
    }

    #[test]
    fn precondition_network_available() {
        let state = WorldState::default();
        assert!(!Precondition::NetworkAvailable.check(&state));
        let state = WorldState::new().with_network(true);
        assert!(Precondition::NetworkAvailable.check(&state));
    }

    #[test]
    fn precondition_describe() {
        let p = Precondition::FileExists { path: "a.txt".into() };
        assert_eq!(p.describe(), "file exists: a.txt");
        let p = Precondition::ToolAvailable { capability: Capability::ExecuteCommand };
        assert_eq!(p.describe(), "tool available: execute_command");
    }

    // ── Postconditions ──────────────────────────────────────────

    #[test]
    fn postcondition_describe() {
        let p = Postcondition::FileCreated { path: "out.txt".into() };
        assert_eq!(p.describe(), "file created: out.txt");
        let p = Postcondition::VariableSet { key: "result".into() };
        assert_eq!(p.describe(), "variable set: result");
    }

    // ── State Transition ────────────────────────────────────────

    #[test]
    fn transition_records_diff() {
        let before = WorldState::new();
        let after = WorldState::new().with_file(
            "new.txt",
            FileMeta { path: "new.txt".into(), exists: true, size_bytes: None, is_directory: false, modified_at_ms: None },
        );
        let t = StateTransition::new("a1", "g1", before, after, &[]);
        assert!(!t.diff.is_empty());
        assert!(t.preconditions_met);
        assert!(t.preconditions_failed.is_empty());
    }

    #[test]
    fn transition_records_failed_preconditions() {
        let before = WorldState::new();
        let after = WorldState::new();
        let preconditions = vec![
            Precondition::FileExists { path: "missing.txt".into() },
            Precondition::ToolAvailable { capability: Capability::ReadFile },
        ];
        let t = StateTransition::new("a1", "g1", before, after, &preconditions);
        assert!(!t.preconditions_met);
        assert_eq!(t.preconditions_failed.len(), 2);
    }

    // ── World Model ─────────────────────────────────────────────

    #[test]
    fn world_model_check_preconditions() {
        let model = WorldModel::new(WorldState::new().with_tool(Capability::ReadFile));
        let failed = model.check_preconditions(&[
            Precondition::ToolAvailable { capability: Capability::ReadFile },
            Precondition::ToolAvailable { capability: Capability::WriteFile },
        ]);
        assert_eq!(failed.len(), 1);
        assert!(failed[0].contains("write_file"));
    }

    #[test]
    fn world_model_record_transition() {
        let mut model = WorldModel::new(WorldState::new());
        let new_state = WorldState::new().with_variable("step", serde_json::json!(1));
        let t = model.record_transition("a1", "g1", new_state, &[]);
        assert_eq!(model.history().len(), 1);
        assert_eq!(t.action_id, "a1");
        assert_eq!(model.state().variables.get("step"), Some(&serde_json::json!(1)));
    }

    #[test]
    fn world_model_apply_postconditions() {
        let mut model = WorldModel::new(WorldState::new());
        model.apply_postconditions(&[
            Postcondition::FileCreated { path: "output.txt".into() },
            Postcondition::VariableSet { key: "done".into() },
        ]);
        assert!(model.state().file_exists("output.txt"));
        assert!(model.state().variables.contains_key("done"));
    }

    #[test]
    fn world_model_setters() {
        let mut model = WorldModel::new(WorldState::new());
        model.set_working_directory("/tmp");
        assert_eq!(model.state().working_directory.as_deref(), Some("/tmp"));
        model.set_variable("x", serde_json::json!(42));
        assert_eq!(model.state().variables.get("x"), Some(&serde_json::json!(42)));
    }

    // ── Cognitive Trace ─────────────────────────────────────────

    #[test]
    fn trace_node_basic() {
        let node = TraceNode::new(TraceNodeKind::Planning, serde_json::json!("input"), serde_json::json!("output"));
        assert_eq!(node.kind, TraceNodeKind::Planning);
        assert!(node.parent_id.is_none());
    }

    #[test]
    fn trace_node_with_parent_and_confidence() {
        let parent = TraceNode::new(TraceNodeKind::Planning, serde_json::json!({}), serde_json::json!({}));
        let child = TraceNode::new(TraceNodeKind::Evaluation, serde_json::json!({}), serde_json::json!({}))
            .with_parent(&parent.id)
            .with_confidence(0.85);
        assert_eq!(child.parent_id.as_deref(), Some(parent.id.as_str()));
        assert_eq!(child.confidence, Some(0.85));
    }

    #[test]
    fn trace_node_confidence_clamped() {
        let node = TraceNode::new(TraceNodeKind::Execution, serde_json::json!({}), serde_json::json!({}))
            .with_confidence(1.5);
        assert_eq!(node.confidence, Some(1.0));
        let node = TraceNode::new(TraceNodeKind::Execution, serde_json::json!({}), serde_json::json!({}))
            .with_confidence(-0.5);
        assert_eq!(node.confidence, Some(0.0));
    }

    #[test]
    fn cognitive_trace_nodes_of_kind() {
        let mut trace = CognitiveTrace::new("g1");
        trace.push(TraceNode::new(TraceNodeKind::Planning, serde_json::json!({}), serde_json::json!({})));
        trace.push(TraceNode::new(TraceNodeKind::Execution, serde_json::json!({}), serde_json::json!({})));
        trace.push(TraceNode::new(TraceNodeKind::Planning, serde_json::json!({}), serde_json::json!({})));
        assert_eq!(trace.nodes_of_kind(&TraceNodeKind::Planning).len(), 2);
        assert_eq!(trace.nodes_of_kind(&TraceNodeKind::Execution).len(), 1);
    }

    #[test]
    fn cognitive_trace_display() {
        let mut trace = CognitiveTrace::new("g1");
        trace.push(TraceNode::new(TraceNodeKind::Planning, serde_json::json!({}), serde_json::json!({})).with_confidence(0.7));
        trace.push(TraceNode::new(TraceNodeKind::Evaluation, serde_json::json!({}), serde_json::json!({})));
        let display = trace.display();
        assert!(display.contains("Goal: g1"));
        assert!(display.contains("Planning"));
        assert!(display.contains("0.70"));
    }

    // ── GoalEngine Cognitive Trace ──────────────────────────────

    #[tokio::test]
    async fn execute_produces_cognitive_trace() {
        let mut reg = ToolRegistry::new();
        reg.register(Box::new(MockTool::success("cargo", "ok")));
        let mut engine = GoalEngine::new().with_registry(reg);

        let goal = Goal::new("cargo build");
        let action = Action::single("cargo build", Capability::ExecuteCommand, &goal.id);
        let exp = Expectation::success("will succeed");
        let exec_ctx = ExecutionContext::for_goal(&goal.id);
        let report = engine.execute(&goal, &action, &exp, &exec_ctx).await;

        let trace = &report.cognitive_trace;
        assert_eq!(trace.goal_id, goal.id);
        assert!(trace.nodes.len() >= 3, "trace should have execution, verification, reflection nodes");

        let kinds: Vec<_> = trace.nodes.iter().map(|n| n.kind.clone()).collect();
        assert!(kinds.contains(&TraceNodeKind::Execution));
        assert!(kinds.contains(&TraceNodeKind::Verification));
        assert!(kinds.contains(&TraceNodeKind::Reflection));
    }

    #[tokio::test]
    async fn cognitive_trace_records_timing() {
        let mut reg = ToolRegistry::new();
        reg.register(Box::new(MockTool::success("echo", "ok")));
        let mut engine = GoalEngine::new().with_registry(reg);

        let goal = Goal::new("echo test");
        let action = Action::single("echo", Capability::ExecuteCommand, &goal.id);
        let exp = Expectation::success("will work");
        let exec_ctx = ExecutionContext::for_goal(&goal.id);
        let report = engine.execute(&goal, &action, &exp, &exec_ctx).await;

        for node in &report.cognitive_trace.nodes {
            assert!(node.finished_at_ms >= node.started_at_ms);
        }
    }

    // ── Risk Model ──────────────────────────────────────────────

    #[test]
    fn risk_read_is_low() {
        let r = assess_risk(&Capability::ReadFile);
        assert_eq!(r.risk, Risk::Low);
    }

    #[test]
    fn risk_delete_is_high() {
        let r = assess_risk(&Capability::DeleteFile);
        assert_eq!(r.risk, Risk::High);
        assert!(r.risk.requires_approval());
    }

    #[test]
    fn risk_execute_is_medium() {
        let r = assess_risk(&Capability::ExecuteCommand);
        assert_eq!(r.risk, Risk::Medium);
        assert!(!r.risk.requires_approval());
    }

    #[test]
    fn risk_display() {
        assert_eq!(Risk::Low.to_string(), "low");
        assert_eq!(Risk::Critical.to_string(), "critical");
    }

    #[test]
    fn risk_ordering() {
        assert!(Risk::Low < Risk::Medium);
        assert!(Risk::Medium < Risk::High);
        assert!(Risk::High < Risk::Critical);
    }

    // ── Simulation ──────────────────────────────────────────────

    #[test]
    fn simulator_feasible_plan() {
        let sim = DefaultSimulator::new();
        let world = WorldState::new().with_tool(Capability::ExecuteCommand);
        let goal = Goal::new("cargo build");
        let action = Action::single("cargo build", Capability::ExecuteCommand, &goal.id);
        let plan = Plan::from_action(action, "build");

        let result = sim.simulate(&plan, &world);
        assert!(result.feasible);
        assert!(result.confidence > 0.0);
        assert!(result.failed_preconditions.is_empty());
        assert_eq!(result.risks.len(), 1);
        assert_eq!(result.risks[0].risk, Risk::Medium);
    }

    #[test]
    fn simulator_infeasible_plan() {
        let sim = DefaultSimulator::new();
        let world = WorldState::new(); // no tools available
        let goal = Goal::new("read file");
        let action = Action::single("read file", Capability::ReadFile, &goal.id);
        let plan = Plan::from_action(action, "read");

        let result = sim.simulate(&plan, &world);
        assert!(!result.feasible);
        assert!(!result.failed_preconditions.is_empty());
    }

    #[test]
    fn simulator_multi_step() {
        let sim = DefaultSimulator::new();
        let world = WorldState::new()
            .with_tool(Capability::ExecuteCommand)
            .with_tool(Capability::ReadFile);
        let goal = Goal::new("build and verify");
        let a1 = Action::step("cargo build", Capability::ExecuteCommand, &goal.id, 0, 2);
        let a2 = Action::step("read output", Capability::ReadFile, &goal.id, 1, 2);
        let plan = Plan::from_actions(vec![a1, a2], "build then read", PlanSource::Direct);

        let result = sim.simulate(&plan, &world);
        assert!(result.feasible);
        assert_eq!(result.step_results.len(), 2);
        assert!(result.expected_postconditions.len() >= 2);
    }

    #[test]
    fn simulator_aggregate_risk() {
        let sim = DefaultSimulator::new();
        let world = WorldState::new()
            .with_tool(Capability::ExecuteCommand)
            .with_tool(Capability::DeleteFile);
        let goal = Goal::new("dangerous");
        let a1 = Action::step("run", Capability::ExecuteCommand, &goal.id, 0, 2);
        let a2 = Action::step("delete", Capability::DeleteFile, &goal.id, 1, 2);
        let plan = Plan::from_actions(vec![a1, a2], "run then delete", PlanSource::Direct);

        let result = sim.simulate(&plan, &world);
        assert_eq!(result.aggregate_risk, Risk::High);
    }

    // ── Trace Recorder ──────────────────────────────────────────

    #[test]
    fn trace_recorder_assembles_trace() {
        let mut recorder = TraceRecorder::new("g1");
        recorder.record(TraceEvent::PlanningStarted { goal_id: "g1".into() });
        recorder.record(TraceEvent::PlanGenerated { goal_id: "g1".into(), candidate_count: 3 });
        recorder.record(TraceEvent::ExecutionStarted { goal_id: "g1".into(), action_id: "a1".into() });
        recorder.record(TraceEvent::ExecutionFinished { goal_id: "g1".into(), action_id: "a1".into(), success: true });

        let trace = recorder.trace();
        assert_eq!(trace.goal_id, "g1");
        assert!(trace.nodes.len() >= 3);
    }

    #[test]
    fn trace_recorder_event_log() {
        let mut recorder = TraceRecorder::new("g1");
        recorder.record(TraceEvent::PlanningStarted { goal_id: "g1".into() });
        recorder.record(TraceEvent::SimulationFinished { goal_id: "g1".into(), plan_id: "p1".into(), feasible: true, confidence: 0.9 });
        assert_eq!(recorder.events().len(), 2);
    }

    #[test]
    fn trace_recorder_into_trace() {
        let mut recorder = TraceRecorder::new("g1");
        recorder.record(TraceEvent::PlanningStarted { goal_id: "g1".into() });
        let trace = recorder.into_trace();
        assert_eq!(trace.goal_id, "g1");
        assert_eq!(trace.nodes.len(), 1);
    }

    // ── Confidence ──────────────────────────────────────────────

    #[test]
    fn confidence_semantic_constructors() {
        assert_eq!(Confidence::none().value(), 0.0);
        assert_eq!(Confidence::low().value(), 0.25);
        assert_eq!(Confidence::medium().value(), 0.5);
        assert_eq!(Confidence::high().value(), 0.75);
        assert_eq!(Confidence::certain().value(), 1.0);
    }

    #[test]
    fn confidence_from_raw_clamped() {
        assert_eq!(Confidence::from_raw(1.5).value(), 1.0);
        assert_eq!(Confidence::from_raw(-0.5).value(), 0.0);
    }

    #[test]
    fn confidence_from_ratio() {
        let c = Confidence::from_ratio(3, 4);
        assert!((c.value() - 0.75).abs() < 0.001);
        let c = Confidence::from_ratio(0, 0);
        assert_eq!(c.value(), 0.0);
    }

    #[test]
    fn confidence_and_multiply() {
        let a = Confidence::high();   // 0.75
        let b = Confidence::medium(); // 0.5
        let result = a.and(b);
        assert!((result.value() - 0.375).abs() < 0.001);
    }

    #[test]
    fn confidence_or_average() {
        let a = Confidence::high();   // 0.75
        let b = Confidence::low();    // 0.25
        let result = a.or(b);
        assert!((result.value() - 0.5).abs() < 0.001);
    }

    #[test]
    fn confidence_discount() {
        let c = Confidence::high(); // 0.75
        let discounted = c.discount(0.5);
        assert!((discounted.value() - 0.375).abs() < 0.001);
    }

    #[test]
    fn confidence_level_classification() {
        assert_eq!(Confidence::none().level(), ConfidenceLevel::None);
        assert_eq!(Confidence::low().level(), ConfidenceLevel::Low);
        assert_eq!(Confidence::medium().level(), ConfidenceLevel::Medium);
        assert_eq!(Confidence::high().level(), ConfidenceLevel::High);
        assert_eq!(Confidence::certain().level(), ConfidenceLevel::Certain);
    }

    #[test]
    fn confidence_meets_threshold() {
        assert!(Confidence::high().meets(Confidence::medium()));
        assert!(!Confidence::low().meets(Confidence::medium()));
        assert!(Confidence::certain().meets(Confidence::certain()));
    }

    #[test]
    fn confidence_display() {
        assert_eq!(Confidence::medium().to_string(), "50%");
        assert_eq!(Confidence::certain().to_string(), "100%");
    }

    #[test]
    fn confidence_mul_operator() {
        let a = Confidence::high();
        let b = Confidence::high();
        let result = a * b;
        assert!((result.value() - 0.5625).abs() < 0.001);
    }

    #[test]
    fn confidence_into_f32() {
        let c = Confidence::from_raw(0.42);
        let v: f32 = c.into();
        assert!((v - 0.42).abs() < 0.001);
    }

    // ── Confidence Engine ───────────────────────────────────────

    #[test]
    fn confidence_engine_multiply() {
        let engine = ConfidenceEngine::new()
            .add_rule("combined", ConfidenceRule::Multiply {
                sources: vec!["a".into(), "b".into()],
            });

        let mut inputs = HashMap::new();
        inputs.insert("a".into(), Confidence::high());   // 0.75
        inputs.insert("b".into(), Confidence::medium()); // 0.5

        let outputs = engine.evaluate(&inputs);
        let combined = outputs.get("combined").unwrap();
        assert!((combined.value() - 0.375).abs() < 0.001);
    }

    #[test]
    fn confidence_engine_average() {
        let engine = ConfidenceEngine::new()
            .add_rule("avg", ConfidenceRule::Average {
                sources: vec!["a".into(), "b".into()],
            });

        let mut inputs = HashMap::new();
        inputs.insert("a".into(), Confidence::high());   // 0.75
        inputs.insert("b".into(), Confidence::low());    // 0.25

        let outputs = engine.evaluate(&inputs);
        let avg = outputs.get("avg").unwrap();
        assert!((avg.value() - 0.5).abs() < 0.001);
    }

    #[test]
    fn confidence_engine_discount() {
        let engine = ConfidenceEngine::new()
            .add_rule("discounted", ConfidenceRule::Discount {
                source: "a".into(),
                factor: 0.5,
            });

        let mut inputs = HashMap::new();
        inputs.insert("a".into(), Confidence::high()); // 0.75

        let outputs = engine.evaluate(&inputs);
        let discounted = outputs.get("discounted").unwrap();
        assert!((discounted.value() - 0.375).abs() < 0.001);
    }

    #[test]
    fn confidence_engine_cap() {
        let engine = ConfidenceEngine::new()
            .add_rule("capped", ConfidenceRule::Cap {
                source: "a".into(),
                max: Confidence::medium(),
            });

        let mut inputs = HashMap::new();
        inputs.insert("a".into(), Confidence::certain()); // 1.0

        let outputs = engine.evaluate(&inputs);
        let capped = outputs.get("capped").unwrap();
        assert_eq!(capped.value(), 0.5);
    }

    #[test]
    fn confidence_engine_min() {
        let engine = ConfidenceEngine::new()
            .add_rule("min_val", ConfidenceRule::Min {
                sources: vec!["a".into(), "b".into()],
            });

        let mut inputs = HashMap::new();
        inputs.insert("a".into(), Confidence::high()); // 0.75
        inputs.insert("b".into(), Confidence::low());  // 0.25

        let outputs = engine.evaluate(&inputs);
        let min_val = outputs.get("min_val").unwrap();
        assert_eq!(min_val.value(), 0.25);
    }

    #[test]
    fn confidence_engine_missing_input_defaults_to_none() {
        let engine = ConfidenceEngine::new()
            .add_rule("result", ConfidenceRule::Multiply {
                sources: vec!["a".into(), "missing".into()],
            });

        let mut inputs = HashMap::new();
        inputs.insert("a".into(), Confidence::high());

        let outputs = engine.evaluate(&inputs);
        let result = outputs.get("result").unwrap();
        assert_eq!(result.value(), 0.0); // 0.75 * 0.0 = 0.0
    }
}
