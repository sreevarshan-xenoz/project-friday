//! The evidence pipeline (MasterPlan #6) — the trustworthy research path:
//!
//! ```text
//!            Research Goal
//!                 ↓
//!         ┌───────┴────────┐
//!         ↓                ↓
//!      AnyDoc            Web
//!         ↓                ↓
//!         └───────┬────────┘
//!                 ↓
//!            Evidence[]          ← [`EvidenceSet`] (this module)
//!                 ↓
//!            Verification        ← concrete [`EvidenceVerifier`]s BEHIND the
//!                 ↓                frozen [`Verifier`](crate::Verifier) seam
//!         Verified Evidence
//!                 ↓
//!              Memory            ← [`evidence_episode`] → admission gate
//! ```
//!
//! The two information organs (Web via [`crate::ResolvingWebTool`], Docs via
//! the `docs` adapter) already normalize their output — web into
//! [`crate::WebEvidence`] entries, docs into the FRIDAY [`Document`] contract
//! (`friday_actions::Document`). What was missing is the **connective tissue**:
//! one channel-agnostic [`Evidence`] shape both organs converge on, a blended
//! body per goal ([`EvidenceSet`]), an evidence **verification** behind the
//! same frozen `Verifier` seam the CodeTestVerifier uses, and a hop that
//! carries *verified* evidence into episodic memory.
//!
//! **Guardrails:**
//! - No new trait. Verification reuses the frozen
//!   [`Verifier`](crate::Verifier) / [`Verification`](crate::Verification)
//!   registry exactly like [`crate::CodeTestVerifier`] — these are the
//!   `EvidenceVerifier` slot in the constitution's
//!   `CodeTestVerifier → SonarVerifier → SecurityVerifier → EvidenceVerifier`
//!   progression.
//! - Provider shape never leaks. `Evidence` carries `source` (the winning
//!   provider/parser), never a `DuckDuckGoResponse` / `Document` raw blob.
//! - Memory is gated: [`evidence_episode`] builds a [`CognitiveEpisode`] whose
//!   execution result is the blended evidence and whose verification is the
//!   evidence verification — the existing `MemoryAdmissionController` still
//!   decides what actually enters episodic memory.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::memory::{CognitiveEpisode, EpisodeOutcome};
use crate::verification::{Check, ToolResult, Verification, VerifiedResult, Verifier};
use crate::web_evidence::WebEvidence;
use crate::Goal;

/// The information organ that produced an evidence item. `Web` and `Docs` are
/// the two blended today; the enum is open so a future `Git`/`External` organ
/// converges on the same shape instead of leaking its own.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EvidenceChannel {
    Web,
    Docs,
    Git,
    External,
}

impl EvidenceChannel {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Web => "web",
            Self::Docs => "docs",
            Self::Git => "git",
            Self::External => "external",
        }
    }
}

/// One evidence item — the only shape the cognitive layer blends and verifies.
/// Both organs normalize into this; the winning provider/parser survives only
/// as [`Evidence::source`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Evidence {
    pub channel: EvidenceChannel,
    /// The winning provider / parser / adapter (e.g. `duckduckgo`, `direct`,
    /// `docs:anydoc`). Never a provider-specific response struct.
    pub source: String,
    pub title: String,
    /// URL for web evidence; file path for docs evidence.
    pub url: String,
    pub content: String,
    pub retrieved_at: DateTime<Utc>,
    /// Retrieval confidence (verification later assesses truth).
    pub confidence: f64,
    pub metadata: serde_json::Value,
}

impl From<WebEvidence> for Evidence {
    fn from(w: WebEvidence) -> Self {
        Self {
            channel: EvidenceChannel::Web,
            source: w.source,
            title: w.title,
            url: w.url,
            content: w.content,
            retrieved_at: w.retrieved_at,
            confidence: w.confidence,
            metadata: json!({}),
        }
    }
}

