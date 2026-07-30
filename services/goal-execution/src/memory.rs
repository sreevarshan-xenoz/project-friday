use crate::confidence::Confidence;
use crate::goal::Goal;
use crate::trace::{CognitiveTrace, TraceNode};
use crate::verification::{ToolResult, VerifiedResult};
use crate::planning::Plan;
use crate::world::WorldState;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// ── Belief ────────────────────────────────────────────────────────

/// The system's current stance on something, including uncertainty and freshness.
///
/// A `KnowledgeRecord` captures what has been learned.
/// A `Belief` captures the system's **current stance** — how confident we are,
/// how fresh it is, and what evidence supports it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Belief {
    /// What the system believes (human-readable).
    pub statement: String,
    /// How confident the system is in this belief.
    pub confidence: Confidence,
    /// Evidence supporting the belief (episode IDs, observation counts, etc.).
    pub evidence: Vec<String>,
    /// What produced this belief (e.g. "reflection", "pattern_detector", "manual").
    pub source: String,
    /// When this belief was last verified against current reality.
    pub last_verified_ms: u64,
    /// When this belief expires and should be re-evaluated.
    /// `None` means it does not expire.
    pub expires_at_ms: Option<u64>,
    /// Confidence threshold below which the belief is considered stale.
    pub confidence_floor: Confidence,
}

impl Belief {
    pub fn new(
        statement: impl Into<String>,
        confidence: Confidence,
        source: impl Into<String>,
    ) -> Self {
        let now = chrono::Utc::now().timestamp_millis() as u64;
        Self {
            statement: statement.into(),
            confidence,
            evidence: Vec::new(),
            source: source.into(),
            last_verified_ms: now,
            expires_at_ms: None,
            confidence_floor: Confidence::low(),
        }
    }

    /// Belief is still fresh if it has not expired.
    pub fn is_fresh(&self, now_ms: u64) -> bool {
        self.expires_at_ms.is_none_or(|exp| now_ms < exp)
    }

    /// Belief is still above confidence floor.
    pub fn is_still_believed(&self) -> bool {
        self.confidence.meets(self.confidence_floor)
    }

    /// Belief is usable: fresh and above confidence floor.
    pub fn is_usable(&self, now_ms: u64) -> bool {
        self.is_fresh(now_ms) && self.is_still_believed()
    }

    /// Update confidence. Pushes verification timestamp.
    pub fn verify(&mut self, confidence: Confidence, now_ms: u64) {
        self.confidence = confidence;
        self.last_verified_ms = now_ms;
    }

    /// Add evidence supporting this belief.
    pub fn with_evidence(mut self, evidence: impl Into<String>) -> Self {
        self.evidence.push(evidence.into());
        self
    }

    /// Set expiry.
    pub fn with_expiry_ms(mut self, expires_at_ms: u64) -> Self {
        self.expires_at_ms = Some(expires_at_ms);
        self
    }

    /// Set confidence floor.
    pub fn with_confidence_floor(mut self, floor: Confidence) -> Self {
        self.confidence_floor = floor;
        self
    }
}

// ── Attention Score ───────────────────────────────────────────────

/// How much attention an object (goal, belief, event, etc.) deserves.
///
/// Every attention score component is [0.0, 1.0].
/// The aggregate is a weighted sum used by the attention system.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AttentionScore {
    /// How important is this to the mission or goal?
    pub importance: f32,
    /// How time-sensitive is this?
    pub urgency: f32,
    /// How new or unexpected is this?
    pub novelty: f32,
    /// What is the risk if this is ignored?
    pub risk: f32,
    /// How confident are we in the underlying information?
    pub confidence: f32,
    /// Mission priority boost [0.0, 1.0].
    pub mission_priority: f32,
}

impl AttentionScore {
    pub fn zero() -> Self {
        Self {
            importance: 0.0,
            urgency: 0.0,
            novelty: 0.0,
            risk: 0.0,
            confidence: 0.0,
            mission_priority: 0.0,
        }
    }

