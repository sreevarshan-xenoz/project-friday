use crate::common::Capability;
use crate::confidence::Confidence;
use crate::causal::{CausalModel, CausalSimulation};
use friday_knowledge::KnowledgeGraph;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

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
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
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
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FileMeta {
    pub path: String,
    pub exists: bool,
    pub size_bytes: Option<u64>,
    pub is_directory: bool,
    pub modified_at_ms: Option<u64>,
}

/// Information about a running process.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProcessInfo {
    pub pid: u32,
    pub name: String,
    pub command: String,
}

/// Progress within a mission.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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
        self.files.get(path).map(|m| m.exists).unwrap_or(false)
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
    FileExists {
        path: String,
    },
    FileNotExists {
        path: String,
    },
    ToolAvailable {
        capability: Capability,
    },
    VariableEquals {
        key: String,
        value: serde_json::Value,
    },
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
/// maintains a full history of state changes, and carries the **knowledge
/// graph** as its relationship layer (synthesis Phase 4).
///
/// ```text
/// WorldModel
///   state:     WorldState        (current snapshot)
///   history:   Vec<StateTransition>  (immutable audit trail)
///   knowledge: KnowledgeGraph    (semantic facts: Requires / Creates edges)
///   causal:    CausalModel       (entity graph + cause→effect edges + simulation)
/// ```
///
/// The World Model is the single source of truth for environment state.
/// Actions declare preconditions; the model checks them before execution.
/// After execution, postconditions are applied and a transition is recorded.
/// The seeded knowledge graph lets the planner ask "what must exist before
/// `git.commit` runs?" against the state (M5.6 planner integration); the
/// causal model answers "if `git.commit` runs, what does the state become?"
/// (synthesis Phase 4 — simulation/counterfactuals).
pub struct WorldModel {
    state: WorldState,
    history: Vec<StateTransition>,
    /// Semantic relationship layer (seeded with the M5.6 ontology).
    knowledge: KnowledgeGraph,
    /// Causal layer: grounded entity graph + cause→effect edges + simulation.
    causal: CausalModel,
}

impl WorldModel {
    pub fn new(initial: WorldState) -> Self {
        Self {
            state: initial,
            history: Vec::new(),
            knowledge: KnowledgeGraph::new(),
            causal: CausalModel::new(),
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

    /// The knowledge graph — semantic facts (Requires / Creates edges).
    pub fn knowledge(&self) -> &KnowledgeGraph {
        &self.knowledge
    }

    /// Mutable access to the knowledge graph (assert / remember / improve).
    pub fn knowledge_mut(&mut self) -> &mut KnowledgeGraph {
        &mut self.knowledge
    }

    /// Replace the knowledge graph (custom graphs / tests).
    pub fn with_knowledge(mut self, knowledge: KnowledgeGraph) -> Self {
        self.knowledge = knowledge;
        self
    }

    /// The causal layer — grounded entity graph + causal edges.
    pub fn causal(&self) -> &CausalModel {
        &self.causal
    }

    /// Mutable access to the causal layer (observe entities, teach edges).
    pub fn causal_mut(&mut self) -> &mut CausalModel {
        &mut self.causal
    }

    /// Simulate *"if `action` runs now, what changes?"* — the counterfactual
    /// projection. Pure: the model and current state are untouched; the result
    /// carries the projected world state and entity graph (S').
    pub fn simulate(&self, action: &Capability) -> CausalSimulation {
        self.causal.simulate(action, &self.state)
    }

    /// Commit a simulation's projection as the real state: adopt S' (world
    /// state + entity graph) and record an audit transition. Returns `None`
    /// when the simulation was inapplicable, and refuses a **stale** projection
    /// whose `source_world` no longer matches the current state — adopting a
    /// prediction derived from an older world would silently clobber newer
    /// state. Simulate immediately before applying.
    pub fn apply_simulation(
        &mut self,
        action_id: impl Into<String>,
        goal_id: impl Into<String>,
        sim: &CausalSimulation,
    ) -> Option<StateTransition> {
        if !sim.applicable {
            return None;
        }
        if sim.source_world != self.state {
            return None;
        }
        let before = self.state.clone();
        let transition = StateTransition::new(
            action_id,
            goal_id,
            before,
            sim.projected_world.clone(),
            &[],
        );
        self.state = sim.projected_world.clone();
        self.causal.entities = sim.projected_entities.clone();
        self.history.push(transition.clone());
        Some(transition)
    }

    /// The planner-facing gate (M5.6): what does `capability` require that the
    /// current world state cannot provide? Entities are derived from the state
    /// (known file/dir paths, available tools, network) and checked against the
    /// graph's `Requires` edges. Empty = preconditions satisfied.
    ///
    /// **Id convention:** `capability` must be a **capability-registry dot-id**
    /// (`git.commit`, `web.search`) — the graph's id-space — NOT the
    /// `Capability` enum's snake_case (`execute_command`). An unmodeled
    /// capability returns empty without claiming anything: check
    /// `knowledge().has_preconditions(capability)` to distinguish "satisfied"
    /// from "not modeled".
    ///
    /// ```text
    /// model.find_missing_capability_prerequisites("git.commit")
    ///     → ["GitRepository"]        (no .git anywhere in the state)
    /// model.find_missing_capability_prerequisites("git.commit")
    ///     → []                        (.git known to exist)
    /// ```
    pub fn find_missing_capability_prerequisites(&self, capability: &str) -> Vec<String> {
        let mut active: Vec<String> = Vec::new();
        active.extend(self.state.files.keys().cloned());
        if self.state.network_available {
            active.push("Network".into());
        }
        if self
            .state
            .files
            .keys()
            .any(|p| p == ".git" || p.starts_with(".git/") || p.ends_with("/.git"))
        {
            active.push("GitRepository".into());
        }
        // The causal layer's grounded entity graph is real, observed state:
        // every entity kind present satisfies a prerequisite the file-path
        // heuristic cannot see (e.g. an explicitly modeled GitRepository).
        for entity in &self.causal.entities {
            active.push(entity.kind.clone());
        }
        let refs: Vec<&str> = active.iter().map(|s| s.as_str()).collect();
        self.knowledge.find_missing_preconditions(capability, &refs)
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
                        meta.modified_at_ms = Some(chrono::Utc::now().timestamp_millis() as u64);
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