impl Evidence {
    /// Normalize a parsed FRIDAY [`Document`](friday_actions::Document)
    /// (AnyDoc's `docs.parse` output) into the unified evidence shape.
    pub fn from_document(doc: &friday_actions::Document) -> Self {
        let title = doc
            .title
            .clone()
            .unwrap_or_else(|| doc.metadata.filename.clone());
        Self {
            channel: EvidenceChannel::Docs,
            source: format!("docs:{}", doc.metadata.parser),
            title,
            url: doc.source.as_str().to_string(),
            content: doc.markdown.clone(),
            retrieved_at: DateTime::from_timestamp_millis(doc.metadata.parsed_at_ms as i64)
                .unwrap_or_else(Utc::now),
            confidence: 1.0,
            metadata: json!({
                "format": doc.metadata.format,
                "chars": doc.metadata.chars,
                "paragraphs": doc.metadata.paragraphs,
            }),
        }
    }
}

/// The blended research body for one goal: every evidence action's normalized
/// output merged into one `Evidence[]`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct EvidenceSet {
    pub goal: String,
    pub items: Vec<Evidence>,
}

impl EvidenceSet {
    pub fn new(goal: impl Into<String>) -> Self {
        Self {
            goal: goal.into(),
            items: Vec::new(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// Distinct channels present in the body, in first-seen order.
    pub fn channels(&self) -> Vec<&str> {
        let mut seen = Vec::new();
        for item in &self.items {
            let label = item.channel.label();
            if !seen.contains(&label) {
                seen.push(label);
            }
        }
        seen
    }

    /// Build the body from every evidence action's raw tool output
    /// (`extract_evidence` per output, then blend).
    pub fn from_outputs(goal: &str, outputs: &[String]) -> Self {
        let mut incoming = Self::new(goal);
        for out in outputs {
            incoming.items.extend(extract_evidence(out));
        }
        let mut set = Self::new(goal);
        set.blend(incoming);
        set
    }

    /// Merge `other` into this body. Deduplicates exact `(channel, url)`
    /// repeats; when the **same url** arrives from a *different* channel that
    /// is corroboration — both copies get a confidence bump (capped at 1.0).
    /// Returns the number of items newly added.
    pub fn blend(&mut self, other: EvidenceSet) -> usize {
        let mut added = 0;
        for item in other.items {
            if self
                .items
                .iter()
                .any(|e| e.channel == item.channel && e.url == item.url)
            {
                continue; // exact repeat — dedupe
            }
            if let Some(existing) = self
                .items
                .iter_mut()
                .find(|e| e.url == item.url && e.channel != item.channel)
            {
                // Corroboration: two organs observed the same thing. Record
                // the count so later verification can distinguish "observed
                // once" from "corroborated" — the confidence bump is capped
                // at 1.0 but the provenance survives in metadata.
                let n = existing
                    .metadata
                    .get("corroborations")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0);
                existing.metadata["corroborations"] = json!(n + 1);
                existing.confidence = (existing.confidence + 0.1).min(1.0);
                let mut boosted = item;
                boosted.metadata["corroborations"] = json!(1u64);
                boosted.confidence = (boosted.confidence + 0.1).min(1.0);
                self.items.push(boosted);
            } else {
                self.items.push(item);
            }
            added += 1;
        }
        added
    }

    /// Every item meets the confidence floor (empty body never passes).
    pub fn all_confident(&self, floor: f64) -> bool {
        !self.items.is_empty() && self.items.iter().all(|e| e.confidence >= floor)
    }
}

/// Parse a tool output string into unified [`Evidence`] items. Handles the
/// normalized web shape (`{..., "evidence": [WebEvidence...]}`), the
/// pipeline's own serialized `Evidence[]` array, and the docs shapes
/// (`Document` JSON from `docs.parse`, `{path, content}` from `docs.read`).
/// Returns empty for anything unparseable — garbage is never evidence.
pub fn extract_evidence(output: &str) -> Vec<Evidence> {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(output) else {
        return Vec::new();
    };
    if let Some(arr) = value.get("evidence").and_then(|v| v.as_array()) {
        return arr.iter().filter_map(evidence_from_web_json).collect();
    }
    // The pipeline's own serialized `Vec<Evidence>` (round-trip shape).
    if value.is_array() {
        if let Ok(items) = serde_json::from_value::<Vec<Evidence>>(value) {
            return items;
        }
        return Vec::new();
    }
    if value.get("markdown").is_some() {
        if let Ok(doc) = serde_json::from_value::<friday_actions::Document>(value.clone()) {
            return vec![Evidence::from_document(&doc)];
        }
    }
    if let Some(content) = value.get("content").and_then(|v| v.as_str()) {
        // `docs.read` shape: { path, content, chars, truncated }.
        let path = value
            .get("path")
            .and_then(|v| v.as_str())
            .unwrap_or_default();
        let title = path.rsplit('/').next().unwrap_or(path).to_string();
        return vec![Evidence {
            channel: EvidenceChannel::Docs,
            source: "docs:read".to_string(),
            title,
            url: path.to_string(),
            content: content.to_string(),
            retrieved_at: Utc::now(),
            confidence: 1.0,
            metadata: json!({}),
        }];
    }
    Vec::new()
}

fn evidence_from_web_json(v: &serde_json::Value) -> Option<Evidence> {
    let source = v.get("source").and_then(|v| v.as_str()).unwrap_or("");
    let url = v.get("url").and_then(|v| v.as_str()).unwrap_or("");
    if source.is_empty() && url.is_empty() {
        return None;
    }
    let retrieved_at = v
        .get("retrieved_at")
        .and_then(|v| v.as_str())
        .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
        .map(|d| d.with_timezone(&Utc))
        .unwrap_or_else(Utc::now);
    Some(Evidence {
        channel: EvidenceChannel::Web,
        source: source.to_string(),
        title: v
            .get("title")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        url: url.to_string(),
        content: v
            .get("content")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        retrieved_at,
        confidence: v.get("confidence").and_then(|v| v.as_f64()).unwrap_or(1.0),
        metadata: json!({}),
    })
}

// ── Evidence verification (behind the frozen Verifier seam) ─────────

/// Check 1/3: the tool output parses into a non-empty `Evidence[]`. Garbage
/// is never verified as evidence.
pub struct EvidenceWellFormedVerifier;

impl Verifier for EvidenceWellFormedVerifier {
    fn name(&self) -> &str {
        "evidence_well_formed"
    }

    fn verify(&self, _action: &crate::Action, result: &ToolResult) -> Check {
        let count = extract_evidence(&result.output).len();
        Check {
            name: "evidence_well_formed".into(),
            passed: count > 0,
            detail: if count > 0 {
                format!("{count} evidence items parsed from tool output")
            } else {
                "no parseable evidence in tool output".into()
            },
        }
    }
}

/// Check 2/3: every item's retrieval confidence meets the floor. An empty
/// body or any low-confidence item fails — low-confidence retrieval is not a
/// trustworthy basis for a claim.
pub struct EvidenceConfidenceVerifier {
    pub floor: f64,
}

impl Verifier for EvidenceConfidenceVerifier {
    fn name(&self) -> &str {
        "evidence_confidence"
    }

    fn verify(&self, _action: &crate::Action, result: &ToolResult) -> Check {
        let items = extract_evidence(&result.output);
        let low = items
            .iter()
            .filter(|e| e.confidence < self.floor)
            .map(|e| e.confidence)
            .collect::<Vec<_>>();
        Check {
            name: "evidence_confidence".into(),
            passed: !items.is_empty() && low.is_empty(),
            detail: if low.is_empty() {
                if items.is_empty() {
                    "no evidence to assess".into()
                } else {
                    format!("all {} items meet the {:.2} floor", items.len(), self.floor)
                }
            } else {
                format!(
                    "{} items below the {:.2} floor: {low:?}",
                    low.len(),
                    self.floor
                )
            },
        }
    }
}

/// Check 3/3: the body has substance — total content across items meets a
/// minimum (a title-only search dump is observed, not yet verified research).
pub struct EvidenceSubstanceVerifier {
    pub min_chars: usize,
}

impl Verifier for EvidenceSubstanceVerifier {
    fn name(&self) -> &str {
        "evidence_substance"
    }

    fn verify(&self, _action: &crate::Action, result: &ToolResult) -> Check {
        let items = extract_evidence(&result.output);
        let chars: usize = items.iter().map(|e| e.content.chars().count()).sum();
        Check {
            name: "evidence_substance".into(),
            passed: chars >= self.min_chars,
            detail: format!(
                "{chars} content chars across {} items (need ≥ {})",
                items.len(),
                self.min_chars
            ),
        }
    }
}

/// The concrete evidence verifier — mirrors [`crate::CodeTestVerifier`]'s
/// shape: one builder over the three single-purpose checks, exposed as a
/// frozen-seam [`Verification`] registry so the executive can verify an
/// observed evidence tool result exactly like it verifies a command run.
#[derive(Debug, Clone)]
pub struct EvidenceVerifier {
    confidence_floor: f64,
    min_content_chars: usize,
}

impl Default for EvidenceVerifier {
    fn default() -> Self {
        Self::new()
    }
}

impl EvidenceVerifier {
    pub fn new() -> Self {
        Self {
            confidence_floor: 0.5,
            min_content_chars: 100,
        }
    }

    pub fn with_confidence_floor(mut self, floor: f64) -> Self {
        self.confidence_floor = floor;
        self
    }

    pub fn with_min_content_chars(mut self, chars: usize) -> Self {
        self.min_content_chars = chars;
        self
    }

    /// The frozen-seam registry of the three evidence checks.
    pub fn to_verification(&self) -> Verification {
        Verification::new()
            .register(Box::new(EvidenceWellFormedVerifier))
            .register(Box::new(EvidenceConfidenceVerifier {
                floor: self.confidence_floor,
            }))
            .register(Box::new(EvidenceSubstanceVerifier {
                min_chars: self.min_content_chars,
            }))
    }

    /// Verify an observed evidence tool result.
    pub fn verify(&self, action: &crate::Action, result: &ToolResult) -> VerifiedResult {
        self.to_verification().verify(action, result)
    }
}

/// The verified research body published by the executive for an evidence-gated
/// plan: the blended `Evidence[]` plus the `VerifiedResult` that gates it.
#[derive(Debug, Clone)]
pub struct EvidenceBody {
    pub set: EvidenceSet,
    pub verified: VerifiedResult,
}

/// The "→ Memory" hop: build the consolidatable episode for an evidence-gated
/// goal. Its execution result is the blended evidence (serialized), its
/// verification is the evidence verification, and it is tagged `research` —
/// so the admission gate judges the *evidence*, not a placeholder. Only
/// verified evidence finishes `Success`; an unverified body finishes
/// `Failure` (a learning signal, never a claim).
pub fn evidence_episode(
    goal: Goal,
    set: &EvidenceSet,
    verified: &VerifiedResult,
) -> CognitiveEpisode {
    let body = serde_json::to_string(&set.items).unwrap_or_else(|_| "[]".to_string());
    let outcome = if verified.verified && verified.tool_success {
        EpisodeOutcome::Success
    } else {
        EpisodeOutcome::Failure
    };
    CognitiveEpisode::begin(goal)
        .with_execution_result(ToolResult::success(body))
        .with_verification(verified.clone())
        .with_tag("research")
        .finish(outcome)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::confidence::Confidence;
    use crate::{Action, Capability, ExecutionState};

    fn web_output() -> String {
        json!({
            "query": "rust async",
            "evidence": [
                {
                    "source": "duckduckgo",
                    "title": "The Rust Book",
                    "url": "https://doc.rust-lang.org/book/",
                    "content": "Learn async Rust with the official book. This page is long enough to count as substance for the pipeline: ownership, futures, and the tokio runtime all appear here.",
                    "retrieved_at": "2026-08-10T00:00:00Z",
                    "confidence": 1.0,
                },
                {
                    "source": "duckduckgo",
                    "title": "Tokio docs",
                    "url": "https://docs.rs/tokio",
                    "content": "",
                    "retrieved_at": "2026-08-10T00:00:00Z",
                    "confidence": 1.0,
                },
            ],
        })
        .to_string()
    }

    fn document_output() -> String {
        json!({
            "id": "d1",
            "source": "/repo/spec.md",
            "title": "spec",
            "mime_type": "text/markdown",
            "markdown": "# FRIDAY\nA trustworthy research pipeline with real content across the document.",
            "metadata": {
                "filename": "spec.md",
                "size_bytes": 120,
                "format": "md",
                "parser": "passthrough",
                "chars": 70,
                "paragraphs": 2,
                "parsed_at_ms": 1_700_000_000_000u64,
            },
            "assets": [],
        })
        .to_string()
    }

    fn evidence_action() -> Action {
        Action::single(
            "gather evidence: web (web.search)",
            Capability::Custom("web.search".into()),
            "g1",
        )
    }

    // ── Extraction: both organs converge on Evidence[] ─────────────

    #[test]
    fn extract_evidence_parses_web_output() {
        let items = extract_evidence(&web_output());
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].channel, EvidenceChannel::Web);
        assert_eq!(items[0].source, "duckduckgo");
        assert_eq!(items[0].title, "The Rust Book");
        assert_eq!(items[0].url, "https://doc.rust-lang.org/book/");
        assert_eq!(items[0].confidence, 1.0);
        assert_eq!(items[1].channel, EvidenceChannel::Web);
    }

