use crate::common::Capability;
use crate::memory::{CognitiveEpisode, EpisodeOutcome, EpisodeStore};
use crate::policy::Policy;
use crate::policy::PolicyKind;
use serde::{Deserialize, Serialize};

// ── Experience Pattern ────────────────────────────────────────────

/// What kind of pattern was detected.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum PatternKind {
    /// Repeated success for a capability/plan combination.
    RepeatedSuccess,
    /// Repeated failure for a capability/plan combination.
    RepeatedFailure,
    /// A workflow: action A usually follows action B.
    Workflow,
    /// A preference: tool X is preferred for capability Y.
    Preference,
    /// A risk observation: capability X is consistently risky.
    RiskObservation,
}

impl std::fmt::Display for PatternKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::RepeatedSuccess => write!(f, "repeated_success"),
            Self::RepeatedFailure => write!(f, "repeated_failure"),
            Self::Workflow => write!(f, "workflow"),
            Self::Preference => write!(f, "preference"),
            Self::RiskObservation => write!(f, "risk_observation"),
        }
    }
}

/// An intermediate representation: detected from episodes, not yet stable knowledge.
///
/// Patterns accumulate evidence. Only when support and confidence cross
/// thresholds do they become stable `KnowledgeRecord`s or `Policy` proposals.
///
/// ```text
/// Episodes ──→ Pattern Detection ──→ ExperiencePattern ──→ Threshold Check
///                                                              │
///                                          ┌───────────────────┘
///                                          ▼
///                                    KnowledgeRecord / Policy
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExperiencePattern {
    /// Unique pattern identifier.
    pub id: String,
    /// What kind of pattern this is.
    pub kind: PatternKind,
    /// Human-readable description of the pattern.
    pub description: String,
    /// Number of episodes supporting this pattern.
    pub support: u32,
    /// Confidence in the pattern [0.0, 1.0].
    pub confidence: f32,
    /// IDs of episodes that support this pattern.
    pub episode_ids: Vec<String>,
    /// The capability this pattern relates to (if applicable).
    pub capability: Option<Capability>,
    /// The recorded `from → then` transition of a Workflow pattern (S7). The
    /// Sequence policy promoted from a workflow is only well-formed when it
    /// carries the real capabilities — the planner consumes `from` as the
    /// policy's capability and `then` as `config["then"]`.
    #[serde(default)]
    pub sequence: Option<(Capability, Capability)>,
    /// When this pattern was first detected.
    pub first_seen_ms: u64,
    /// When this pattern was last reinforced.
    pub last_seen_ms: u64,
    /// Whether this pattern has been promoted to a KnowledgeRecord or Policy.
    pub promoted: bool,
}

impl ExperiencePattern {
    pub fn new(kind: PatternKind, description: impl Into<String>) -> Self {
        let now = chrono::Utc::now().timestamp_millis() as u64;
        Self {
            id: format!("pat_{}", uuid::Uuid::new_v4()),
            kind,
            description: description.into(),
            support: 0,
            confidence: 0.0,
            episode_ids: Vec::new(),
            capability: None,
            sequence: None,
            first_seen_ms: now,
            last_seen_ms: now,
            promoted: false,
        }
    }

    /// Add an episode as evidence. Recomputes confidence.
    pub fn add_evidence(&mut self, episode_id: impl Into<String>) {
        self.episode_ids.push(episode_id.into());
        self.support = self.episode_ids.len() as u32;
        self.last_seen_ms = chrono::Utc::now().timestamp_millis() as u64;
        // Confidence grows with support, approaching 1.0 asymptotically.
        // formula: 1 - (1 / (1 + support))
        self.confidence = 1.0 - (1.0 / (1.0 + self.support as f32));
    }

    /// Pattern meets minimum support and confidence thresholds.
    pub fn meets_threshold(&self, min_support: u32, min_confidence: f32) -> bool {
        self.support >= min_support && self.confidence >= min_confidence
    }

    /// Set the capability this pattern relates to.
    pub fn with_capability(mut self, cap: Capability) -> Self {
        self.capability = Some(cap);
        self
    }

    /// Mark as promoted (no longer just a pattern — now knowledge or policy).
    pub fn promote(&mut self) {
        self.promoted = true;
    }
}

// ── Semantic Memory ───────────────────────────────────────────────

/// Configuration for when patterns become knowledge.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromotionThresholds {
    /// Minimum episodes before a pattern is promoted.
    pub min_support: u32,
    /// Minimum confidence before a pattern is promoted.
    pub min_confidence: f32,
}