    /// Compute aggregate score. Weights are tunable.
    /// importance:0.3 + urgency:0.25 + novelty:0.15 + risk:0.15 + confidence:0.1 + mission_priority:0.05
    pub fn aggregate(&self) -> f32 {
        (self.importance * 0.30)
            + (self.urgency * 0.25)
            + (self.novelty * 0.15)
            + (self.risk * 0.15)
            + (self.confidence * 0.10)
            + (self.mission_priority * 0.05)
    }

    /// Build a score using the builder pattern.
    pub fn builder() -> AttentionScoreBuilder {
        AttentionScoreBuilder::default()
    }
}

#[derive(Debug, Default)]
pub struct AttentionScoreBuilder {
    importance: f32,
    urgency: f32,
    novelty: f32,
    risk: f32,
    confidence: f32,
    mission_priority: f32,
}

impl AttentionScoreBuilder {
    pub fn importance(mut self, v: f32) -> Self { self.importance = v; self }
    pub fn urgency(mut self, v: f32) -> Self { self.urgency = v; self }
    pub fn novelty(mut self, v: f32) -> Self { self.novelty = v; self }
    pub fn risk(mut self, v: f32) -> Self { self.risk = v; self }
    pub fn confidence(mut self, v: f32) -> Self { self.confidence = v; self }
    pub fn mission_priority(mut self, v: f32) -> Self { self.mission_priority = v; self }

    pub fn build(self) -> AttentionScore {
        AttentionScore {
            importance: self.importance,
            urgency: self.urgency,
            novelty: self.novelty,
            risk: self.risk,
            confidence: self.confidence,
            mission_priority: self.mission_priority,
        }
    }
}

// ── Working Memory ────────────────────────────────────────────────

/// The cognitive scratchpad — what every engine reads first.
///
/// Working memory is volatile state that represents the system's
/// current focus. It is populated by the Attention system and
/// consumed by all downstream engines (Planning, Execution, Reflection).
///
/// ```text
/// Perception → Workspace → Attention → WorkingMemory → Planning
///                                                       ↓
///                                              Execution → Verification
///                                                       ↓
///                                              Reflection → Learning
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkingMemory {
    /// The goal currently being pursued.
    pub active_goal: Option<Goal>,
    /// The plan currently being executed.
    pub active_plan: Option<Plan>,
    /// Current snapshot of the world.
    pub current_world: Option<WorldState>,
    /// Cognitive trace of the current execution cycle.
    pub current_trace: Option<CognitiveTrace>,
    /// Most recent workspace events (bounded ring buffer).
    pub recent_events: Vec<WorkspaceEventRef>,
    /// What the system is currently focused on.
    pub focus: FocusTarget,
    /// Maximum number of recent events to retain.
    pub context_window: usize,
    /// Temporary beliefs held in working memory.
    pub beliefs: Vec<Belief>,
    /// Attention scores for tracked objects.
    pub attention_scores: HashMap<String, AttentionScore>,
}

/// Reference to a workspace event in working memory.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceEventRef {
    pub event_id: String,
    pub topic: String,
    pub engine: String,
    pub timestamp_ms: u64,
}

/// What the system is currently focused on.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum FocusTarget {
    /// Not focused on anything specific.
    Idle,
    /// Pursuing a specific goal.
    Goal(String),
    /// Observing something in the environment.
    Observation(String),
    /// Planning the next action.
    Planning,
    /// Executing an action.
    Executing(String),
    /// Reflecting on an outcome.
    Reflecting(String),
}

impl Default for FocusTarget {
    fn default() -> Self { Self::Idle }
}

impl Default for WorkingMemory {
    fn default() -> Self {
        Self {
            active_goal: None,
            active_plan: None,
            current_world: None,
            current_trace: None,
            recent_events: Vec::new(),
            focus: FocusTarget::Idle,
            context_window: 50,
            beliefs: Vec::new(),
            attention_scores: HashMap::new(),
        }
    }
}

impl WorkingMemory {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_context_window(mut self, window: usize) -> Self {
        self.context_window = window;
        self
    }

    // ── Active state ────────────────────────────────────────────

    pub fn set_focus(&mut self, target: FocusTarget) {
        self.focus = target;
    }

    pub fn set_goal(&mut self, goal: Goal) {
        self.active_goal = Some(goal);
    }

