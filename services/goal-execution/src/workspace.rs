use crate::attention::AttentionItem;
use crate::experience::ExperiencePattern;
use crate::memory::{Belief, CognitiveEpisode};
use crate::trace::CognitiveTrace;
use crate::goal::Goal;
use crate::planning::Plan;
use crate::simulation::SimulationResult;
use crate::verification::ToolResult;
use crate::world::WorldState;
use serde::{Deserialize, Serialize};
use std::sync::{Arc, RwLock};
use tokio::sync::broadcast;

// ── Global Workspace (Cognitive Blackboard) ──

/// A change notification — which slot changed and what the new value is.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceChange {
    /// Which slot changed (e.g. "active_goal", "attention", "beliefs").
    pub slot: String,
    /// Human-readable description of the change.
    pub description: String,
    /// When the change occurred.
    pub timestamp_ms: u64,
}

impl WorkspaceChange {
    pub fn new(slot: impl Into<String>, description: impl Into<String>) -> Self {
        Self {
            slot: slot.into(),
            description: description.into(),
            timestamp_ms: chrono::Utc::now().timestamp_millis() as u64,
        }
    }
}

/// The Global Workspace — a cognitive blackboard that every engine
/// reads from and writes to.
///
/// Unlike the event bus in `friday_cognitive` (which tracks history),
/// this represents the **current cognitive state** of the system.
///
/// ```text
/// Perception ──→ Workspace ──→ AttentionManager ──→ WorkingMemory
///                   ↑                                    │
///                   │                                    ▼
///                   │                              Planning
///                   │                                    │
///                   │                              Simulation
///                   │                                    │
///                   │                              Execution
///                   │                                    │
///                   │                              Verification
///                   │                                    │
///                   │                              Reflection
///                   │                                    │
///                   │                              CognitiveEpisode
///                   │                                    │
///                   └───────────────── Semantic Memory ←──┘
/// ```
///
/// Every engine publishes to and consumes from the workspace.
/// Changes broadcast so observers can react without direct coupling.
pub struct CognitiveBlackboard {
    /// The goal currently being pursued.
    active_goal: RwLock<Option<Goal>>,
    /// The plan being executed.
    active_plan: RwLock<Option<Plan>>,
    /// Top attention items (from AttentionManager).
    attention: RwLock<Vec<AttentionItem>>,
    /// Current beliefs in working memory.
    beliefs: RwLock<Vec<Belief>>,
    /// Current snapshot of the world.
    world_state: RwLock<Option<WorldState>>,
    /// Current reasoning chain (thoughts).
    thoughts: RwLock<Vec<String>>,
    /// Active predictions from the simulator.
    predictions: RwLock<Vec<SimulationResult>>,
    /// Patterns being evaluated.
    hypotheses: RwLock<Vec<ExperiencePattern>>,
    /// Last execution result.
    execution: RwLock<Option<ToolResult>>,
    /// Last reflection.
    reflection: RwLock<Option<String>>,
    /// Current in-flight cognitive episode.
    current_episode: RwLock<Option<CognitiveEpisode>>,
    /// Current cognitive trace.
    current_trace: RwLock<Option<CognitiveTrace>>,
    /// Change broadcast channel.
    notifier: Arc<broadcast::Sender<WorkspaceChange>>,
}

impl CognitiveBlackboard {
    /// Create a new workspace.
    pub fn new() -> Arc<Self> {
        let (tx, _) = broadcast::channel(256);
        Arc::new(Self {
            active_goal: RwLock::new(None),
            active_plan: RwLock::new(None),
            attention: RwLock::new(Vec::new()),
            beliefs: RwLock::new(Vec::new()),
            world_state: RwLock::new(None),
            thoughts: RwLock::new(Vec::new()),
            predictions: RwLock::new(Vec::new()),
            hypotheses: RwLock::new(Vec::new()),
            execution: RwLock::new(None),
            reflection: RwLock::new(None),
            current_episode: RwLock::new(None),
            current_trace: RwLock::new(None),
            notifier: Arc::new(tx),
        })
    }

    /// Subscribe to workspace changes.
    pub fn subscribe(&self) -> broadcast::Receiver<WorkspaceChange> {
        self.notifier.subscribe()
    }

    /// Notify subscribers of a change.
    fn notify(&self, slot: impl Into<String>, description: impl Into<String>) {
        let change = WorkspaceChange::new(slot, description);
        let _ = self.notifier.send(change);
    }

    // ── Active Goal ─────────────────────────────────────────────

    pub fn set_active_goal(&self, goal: Goal) {
        if let Ok(mut w) = self.active_goal.write() {
            let desc = format!("goal: {}", goal.description);
            *w = Some(goal);
            drop(w);
            self.notify("active_goal", desc);
        }
    }