impl Default for PromotionThresholds {
    fn default() -> Self {
        Self {
            min_support: 3,
            min_confidence: 0.6,
        }
    }
}

/// The result of a semantic memory processing cycle.
#[derive(Debug, Clone, Default)]
pub struct ProcessingResult {
    /// New patterns detected in this cycle.
    pub patterns_detected: usize,
    /// Patterns promoted to knowledge/policy.
    pub patterns_promoted: usize,
    /// New policies proposed.
    pub policies_proposed: usize,
    /// Total episodes processed.
    pub episodes_processed: usize,
}

/// Semantic Memory — derives knowledge from experience.
///
/// Consumes completed `CognitiveEpisode`s from `EpisodeStore`,
/// detects patterns, synthesizes knowledge, and proposes policies.
///
/// ```text
/// EpisodeStore ──→ SemanticMemory ──→ Patterns ──→ Knowledge/Policies
/// ```
///
/// Key invariant: Semantic Memory **never writes knowledge directly**.
/// It only produces evidence-backed patterns that meet promotion thresholds.
pub struct SemanticMemory {
    /// Active patterns being tracked.
    patterns: Vec<ExperiencePattern>,
    /// Promoted knowledge records (derived from patterns).
    knowledge: Vec<Policy>,
    /// Thresholds for promotion.
    thresholds: PromotionThresholds,
    /// Maximum patterns to track (prunes oldest unpromoted).
    max_patterns: usize,
    /// IDs of episodes already processed (dedup).
    processed_episode_ids: Vec<String>,
}

impl SemanticMemory {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_thresholds(thresholds: PromotionThresholds) -> Self {
        Self {
            thresholds,
            ..Self::default()
        }
    }

    /// Process all completed, unprocessed episodes from the store.
    pub fn process_episodes(&mut self, store: &EpisodeStore) -> ProcessingResult {
        let mut result = ProcessingResult::default();

        for episode in store.episodes() {
            if !episode.is_complete() {
                continue;
            }
            if self.processed_episode_ids.contains(&episode.id) {
                continue;
            }

            self.process_episode(episode);
            result.episodes_processed += 1;
        }

        // Check which patterns meet promotion thresholds.
        // Collect indices first to avoid borrow issues.
        let to_promote: Vec<usize> = self
            .patterns
            .iter()
            .enumerate()
            .filter(|(_, p)| {
                !p.promoted
                    && p.meets_threshold(
                        self.thresholds.min_support,
                        self.thresholds.min_confidence,
                    )
            })
            .map(|(i, _)| i)
            .collect();

        for idx in to_promote {
            let pattern = &self.patterns[idx];
            let policy = self.pattern_to_policy(pattern);
            self.patterns[idx].promote();
            result.patterns_promoted += 1;
            if let Some(policy) = policy {
                self.knowledge.push(policy);
                result.policies_proposed += 1;
            }
        }

        result.patterns_detected = self.patterns.iter().filter(|p| !p.promoted).count();
        result
    }

    /// Process a single episode: find or create matching patterns.
    fn process_episode(&mut self, episode: &CognitiveEpisode) {
        self.processed_episode_ids.push(episode.id.clone());

        // 1. Outcome-based patterns
        let kind = match &episode.outcome {
            EpisodeOutcome::Success | EpisodeOutcome::UnexpectedSuccess => {
                PatternKind::RepeatedSuccess
            }
            EpisodeOutcome::Failure | EpisodeOutcome::UnexpectedFailure => {
                PatternKind::RepeatedFailure
            }
            _ => return, // InProgress/Aborted don't generate patterns.
        };

        let cap = episode
            .plan
            .as_ref()
            .and_then(|p| p.actions.first().map(|a| a.capability.clone()));

        let desc = format!("{}: {}", kind, episode.goal.description);

        // Find existing matching pattern or create new one.
        let existing = self
            .patterns
            .iter_mut()
            .find(|p| p.kind == kind && p.capability == cap && !p.promoted);

        match existing {
            Some(pattern) => {
                pattern.add_evidence(episode.id.clone());
            }
            None => {
                let mut pattern = ExperiencePattern::new(kind, &desc);
                pattern.capability = cap;
                pattern.add_evidence(episode.id.clone());
                self.patterns.push(pattern);
            }
        }

        // 2. Workflow patterns: if plan has multiple steps, record the
        // `from → then` transition — the exact pair the promoted Sequence
        // policy will carry (S7). Workflows group by their transition, so a
        // "read then write" workflow never merges with a "search then read"
        // one into a meaningless aggregate.
        if let Some(plan) = &episode.plan {
            if plan.actions.len() >= 2 {
                let from = plan.actions[0].capability.clone();
                let then = plan.actions[1].capability.clone();
                let desc = format!(
                    "workflow: {} then {} ({} steps) for {}",
                    from,
                    then,
                    plan.actions.len(),
                    episode.goal.description
                );
                let existing_workflow = self.patterns.iter_mut().find(|p| {
                    p.kind == PatternKind::Workflow
                        && !p.promoted
                        && p.sequence.as_ref() == Some(&(from.clone(), then.clone()))
                });

                match existing_workflow {
                    Some(wf) => {
                        wf.add_evidence(episode.id.clone());
                    }
                    None => {
                        let mut wf = ExperiencePattern::new(PatternKind::Workflow, &desc);
                        wf.sequence = Some((from, then));
                        wf.add_evidence(episode.id.clone());
                        self.patterns.push(wf);
                    }
                }
            }
        }

        // Prune if over capacity (drop oldest unpromoted).
        let unpromoted = self.patterns.iter().filter(|p| !p.promoted).count();
        if unpromoted > self.max_patterns {
            let to_remove = unpromoted - self.max_patterns;
            let mut removed = 0;
            self.patterns.retain(|p| {
                if !p.promoted && removed < to_remove {
                    removed += 1;
                    false
                } else {
                    true
                }
            });
        }
    }