    pub fn set_plan(&mut self, plan: Plan) {
        self.active_plan = Some(plan);
    }

    pub fn set_world(&mut self, world: WorldState) {
        self.current_world = Some(world);
    }

    pub fn set_trace(&mut self, trace: CognitiveTrace) {
        self.current_trace = Some(trace);
    }

    // ── Events ──────────────────────────────────────────────────

    /// Push a new event reference. Drops oldest if at capacity.
    pub fn push_event(&mut self, event: WorkspaceEventRef) {
        if self.recent_events.len() >= self.context_window {
            self.recent_events.remove(0);
        }
        self.recent_events.push(event);
    }

    pub fn recent_events(&self) -> &[WorkspaceEventRef] {
        &self.recent_events
    }

    // ── Beliefs ─────────────────────────────────────────────────

    /// Add a belief. Returns its index.
    pub fn add_belief(&mut self, belief: Belief) -> usize {
        self.beliefs.push(belief);
        self.beliefs.len() - 1
    }

    /// Remove beliefs that are no longer usable.
    pub fn prune_stale_beliefs(&mut self, now_ms: u64) -> Vec<Belief> {
        let mut removed = Vec::new();
        self.beliefs.retain(|b| {
            if !b.is_usable(now_ms) {
                removed.push(b.clone());
                false
            } else {
                true
            }
        });
        removed
    }

    /// Get all beliefs that are currently usable.
    pub fn active_beliefs(&self, now_ms: u64) -> Vec<&Belief> {
        self.beliefs.iter().filter(|b| b.is_usable(now_ms)).collect()
    }

    // ── Attention ───────────────────────────────────────────────

    pub fn set_attention(&mut self, id: impl Into<String>, score: AttentionScore) {
        self.attention_scores.insert(id.into(), score);
    }

    pub fn get_attention(&self, id: &str) -> Option<&AttentionScore> {
        self.attention_scores.get(id)
    }

    /// Get top N most attended items, sorted by aggregate score descending.
    pub fn top_attended(&self, n: usize) -> Vec<(&str, &AttentionScore)> {
        let mut scores: Vec<_> = self.attention_scores.iter()
            .map(|(k, v)| (k.as_str(), v))
            .collect();
        scores.sort_by(|a, b| b.1.aggregate().partial_cmp(&a.1.aggregate()).unwrap_or(std::cmp::Ordering::Equal));
        scores.into_iter().take(n).collect()
    }

    // ── Snapshot ────────────────────────────────────────────────

    /// Clear all state and reset to empty.
    pub fn clear(&mut self) {
        self.active_goal = None;
        self.active_plan = None;
        self.current_world = None;
        self.current_trace = None;
        self.recent_events.clear();
        self.focus = FocusTarget::Idle;
        self.beliefs.clear();
        self.attention_scores.clear();
    }
}

// ── Cognitive Episode ─────────────────────────────────────────────

/// Outcome status of a cognitive episode.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum EpisodeOutcome {
    /// Goal achieved as expected.
    Success,
    /// Goal failed as expected.
    Failure,
    /// Goal achieved unexpectedly (e.g. low confidence, succeeded anyway).
    UnexpectedSuccess,
    /// Goal failed unexpectedly (e.g. high confidence, failed).
    UnexpectedFailure,
    /// Goal was aborted (e.g. user cancellation, timeout).
    Aborted,
    /// Episode is still in progress (not yet finalized).
    InProgress,
}

impl std::fmt::Display for EpisodeOutcome {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Success => write!(f, "success"),
            Self::Failure => write!(f, "failure"),
            Self::UnexpectedSuccess => write!(f, "unexpected_success"),
            Self::UnexpectedFailure => write!(f, "unexpected_failure"),
            Self::Aborted => write!(f, "aborted"),
            Self::InProgress => write!(f, "in_progress"),
        }
    }
}

