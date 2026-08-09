use crate::memory::AttentionScore;
use serde::{Deserialize, Serialize};

// ── Attention Manager ─────────────────────────────────────────────

/// What kind of object is being attended to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum AttentionItemKind {
    Goal,
    Observation,
    Belief,
    Pattern,
    Risk,
    Custom(String),
}

impl std::fmt::Display for AttentionItemKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Goal => write!(f, "goal"),
            Self::Observation => write!(f, "observation"),
            Self::Belief => write!(f, "belief"),
            Self::Pattern => write!(f, "pattern"),
            Self::Risk => write!(f, "risk"),
            Self::Custom(s) => write!(f, "{}", s),
        }
    }
}

/// A scored item competing for attention.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AttentionItem {
    /// Unique identifier for this item.
    pub id: String,
    /// What kind of object this is.
    pub kind: AttentionItemKind,
    /// Human-readable description.
    pub description: String,
    /// The attention score.
    pub score: AttentionScore,
    /// When this item was added to the attention queue.
    pub added_at_ms: u64,
    /// When this item was last re-scored.
    pub last_scored_ms: u64,
    /// How many times this item has been focused on.
    pub focus_count: u32,
    /// Whether this item has been deprioritized (soft-blocked).
    pub deprioritized: bool,
}

impl AttentionItem {
    pub fn new(
        kind: AttentionItemKind,
        description: impl Into<String>,
        score: AttentionScore,
    ) -> Self {
        let now = chrono::Utc::now().timestamp_millis() as u64;
        Self {
            id: format!("att_{}", uuid::Uuid::new_v4()),
            kind,
            description: description.into(),
            score,
            added_at_ms: now,
            last_scored_ms: now,
            focus_count: 0,
            deprioritized: false,
        }
    }

    /// Update the score. Pushes last_scored_ms.
    pub fn rescore(&mut self, score: AttentionScore) {
        self.score = score;
        self.last_scored_ms = chrono::Utc::now().timestamp_millis() as u64;
    }

    /// Record that this item was focused on.
    pub fn record_focus(&mut self) {
        self.focus_count += 1;
    }

    /// Effective score: base aggregate minus deprioritization penalty.
    pub fn effective_score(&self) -> f32 {
        let base = self.score.aggregate();
        if self.deprioritized {
            base * 0.3 // 70% penalty for deprioritized items
        } else {
            base
        }
    }
}

/// Configuration for attention management.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AttentionConfig {
    /// Maximum items in the attention queue.
    pub max_items: usize,
    /// How often to decay scores (in ms). Items lose novelty over time.
    pub decay_interval_ms: u64,
    /// Novelty decay factor per interval [0.0, 1.0]. 0.9 = 10% decay.
    pub novelty_decay: f32,
    /// Minimum score to be considered for focus.
    pub min_focus_score: f32,
}

impl Default for AttentionConfig {
    fn default() -> Self {
        Self {
            max_items: 100,
            decay_interval_ms: 60_000, // 1 minute
            novelty_decay: 0.9,
            min_focus_score: 0.1,
        }
    }
}

/// The Attention Manager — the scheduler for cognition.
///
/// Scores goals, observations, beliefs, and patterns.
/// Maintains a priority queue. Updates scores as the world changes.
/// Chooses the next focus target.
///
/// ```text
/// Items compete for attention
///         │
///         ▼
/// AttentionManager
///         │
///         ├── rescore(item_id, new_score)
///         ├── deprioritize(item_id)
///         ├── decay_novelty()
///         ├── focus() → AttentionItem
///         └── ranked() → Vec<AttentionItem>
/// ```
#[derive(Default)]
pub struct AttentionManager {
    items: Vec<AttentionItem>,
    config: AttentionConfig,
    /// IDs of items that have been focused on recently (for novelty tracking).
    recent_focus_ids: Vec<String>,
}

impl AttentionManager {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_config(config: AttentionConfig) -> Self {
        Self {
            config,
            ..Self::default()
        }
    }