    pub fn get_active_goal(&self) -> Option<Goal> {
        self.active_goal.read().ok().and_then(|r| r.clone())
    }

    pub fn clear_active_goal(&self) {
        if let Ok(mut w) = self.active_goal.write() {
            *w = None;
            drop(w);
            self.notify("active_goal", "cleared");
        }
    }

    // ── Active Plan ─────────────────────────────────────────────

    pub fn set_active_plan(&self, plan: Plan) {
        if let Ok(mut w) = self.active_plan.write() {
            let desc = format!("plan: {} ({} actions)", plan.strategy, plan.actions.len());
            *w = Some(plan);
            drop(w);
            self.notify("active_plan", desc);
        }
    }

    pub fn get_active_plan(&self) -> Option<Plan> {
        self.active_plan.read().ok().and_then(|r| r.clone())
    }

    pub fn clear_active_plan(&self) {
        if let Ok(mut w) = self.active_plan.write() {
            *w = None;
            drop(w);
            self.notify("active_plan", "cleared");
        }
    }

    // ── Attention ───────────────────────────────────────────────

    pub fn set_attention(&self, items: Vec<AttentionItem>) {
        if let Ok(mut w) = self.attention.write() {
            let desc = format!("{} items in attention queue", items.len());
            *w = items;
            drop(w);
            self.notify("attention", desc);
        }
    }

    pub fn get_attention(&self) -> Vec<AttentionItem> {
        self.attention.read().ok().map_or(Vec::new(), |r| r.clone())
    }

    // ── Beliefs ─────────────────────────────────────────────────

    pub fn set_beliefs(&self, beliefs: Vec<Belief>) {
        if let Ok(mut w) = self.beliefs.write() {
            let desc = format!("{} beliefs", beliefs.len());
            *w = beliefs;
            drop(w);
            self.notify("beliefs", desc);
        }
    }

    pub fn add_belief(&self, belief: Belief) {
        if let Ok(mut w) = self.beliefs.write() {
            w.push(belief);
            drop(w);
            self.notify("beliefs", "added belief");
        }
    }

    pub fn get_beliefs(&self) -> Vec<Belief> {
        self.beliefs.read().ok().map_or(Vec::new(), |r| r.clone())
    }

    // ── World State ─────────────────────────────────────────────

    pub fn set_world_state(&self, state: WorldState) {
        if let Ok(mut w) = self.world_state.write() {
            let desc = format!("world_state: {} files, {} tools", state.files.len(), state.available_tools.len());
            *w = Some(state);
            drop(w);
            self.notify("world_state", desc);
        }
    }

    pub fn get_world_state(&self) -> Option<WorldState> {
        self.world_state.read().ok().and_then(|r| r.clone())
    }

    // ── Thoughts ────────────────────────────────────────────────

    pub fn push_thought(&self, thought: impl Into<String>) {
        if let Ok(mut w) = self.thoughts.write() {
            w.push(thought.into());
            if w.len() > 50 {
                w.remove(0);
            }
            drop(w);
            self.notify("thoughts", "new thought recorded");
        }
    }

    pub fn get_thoughts(&self) -> Vec<String> {
        self.thoughts.read().ok().map_or(Vec::new(), |r| r.clone())
    }

    pub fn clear_thoughts(&self) {
        if let Ok(mut w) = self.thoughts.write() {
            w.clear();
            drop(w);
            self.notify("thoughts", "cleared");
        }
    }

    // ── Predictions ─────────────────────────────────────────────

    pub fn set_predictions(&self, predictions: Vec<SimulationResult>) {
        if let Ok(mut w) = self.predictions.write() {
            let desc = format!("{} predictions", predictions.len());
            *w = predictions;
            drop(w);
            self.notify("predictions", desc);
        }
    }

    pub fn get_predictions(&self) -> Vec<SimulationResult> {
        self.predictions.read().ok().map_or(Vec::new(), |r| r.clone())
    }

    // ── Hypotheses ──────────────────────────────────────────────

    pub fn set_hypotheses(&self, hypotheses: Vec<ExperiencePattern>) {
        if let Ok(mut w) = self.hypotheses.write() {
            let desc = format!("{} hypotheses", hypotheses.len());
            *w = hypotheses;
            drop(w);
            self.notify("hypotheses", desc);
        }
    }

    pub fn get_hypotheses(&self) -> Vec<ExperiencePattern> {
        self.hypotheses.read().ok().map_or(Vec::new(), |r| r.clone())
    }

    // ── Execution ───────────────────────────────────────────────