/// A complete cognitive episode — the full lifecycle of a goal.
///
/// Captures everything from Goal → Planning → Simulation → Execution →
/// Verification → Reflection. This is what Semantic Memory derives patterns from.
///
/// ```text
/// Goal ──→ Planning ──→ Simulation ──→ Execution ──→ Verification ──→ Reflection
///   │          │            │              │               │               │
///   └──────────┴────────────┴──────────────┴───────────────┴───────────────┘
///                              CognitiveEpisode
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CognitiveEpisode {
    /// Unique episode identifier.
    pub id: String,
    /// The goal that initiated this episode.
    pub goal: Goal,
    /// World state before execution began.
    pub world_before: Option<WorldState>,
    /// The plan chosen for execution.
    pub plan: Option<Plan>,
    /// Result of execution (if executed).
    pub execution_result: Option<ToolResult>,
    /// Verification result (if verified).
    pub verification_result: Option<VerifiedResult>,
    /// Reflection insight (if reflected upon).
    pub reflection: Option<String>,
    /// World state after execution completed.
    pub world_after: Option<WorldState>,
    /// Cognitive trace of the entire episode.
    pub trace: CognitiveTrace,
    /// Overall confidence in this episode's outcome.
    pub confidence: Confidence,
    /// Outcome classification.
    pub outcome: EpisodeOutcome,
    /// When the episode began (goal created).
    pub started_ms: u64,
    /// When the episode finished (reflection complete).
    /// `None` if still in progress.
    pub finished_ms: Option<u64>,
    /// Episode-specific tags for indexing.
    pub tags: Vec<String>,
}

impl CognitiveEpisode {
    /// Start a new episode from a goal.
    pub fn begin(goal: Goal) -> Self {
        let now = chrono::Utc::now().timestamp_millis() as u64;
        let trace = CognitiveTrace::new(&goal.id);
        Self {
            id: format!("cog_ep_{}", uuid::Uuid::new_v4()),
            goal,
            world_before: None,
            plan: None,
            execution_result: None,
            verification_result: None,
            reflection: None,
            world_after: None,
            trace,
            confidence: Confidence::medium(),
            outcome: EpisodeOutcome::InProgress,
            started_ms: now,
            finished_ms: None,
            tags: Vec::new(),
        }
    }

    /// Record the world state at the start.
    pub fn with_world_before(mut self, world: WorldState) -> Self {
        self.world_before = Some(world);
        self
    }

    /// Record the plan that was chosen.
    pub fn with_plan(mut self, plan: Plan) -> Self {
        self.plan = Some(plan);
        self
    }

    /// Record execution result.
    pub fn with_execution_result(mut self, result: ToolResult) -> Self {
        self.execution_result = Some(result);
        self
    }

    /// Record verification result. Computes outcome from confidence vs verified.
    pub fn with_verification(mut self, result: VerifiedResult) -> Self {
        self.confidence = result.confidence;
        self.verification_result = Some(result);
        self
    }

    /// Record reflection insight.
    pub fn with_reflection(mut self, insight: impl Into<String>) -> Self {
        self.reflection = Some(insight.into());
        self
    }

    /// Record the world state after execution.
    pub fn with_world_after(mut self, world: WorldState) -> Self {
        self.world_after = Some(world);
        self
    }

    /// Add a tag.
    pub fn with_tag(mut self, tag: impl Into<String>) -> Self {
        self.tags.push(tag.into());
        self
    }

    /// Finish the episode with an outcome.
    pub fn finish(mut self, outcome: EpisodeOutcome) -> Self {
        self.outcome = outcome;
        self.finished_ms = Some(chrono::Utc::now().timestamp_millis() as u64);
        self
    }

    /// Whether this episode is complete.
    pub fn is_complete(&self) -> bool {
        self.outcome != EpisodeOutcome::InProgress
    }

    /// Duration in milliseconds (0 if not finished).
    pub fn duration_ms(&self) -> u64 {
        self.finished_ms.map_or(0, |f| f.saturating_sub(self.started_ms))
    }

    /// Did the episode succeed?
    pub fn succeeded(&self) -> bool {
        matches!(self.outcome, EpisodeOutcome::Success | EpisodeOutcome::UnexpectedSuccess)
    }

    /// Did the episode fail?
    pub fn failed(&self) -> bool {
        matches!(self.outcome, EpisodeOutcome::Failure | EpisodeOutcome::UnexpectedFailure)
    }