    #[test]
    fn extract_evidence_parses_document_output() {
        let items = extract_evidence(&document_output());
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].channel, EvidenceChannel::Docs);
        assert_eq!(items[0].source, "docs:passthrough");
        assert_eq!(items[0].url, "/repo/spec.md");
        assert_eq!(items[0].title, "spec");
        assert!(items[0].content.contains("trustworthy research pipeline"));
    }

    #[test]
    fn extract_evidence_rejects_garbage() {
        assert!(extract_evidence("search results").is_empty());
        assert!(extract_evidence("not json at all").is_empty());
        assert!(extract_evidence("{}").is_empty());
    }

    // ── Blending: dedupe + corroboration ───────────────────────────

    #[test]
    fn blend_dedupes_exact_repeats() {
        let mut set = EvidenceSet::new("g");
        let a = EvidenceSet {
            goal: "g".into(),
            items: extract_evidence(&web_output()),
        };
        assert_eq!(set.blend(a.clone()), 2);
        assert_eq!(set.blend(a), 0, "exact (channel, url) repeats are dropped");
        assert_eq!(set.len(), 2);
    }

    #[test]
    fn blend_corroborates_same_url_across_channels() {
        let mut set = EvidenceSet::new("g");
        // Web sees the page at 0.6 confidence (headline-only snippet).
        let mut web_items = extract_evidence(&web_output());
        web_items[0].url = "https://doc.rust-lang.org/book/".into();
        web_items[0].confidence = 0.6;
        set.blend(EvidenceSet {
            goal: "g".into(),
            items: web_items,
        });
        let base = set.items[0].confidence;
        // Docs (a local copy of the same page) corroborates it.
        let mut doc = Evidence::from_document(
            &serde_json::from_str::<friday_actions::Document>(&document_output())
                .expect("document output parses"),
        );
        doc.url = "https://doc.rust-lang.org/book/".into(); // same url, docs channel
        let doc_set = EvidenceSet {
            goal: "g".into(),
            items: vec![doc],
        };
        assert_eq!(set.blend(doc_set), 1);
        assert_eq!(set.len(), 3);
        let corroborated = set
            .items
            .iter()
            .find(|e| {
                e.channel == EvidenceChannel::Web && e.url == "https://doc.rust-lang.org/book/"
            })
            .expect("web copy still present");
        assert!(
            corroborated.confidence > base,
            "corroboration bumps confidence: {} → {}",
            base,
            corroborated.confidence
        );
        // The provenance survives: later verification can see the item was
        // corroborated by another organ, not just retrieved once.
        assert_eq!(
            corroborated.metadata["corroborations"],
            json!(1),
            "corroboration count recorded in metadata"
        );
        assert_eq!(set.channels(), vec!["web", "docs"]);
    }

    #[test]
    fn evidence_set_from_outputs_builds_a_blended_body() {
        let set = EvidenceSet::from_outputs("research", &[web_output(), document_output()]);
        assert_eq!(set.len(), 3, "2 web + 1 doc");
        assert_eq!(set.goal, "research");
        assert_eq!(set.channels(), vec!["web", "docs"]);
    }

    // ── Verification: the EvidenceVerifier behind the seam ─────────

    #[test]
    fn evidence_verifier_passes_a_healthy_research_body() {
        let verifier = EvidenceVerifier::new();
        let result = ToolResult::success(web_output());
        let verified = verifier.verify(&evidence_action(), &result);
        assert_eq!(verified.checks.len(), 3, "three inspectable checks");
        assert!(verified.verified, "checks: {:?}", verified.checks);
        assert_eq!(
            ExecutionState::from_verified(&verified),
            ExecutionState::Verified
        );
        assert!(verified.checks.iter().all(|c| c.passed));
    }

    #[test]
    fn evidence_verifier_blocks_garbage() {
        let verifier = EvidenceVerifier::new();
        let result = ToolResult::success("search results");
        let verified = verifier.verify(&evidence_action(), &result);
        assert!(!verified.verified);
        assert!(
            verified
                .checks
                .iter()
                .any(|c| c.name == "evidence_well_formed" && !c.passed),
            "well-formed check names the failure: {:?}",
            verified.checks
        );
        assert_eq!(
            ExecutionState::from_verified(&verified),
            ExecutionState::Failed,
            "unparseable output is Failed, never Verified"
        );
    }

    #[test]
    fn evidence_verifier_blocks_low_confidence_items() {
        let verifier = EvidenceVerifier::new().with_confidence_floor(0.8);
        let mut out: serde_json::Value = serde_json::from_str(&web_output()).unwrap();
        out["evidence"][0]["confidence"] = json!(0.3);
        let result = ToolResult::success(out.to_string());
        let verified = verifier.verify(&evidence_action(), &result);
        assert!(!verified.verified);
        let conf = verified
            .checks
            .iter()
            .find(|c| c.name == "evidence_confidence")
            .expect("confidence check present");
        assert!(!conf.passed);
        assert!(
            conf.detail.contains("0.3"),
            "detail names the offender: {}",
            conf.detail
        );
    }

    #[test]
    fn evidence_verifier_blocks_a_title_only_dump() {
        // Content-less search hits: observed, but not yet verified research.
        let verifier = EvidenceVerifier::new();
        let out = json!({
            "query": "q",
            "evidence": [
                { "source": "s", "title": "T", "url": "https://x.example/a", "content": "", "retrieved_at": "2026-08-10T00:00:00Z", "confidence": 1.0 },
                { "source": "s", "title": "U", "url": "https://x.example/b", "content": "", "retrieved_at": "2026-08-10T00:00:00Z", "confidence": 1.0 },
            ],
        });
        let result = ToolResult::success(out.to_string());
        let verified = verifier.verify(&evidence_action(), &result);
        assert!(!verified.verified);
        assert!(
            verified
                .checks
                .iter()
                .any(|c| c.name == "evidence_substance" && !c.passed),
            "{:?}",
            verified.checks
        );
    }

    #[test]
    fn evidence_verifier_failed_check_is_never_verified() {
        let verifier = EvidenceVerifier::new();
        let result = ToolResult::error("web.search unavailable");
        let verified = verifier.verify(&evidence_action(), &result);
        assert!(!verified.tool_success);
        assert!(!verified.verified);
        assert_eq!(
            ExecutionState::from_verified(&verified),
            ExecutionState::Failed
        );
    }

    // ── Memory hop: verified evidence enters episodic memory ───────

    #[test]
    fn evidence_episode_carries_blended_evidence_and_verification() {
        let set = EvidenceSet::from_outputs("research", &[web_output(), document_output()]);
        let result = ToolResult::success(serde_json::to_string(&set.items).unwrap());
        let verified = EvidenceVerifier::new().verify(&evidence_action(), &result);

        let episode = evidence_episode(Goal::new("research"), &set, &verified);
        assert_eq!(episode.outcome, EpisodeOutcome::Success);
        assert!(episode.tags.contains(&"research".to_string()));
        let exec = episode
            .execution_result
            .expect("evidence is the execution result");
        let parsed: Vec<Evidence> = serde_json::from_str(&exec.output).unwrap();
        assert_eq!(
            parsed.len(),
            3,
            "blended body is the episode's execution result"
        );
        let ver = episode.verification_result.expect("verification attached");
        assert!(ver.verified);
    }

    #[test]
    fn unverified_evidence_episode_finishes_failure() {
        let set = EvidenceSet::new("research");
        let verified = VerifiedResult {
            verified: false,
            tool_success: false,
            checks: vec![Check {
                name: "evidence_well_formed".into(),
                passed: false,
                detail: "garbage".into(),
            }],
            confidence: Confidence::none(),
        };
        let episode = evidence_episode(Goal::new("research"), &set, &verified);
        assert_eq!(episode.outcome, EpisodeOutcome::Failure);
        assert!(!episode.succeeded());
    }
}