    pub fn set_execution(&self, result: ToolResult) {
        if let Ok(mut w) = self.execution.write() {
            let desc = if result.success { "execution succeeded" } else { "execution failed" };
            *w = Some(result);
            drop(w);
            self.notify("execution", desc);
        }
    }

    pub fn get_execution(&self) -> Option<ToolResult> {
        self.execution.read().ok().and_then(|r| r.clone())
    }

    pub fn clear_execution(&self) {
        if let Ok(mut w) = self.execution.write() {
            *w = None;
            drop(w);
            self.notify("execution", "cleared");
        }
    }

    // ── Reflection ──────────────────────────────────────────────

    pub fn set_reflection(&self, reflection: impl Into<String>) {
        if let Ok(mut w) = self.reflection.write() {
            let msg: String = reflection.into();
            *w = Some(msg.clone());
            drop(w);
            self.notify("reflection", msg);
        }
    }

    pub fn get_reflection(&self) -> Option<String> {
        self.reflection.read().ok().and_then(|r| r.clone())
    }

    // ── Current Episode ─────────────────────────────────────────

    pub fn set_current_episode(&self, episode: CognitiveEpisode) {
        if let Ok(mut w) = self.current_episode.write() {
            let desc = format!("episode: {} ({})", episode.goal.description, episode.outcome);
            *w = Some(episode);
            drop(w);
            self.notify("current_episode", desc);
        }
    }

    pub fn get_current_episode(&self) -> Option<CognitiveEpisode> {
        self.current_episode.read().ok().and_then(|r| r.clone())
    }

    pub fn clear_current_episode(&self) {
        if let Ok(mut w) = self.current_episode.write() {
            *w = None;
            drop(w);
            self.notify("current_episode", "cleared");
        }
    }

    // ── Current Trace ───────────────────────────────────────────

    pub fn set_current_trace(&self, trace: CognitiveTrace) {
        if let Ok(mut w) = self.current_trace.write() {
            let desc = format!("trace: {} nodes", trace.nodes.len());
            *w = Some(trace);
            drop(w);
            self.notify("current_trace", desc);
        }
    }

    pub fn get_current_trace(&self) -> Option<CognitiveTrace> {
        self.current_trace.read().ok().and_then(|r| r.clone())
    }

    pub fn clear_current_trace(&self) {
        if let Ok(mut w) = self.current_trace.write() {
            *w = None;
            drop(w);
            self.notify("current_trace", "cleared");
        }
    }

    // ── Snapshot ────────────────────────────────────────────────

    /// Snapshot of the entire current cognitive state.
    pub fn snapshot(&self) -> WorkspaceSnapshot {
        WorkspaceSnapshot {
            active_goal: self.get_active_goal(),
            active_plan: self.get_active_plan(),
            attention_count: self.get_attention().len(),
            belief_count: self.get_beliefs().len(),
            world_state_files: self.get_world_state().map(|ws| ws.files.len()).unwrap_or(0),
            thought_count: self.get_thoughts().len(),
            prediction_count: self.get_predictions().len(),
            hypothesis_count: self.get_hypotheses().len(),
            has_execution: self.get_execution().is_some(),
            has_reflection: self.get_reflection().is_some(),
            has_episode: self.get_current_episode().is_some(),
            trace_node_count: self.get_current_trace().map(|t| t.nodes.len()).unwrap_or(0),
            snapshot_ms: chrono::Utc::now().timestamp_millis() as u64,
        }
    }

    /// Clear all state.
    pub fn clear(&self) {
        self.clear_active_goal();
        self.clear_active_plan();
        self.clear_execution();
        self.clear_current_episode();
        self.clear_current_trace();
        self.clear_thoughts();

        if let Ok(mut w) = self.attention.write() { w.clear(); }
        if let Ok(mut w) = self.beliefs.write() { w.clear(); }
        if let Ok(mut w) = self.world_state.write() { *w = None; }
        if let Ok(mut w) = self.predictions.write() { w.clear(); }
        if let Ok(mut w) = self.hypotheses.write() { w.clear(); }
        if let Ok(mut w) = self.reflection.write() { *w = None; }

        self.notify("workspace", "all state cleared");
    }
}

/// A summary snapshot of the workspace at a point in time.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceSnapshot {
    pub active_goal: Option<Goal>,
    pub active_plan: Option<Plan>,
    pub attention_count: usize,
    pub belief_count: usize,
    pub world_state_files: usize,
    pub thought_count: usize,
    pub prediction_count: usize,
    pub hypothesis_count: usize,
    pub has_execution: bool,
    pub has_reflection: bool,
    pub has_episode: bool,
    pub trace_node_count: usize,
    pub snapshot_ms: u64,
}