    /// Get the cognitive trace.
    pub fn trace(&self) -> &CognitiveTrace {
        &self.trace
    }

    /// Push a trace node onto the episode's trace.
    pub fn push_trace(&mut self, node: TraceNode) {
        self.trace.push(node);
    }
}

// ── Episode Store ─────────────────────────────────────────────────

/// Durable episodic memory with indexing.
///
/// Episodes are stored in-memory and can be queried by goal, outcome,
/// time range, or tag. This is the source of truth for Semantic Memory.
pub struct EpisodeStore {
    episodes: Vec<CognitiveEpisode>,
    max_episodes: usize,
}

impl EpisodeStore {
    pub fn new() -> Self {
        Self {
            episodes: Vec::new(),
            max_episodes: 10_000,
        }
    }

    pub fn with_capacity(max_episodes: usize) -> Self {
        Self {
            episodes: Vec::new(),
            max_episodes,
        }
    }

    /// Store an episode. Prunes oldest if over capacity.
    pub fn store(&mut self, episode: CognitiveEpisode) {
        self.episodes.push(episode);
        if self.episodes.len() > self.max_episodes {
            let drain = self.episodes.len() - self.max_episodes;
            self.episodes.drain(..drain);
        }
    }

    /// Total episodes stored.
    pub fn count(&self) -> usize {
        self.episodes.len()
    }

    /// All episodes.
    pub fn episodes(&self) -> &[CognitiveEpisode] {
        &self.episodes
    }

    /// Get episode by ID.
    pub fn get(&self, id: &str) -> Option<&CognitiveEpisode> {
        self.episodes.iter().find(|ep| ep.id == id)
    }

    /// Episodes for a specific goal.
    pub fn by_goal(&self, goal_id: &str) -> Vec<&CognitiveEpisode> {
        self.episodes.iter().filter(|ep| ep.goal.id == goal_id).collect()
    }

    /// Episodes with a specific outcome.
    pub fn by_outcome(&self, outcome: &EpisodeOutcome) -> Vec<&CognitiveEpisode> {
        self.episodes.iter().filter(|ep| ep.outcome == *outcome).collect()
    }

    /// Episodes containing a specific tag.
    pub fn by_tag(&self, tag: &str) -> Vec<&CognitiveEpisode> {
        self.episodes.iter().filter(|ep| ep.tags.contains(&tag.to_string())).collect()
    }

    /// Episodes within a time range (inclusive).
    pub fn by_time_range(&self, from_ms: u64, to_ms: u64) -> Vec<&CognitiveEpisode> {
        self.episodes.iter()
            .filter(|ep| ep.started_ms >= from_ms && ep.started_ms <= to_ms)
            .collect()
    }

    /// Only successful episodes.
    pub fn successes(&self) -> Vec<&CognitiveEpisode> {
        self.by_outcome(&EpisodeOutcome::Success)
    }

    /// Only failed episodes.
    pub fn failures(&self) -> Vec<&CognitiveEpisode> {
        self.by_outcome(&EpisodeOutcome::Failure)
    }

    /// Success rate as a ratio.
    pub fn success_rate(&self) -> f32 {
        if self.episodes.is_empty() {
            return 0.0;
        }
        let successes = self.episodes.iter().filter(|ep| ep.succeeded()).count();
        successes as f32 / self.episodes.len() as f32
    }

    /// Average duration of completed episodes.
    pub fn avg_duration_ms(&self) -> f64 {
        let completed: Vec<_> = self.episodes.iter()
            .filter(|ep| ep.is_complete())
            .collect();
        if completed.is_empty() {
            return 0.0;
        }
        let total: u64 = completed.iter().map(|ep| ep.duration_ms()).sum();
        total as f64 / completed.len() as f64
    }

    /// Most recent N episodes.
    pub fn recent(&self, n: usize) -> Vec<&CognitiveEpisode> {
        let start = self.episodes.len().saturating_sub(n);
        self.episodes[start..].iter().collect()
    }

    /// Clear all episodes.
    pub fn clear(&mut self) {
        self.episodes.clear();
    }
}

impl Default for EpisodeStore {
    fn default() -> Self {
        Self::new()
    }
}
