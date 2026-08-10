//! `HeuristicInvestigationStrategy` — the deterministic default.
//!
//! Keyword-based, confidence-scored, fully testable. It is a strategy, not a
//! model: an LLM strategy can replace it later without touching the plan
//! model or the engine.

use async_trait::async_trait;

use crate::channel::InvestigationChannel;
use crate::plan::{InformationRequirement, InvestigationPlan};
use crate::strategy::InvestigationStrategy;

/// `(keyword, weight)` — longer / more specific phrases weigh more.
type Keyword = (&'static str, f32);

const WEB: &[Keyword] = &[
    ("search the web", 0.9),
    ("search online", 0.9),
    ("the internet", 0.8),
    ("look up", 0.7),
    ("research", 0.7),
    ("find out", 0.6),
    ("explain", 0.5),
    ("latest", 0.5),
    ("online", 0.5),
    ("what is", 0.4),
];

const DOCS: &[Keyword] = &[
    ("documentation", 0.9),
    ("read the docs", 0.9),
    ("api reference", 0.8),
    ("docs", 0.6),
    ("guide", 0.5),
    ("how does", 0.5),
];

const CODEINTEL: &[Keyword] = &[
    ("in this repository", 0.9),
    ("this repo", 0.9),
    ("codebase", 0.8),
    ("project structure", 0.8),
    ("source files", 0.7),
    ("the repository", 0.7),
    ("what files", 0.6),
    ("workspace", 0.4),
];

const MEMORY: &[Keyword] = &[
    ("remember", 0.9),
    ("do you recall", 0.9),
    ("my preference", 0.8),
    ("previously", 0.6),
    ("last time", 0.6),
    ("what did i", 0.6),
    ("earlier", 0.4),
];

const GIT: &[Keyword] = &[
    ("repo status", 0.9),
    ("modified files", 0.8),
    ("git", 0.8),
    ("commit", 0.7),
    ("branches", 0.7),
    ("changed", 0.4),
    ("pull", 0.4),
    ("push", 0.4),
];

const FILESYSTEM: &[Keyword] = &[
    ("list the files", 0.8),
    ("read the file", 0.8),
    ("open the file", 0.7),
    ("directory", 0.6),
    ("folder", 0.6),
    ("file", 0.4),
    ("path", 0.4),
];

/// External reasoning through a logged-in website (Browser Organ): explicit
/// model names and "ask …" phrases so a research goal routes to `webagent.ask`
/// instead of a plain web search. Specific phrases weigh high; a bare
/// "assistant" never fires.
const EXTERNAL: &[Keyword] = &[
    ("ask chatgpt", 0.95),
    ("ask grok", 0.95),
    ("ask the ai", 0.9),
    ("ask an ai", 0.9),
    ("ask a website", 0.85),
    ("chatgpt", 0.8),
    ("grok", 0.8),
    ("logged-in", 0.8),
];

/// The default strategy: keyword tables + confidence threshold.
#[derive(Debug, Clone)]
pub struct HeuristicInvestigationStrategy {
    /// Channels below this confidence are dropped (0.0..=1.0).
    pub min_confidence: f32,
}

impl Default for HeuristicInvestigationStrategy {
    fn default() -> Self {
        Self {
            min_confidence: 0.25,
        }
    }
}

impl HeuristicInvestigationStrategy {
    pub fn new(min_confidence: f32) -> Self {
        Self {
            min_confidence: min_confidence.clamp(0.0, 1.0),
        }
    }
}

#[async_trait]
impl InvestigationStrategy for HeuristicInvestigationStrategy {
    async fn investigate(&self, goal: &str) -> InvestigationPlan {
        // Deterministic core: no awaits, returns immediately when awaited.
        self.compute(goal)
    }
}

impl HeuristicInvestigationStrategy {
    /// The deterministic core (kept sync so the pure logic stays unit-testable
    /// and reusable by any strategy that needs a deterministic baseline).
    fn compute(&self, goal: &str) -> InvestigationPlan {
        let lower = goal.to_lowercase();
        let mut requirements: Vec<InformationRequirement> = Vec::new();

        let tables: &[(InvestigationChannel, &[Keyword])] = &[
            (InvestigationChannel::Web, WEB),
            (InvestigationChannel::Docs, DOCS),
            (InvestigationChannel::CodeIntel, CODEINTEL),
            (InvestigationChannel::Memory, MEMORY),
            (InvestigationChannel::Git, GIT),
            (InvestigationChannel::Filesystem, FILESYSTEM),
            (InvestigationChannel::External, EXTERNAL),
        ];

        for (channel, keywords) in tables {
            let (score, hits) = score_channel(&lower, keywords);
            if score >= self.min_confidence {
                requirements.push(InformationRequirement {
                    channel: *channel,
                    rationale: hits.join(", "),
                    confidence: score,
                });
            }
        }

        // Highest confidence first; ties break on stable channel order so the
        // result is fully deterministic.
        requirements.sort_by(|a, b| {
            b.confidence
                .partial_cmp(&a.confidence)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then(a.channel.ordinal().cmp(&b.channel.ordinal()))
        });

        InvestigationPlan { requirements }
    }
}

/// Sum keyword weights for `goal_lower`, capped at 1.0, returning the hits.
///
/// Short single-word keywords match on word boundaries only, so `file` does
/// not fire on `profile` and `git` does not fire on `digital`. Multi-word
/// phrases keep plain substring matching.
fn score_channel(goal_lower: &str, keywords: &[Keyword]) -> (f32, Vec<&'static str>) {
    let mut total = 0.0f32;
    let mut hits = Vec::new();
    for (kw, w) in keywords {
        let hit = if kw.contains(' ') {
            goal_lower.contains(kw)
        } else {
            contains_word(goal_lower, kw)
        };
        if hit {
            total += *w;
            hits.push(*kw);
        }
    }
    (total.min(1.0), hits)
}

/// Hand-rolled word-boundary search (no regex dependency): `needle` must be
/// delimited by non-alphanumeric characters on both sides.
fn contains_word(hay: &str, needle: &str) -> bool {
    let kw_len = needle.len();
    let mut start = 0;
    while let Some(rel) = hay[start..].find(needle) {
        let i = start + rel;
        let before_ok = i == 0 || !hay[..i].chars().last().unwrap().is_alphanumeric();
        let after = i + kw_len;
        let after_ok =
            after >= hay.len() || !hay[after..].chars().next().unwrap().is_alphanumeric();
        if before_ok && after_ok {
            return true;
        }
        start = i + kw_len;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn web_research_goal_requires_web() {
        let strategy = HeuristicInvestigationStrategy::default();
        let plan = strategy
            .investigate("Search the web for the latest Rust async documentation")
            .await;
        assert!(plan.needs_investigation());
        assert!(plan.channels().contains(&InvestigationChannel::Web));
        let web = plan
            .requirements
            .iter()
            .find(|r| r.channel == InvestigationChannel::Web)
            .expect("web requirement");
        assert!(web.confidence >= 0.9);
        assert!(web.rationale.contains("search the web"));
    }

    #[tokio::test]
    async fn self_contained_goal_needs_no_investigation() {
        let strategy = HeuristicInvestigationStrategy::default();
        let plan = strategy.investigate("Open notepad").await;
        assert!(!plan.needs_investigation());
        assert!(plan.requirements.is_empty());
    }

    #[tokio::test]
    async fn multi_channel_goal_detects_git_and_codeintel() {
        let strategy = HeuristicInvestigationStrategy::default();
        let plan = strategy
            .investigate("Check the git status of the workspace repository and show changed files")
            .await;
        assert!(plan.channels().contains(&InvestigationChannel::Git));
        assert!(plan.channels().contains(&InvestigationChannel::CodeIntel));
    }

    #[tokio::test]
    async fn memory_recall_goal_detected() {
        let strategy = HeuristicInvestigationStrategy::default();
        let plan = strategy
            .investigate("Do you remember what I asked about yesterday?")
            .await;
        assert!(plan.channels().contains(&InvestigationChannel::Memory));
    }

    #[tokio::test]
    async fn external_reasoning_goal_detected() {
        let strategy = HeuristicInvestigationStrategy::default();
        let plan = strategy
            .investigate("Ask ChatGPT what it thinks about this architecture")
            .await;
        assert!(
            plan.channels().contains(&InvestigationChannel::External),
            "{plan:?}"
        );
        // A generic research goal still routes to web, not external.
        let plan = strategy
            .investigate("Search the web for async Rust docs")
            .await;
        assert!(
            !plan.channels().contains(&InvestigationChannel::External),
            "{plan:?}"
        );
    }

    #[tokio::test]
    async fn requirements_ordered_by_confidence() {
        let strategy = HeuristicInvestigationStrategy::default();
        let plan = strategy
            .investigate("Search the web and check the git repo")
            .await;
        // Both web and git likely fire; the list must be strictly non-increasing.
        for pair in plan.requirements.windows(2) {
            assert!(pair[0].confidence >= pair[1].confidence);
        }
    }

    #[tokio::test]
    async fn explain_is_research_not_docs() {
        let strategy = HeuristicInvestigationStrategy::default();
        let plan = strategy.investigate("Explain quantum computing").await;
        assert!(plan.channels().contains(&InvestigationChannel::Web));
    }

    #[tokio::test]
    async fn min_confidence_filters_weak_signals() {
        let strict = HeuristicInvestigationStrategy::new(0.9);
        // "file" alone scores 0.4 — a strict threshold must drop it.
        let plan = strict.investigate("Show me the file").await;
        assert!(!plan.needs_investigation(), "{:?}", plan.requirements);
        // A strong phrase still passes the strict threshold.
        let plan = strict
            .investigate("Search the web for the latest Rust docs")
            .await;
        assert!(plan.channels().contains(&InvestigationChannel::Web));
    }

    #[test]
    fn sync_core_is_unit_testable_without_a_runtime() {
        let h = HeuristicInvestigationStrategy::default();
        assert!(!h.compute("Open notepad").needs_investigation());
        assert!(h
            .compute("Search the web for async Rust")
            .needs_investigation());
    }

    #[tokio::test]
    async fn short_keywords_match_words_not_substrings() {
        let strategy = HeuristicInvestigationStrategy::default();
        // "file" inside "profile" and "git" inside "digital" must not fire.
        let plan = strategy.investigate("Explain my user profile").await;
        assert!(
            !plan.channels().contains(&InvestigationChannel::Filesystem),
            "{:?}",
            plan.requirements
        );
        let plan = strategy.investigate("Explain digital transformation").await;
        assert!(
            !plan.channels().contains(&InvestigationChannel::Git),
            "{:?}",
            plan.requirements
        );
        // Real standalone uses still fire.
        let plan = strategy
            .investigate("Read the file from the git repo")
            .await;
        assert!(plan.channels().contains(&InvestigationChannel::Filesystem));
        assert!(plan.channels().contains(&InvestigationChannel::Git));
    }
}