    /// Add an item to the attention queue.
    pub fn add(&mut self, item: AttentionItem) {
        // Prune if at capacity (drop lowest scored unprioritized).
        if self.items.len() >= self.config.max_items {
            if let Some(idx) = self
                .items
                .iter()
                .filter(|i| !i.deprioritized)
                .enumerate()
                .min_by_key(|(_, i)| (i.effective_score() * 1000.0) as i64)
                .map(|(idx, _)| idx)
            {
                self.items.remove(idx);
            }
        }
        self.items.push(item);
    }

    /// Remove an item by ID.
    pub fn remove(&mut self, id: &str) -> bool {
        let before = self.items.len();
        self.items.retain(|i| i.id != id);
        self.items.len() < before
    }

    /// Update the score for an item.
    pub fn rescore(&mut self, id: &str, score: AttentionScore) -> bool {
        if let Some(item) = self.items.iter_mut().find(|i| i.id == id) {
            item.rescore(score);
            true
        } else {
            false
        }
    }

    /// Deprioritize an item (soft block — still visible but heavily penalized).
    pub fn deprioritize(&mut self, id: &str) -> bool {
        if let Some(item) = self.items.iter_mut().find(|i| i.id == id) {
            item.deprioritized = true;
            true
        } else {
            false
        }
    }

    /// Re-prioritize a deprioritized item.
    pub fn reprioritize(&mut self, id: &str) -> bool {
        if let Some(item) = self.items.iter_mut().find(|i| i.id == id) {
            item.deprioritized = false;
            true
        } else {
            false
        }
    }

    /// Decay novelty scores for all items. Call periodically.
    pub fn decay_novelty(&mut self) {
        let now = chrono::Utc::now().timestamp_millis() as u64;
        for item in &mut self.items {
            let elapsed = now.saturating_sub(item.last_scored_ms);
            if elapsed >= self.config.decay_interval_ms {
                let periods = (elapsed / self.config.decay_interval_ms) as f32;
                let decay = self.config.novelty_decay.powf(periods);
                item.score.novelty *= decay;
                item.last_scored_ms = now;
            }
        }
    }

    /// Select the highest-priority item for focus.
    /// Records the focus event and returns the item.
    pub fn focus(&mut self) -> Option<AttentionItem> {
        let idx = self
            .items
            .iter()
            .enumerate()
            .filter(|(_, i)| i.effective_score() >= self.config.min_focus_score)
            .max_by(|a, b| {
                a.1.effective_score()
                    .partial_cmp(&b.1.effective_score())
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .map(|(idx, _)| idx)?;

        let mut item = self.items.remove(idx);
        item.record_focus();
        self.recent_focus_ids.push(item.id.clone());

        // Keep recent focus list bounded.
        if self.recent_focus_ids.len() > 20 {
            self.recent_focus_ids.remove(0);
        }

        Some(item)
    }

    /// Peek at the top item without removing it.
    pub fn peek(&self) -> Option<&AttentionItem> {
        self.items
            .iter()
            .filter(|i| i.effective_score() >= self.config.min_focus_score)
            .max_by(|a, b| {
                a.effective_score()
                    .partial_cmp(&b.effective_score())
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
    }

    /// All items ranked by effective score (descending).
    pub fn ranked(&self) -> Vec<&AttentionItem> {
        let mut items: Vec<_> = self
            .items
            .iter()
            .filter(|i| i.effective_score() >= self.config.min_focus_score)
            .collect();
        items.sort_by(|a, b| {
            b.effective_score()
                .partial_cmp(&a.effective_score())
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        items
    }

    /// Items by kind.
    pub fn by_kind(&self, kind: &AttentionItemKind) -> Vec<&AttentionItem> {
        self.items.iter().filter(|i| i.kind == *kind).collect()
    }

    /// Total items in queue.
    pub fn count(&self) -> usize {
        self.items.len()
    }

    /// Clear all items.
    pub fn clear(&mut self) {
        self.items.clear();
        self.recent_focus_ids.clear();
    }

    /// Check if an item was recently focused on.
    pub fn was_recently_focused(&self, id: &str) -> bool {
        self.recent_focus_ids.contains(&id.to_string())
    }
}