    /// Convert a pattern into a Policy.
    fn pattern_to_policy(&self, pattern: &ExperiencePattern) -> Option<Policy> {
        match &pattern.kind {
            PatternKind::RepeatedSuccess => {
                let cap = pattern.capability.clone()?;
                Some(Policy::prefer_tool(
                    cap.clone(),
                    format!("best_tool_for_{:?}", cap),
                    pattern.confidence,
                    pattern.support,
                ))
            }
            PatternKind::RepeatedFailure => {
                let cap = pattern.capability.clone()?;
                Some(Policy {
                    id: format!("policy_{}", uuid::Uuid::new_v4()),
                    kind: PolicyKind::Retry,
                    statement: format!("Avoid {:?} — repeated failures", cap),
                    capability: Some(cap),
                    config: serde_json::json!({ "action": "avoid" }),
                    confidence: pattern.confidence,
                    support: pattern.support,
                    created_from: pattern.episode_ids.clone(),
                    expires_at: None,
                    created_at_ms: chrono::Utc::now().timestamp_millis() as u64,
                })
            }
            PatternKind::Workflow => {
                // A Sequence policy is only consumable by the planner when it
                // carries the real `from → then` transition: `capability` is
                // the from-capability and `config["then"]` the then-capability.
                // Without the pair, planning would degenerate into the default
                // `execute_command then execute_command` sequence (S7).
                let (from, then) = pattern.sequence.clone()?;
                Some(Policy {
                    id: format!("policy_{}", uuid::Uuid::new_v4()),
                    kind: PolicyKind::Sequence,
                    statement: pattern.description.clone(),
                    capability: Some(from),
                    config: serde_json::json!({ "then": then }),
                    confidence: pattern.confidence,
                    support: pattern.support,
                    created_from: pattern.episode_ids.clone(),
                    expires_at: None,
                    created_at_ms: chrono::Utc::now().timestamp_millis() as u64,
                })
            }
            _ => None,
        }
    }

    // ── Queries ─────────────────────────────────────────────────

    pub fn patterns(&self) -> &[ExperiencePattern] {
        &self.patterns
    }

    pub fn unpromoted_patterns(&self) -> Vec<&ExperiencePattern> {
        self.patterns.iter().filter(|p| !p.promoted).collect()
    }

    pub fn promoted_patterns(&self) -> Vec<&ExperiencePattern> {
        self.patterns.iter().filter(|p| p.promoted).collect()
    }

    pub fn knowledge(&self) -> &[Policy] {
        &self.knowledge
    }

    pub fn pattern_count(&self) -> usize {
        self.patterns.len()
    }

    pub fn knowledge_count(&self) -> usize {
        self.knowledge.len()
    }

    /// Patterns for a specific capability.
    pub fn patterns_for_capability(&self, cap: &Capability) -> Vec<&ExperiencePattern> {
        self.patterns
            .iter()
            .filter(|p| p.capability.as_ref() == Some(cap))
            .collect()
    }
}

impl Default for SemanticMemory {
    fn default() -> Self {
        Self {
            patterns: Vec::new(),
            knowledge: Vec::new(),
            thresholds: PromotionThresholds::default(),
            max_patterns: 1000,
            processed_episode_ids: Vec::new(),
        }
    }
}
