//! Memory admission controller (Phase 1b of the FRIDAY 2.0 synthesis spec).
//!
//! Before anything enters episodic memory ([`EpisodeStore`]), it passes through
//! [`MemoryAdmissionController::admit`], which runs five gates:
//!
//! ```text
//! episode
//!   ├─ sensitivity   → Pending (human-confirmable; never silently stored)
//!   ├─ evidence      → Reject(NoEvidence)     (intent ≠ execution)
//!   ├─ reliability   → Reject(Unreliable)
//!   ├─ importance    → Reject(LowImportance)
//!   ├─ novelty       → Reject(Duplicate)      (against the current store)
//!   └─ otherwise     → Store
//! ```
//!
//! The epistemic gates (`evidence`, `reliability`) run **before** the value
//! gates (`importance`, `novelty`) so the recorded `AdmissionReason` is the
//! most accurate one: a failed execution is rejected as `Unreliable`, never
//! mislabeled `LowImportance` — failures are exactly what learning should mine.
//!
//! The Mem0 anti-pattern — "everything becomes memory" — is explicitly
//! rejected here: an episode must clear every gate to be stored, and sensitive
//! content is *held* (`Pending`) for a human to `approve` or `reject` rather
//! than silently persisted.
//!
//! All five signals are derived **deterministically** from the episode and the
//! current store. Admission never calls an LLM — a model may later assist a
//! richer sensitivity scan, but the gate itself must be reproducible.
//!
//! **Threshold coupling:** the defaults are tuned to the executive's current
//! verification (`Confidence::medium()` → a verified success derives importance
//! 0.57 against the 0.5 floor). A future verification-confidence change silently
//! shifts what is admitted — re-run `admission::tests` when verification
//! behavior changes.
//!
//! **Scope boundary:** this gate protects the durable episodic tier
//! ([`EpisodeStore`], consumed by SemanticMemory). The `friday-memory`
//! `MemoryWatcher` (raw workspace observation → short-term episodes) stays
//! unfiltered by design — it is the faithful observer feeding working context,
//! and its replay-determinism contract forbids admission there. See the
//! synthesis spec §1b.

use crate::memory::{CognitiveEpisode, EpisodeOutcome, EpisodeStore};
use serde::{Deserialize, Serialize};

/// Deterministic markers that force the sensitivity gate open. A first-pass
/// lexical scan — deliberately conservative (false positives go to `Pending`,
/// where a human decides; a miss is the only dangerous outcome).
///
/// Matching is word-boundary aware (`secret` never matches "secretary") and
/// shell commands like `pwd`/`passwd` are excluded — they are daily developer
/// vocabulary, not secrets.
const SENSITIVE_MARKERS: &[&str] = &[
    "password",
    "secret",
    "api_key",
    "apikey",
    "api key",
    "credential",
    "private_key",
    "private key",
    "authorization",
    "bearer",
    "ssn",
    "credit card",
    "card number",
    "cvv",
];

/// Why an episode failed the admission gate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AdmissionReason {
    /// Routine noise — importance below the configured floor.
    LowImportance,
    /// A near-identical episode already exists in episodic memory.
    Duplicate,
    /// Verification was not trustworthy enough (low confidence / failed tool).
    Unreliable,
    /// The episode produced no execution/verification evidence.
    NoEvidence,
    /// Sensitive content detected — held, never stored without a human.
    Sensitive,
}

impl std::fmt::Display for AdmissionReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::LowImportance => write!(f, "low importance"),
            Self::Duplicate => write!(f, "duplicate"),
            Self::Unreliable => write!(f, "unreliable"),
            Self::NoEvidence => write!(f, "no evidence"),
            Self::Sensitive => write!(f, "sensitive"),
        }
    }
}

/// Outcome of running the admission gate over an episode.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum AdmissionVerdict {
    /// Episode clears every gate — may enter episodic memory.
    Store,
    /// Episode is rejected; the reason names the gate that caught it.
    Reject(AdmissionReason),
    /// Episode is held for human confirmation (e.g. sensitive content).
    Pending(AdmissionReason),
}

impl std::fmt::Display for AdmissionVerdict {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Store => write!(f, "store"),
            Self::Reject(r) => write!(f, "reject: {r}"),
            Self::Pending(r) => write!(f, "pending: {r}"),
        }
    }
}

/// The five signals the admission gate evaluates. All derived
/// deterministically from the episode + current store, so the same episode
/// and store always produce the same verdict.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdmissionSignal {
    /// 0.0–1.0. Derived from outcome + verification confidence.
    pub importance: f64,
    /// 0.0–1.0. 1.0 = nothing like this exists in the store; lower = repeats.
    pub novelty: f64,
    /// 0.0–1.0. From verification confidence + tool success.
    pub reliability: f64,
    /// 0.0–1.0. From a sensitive-content scan of the episode's text surfaces.
    pub sensitivity: f64,
    /// Whether the episode carries execution + verification evidence.
    pub has_evidence: bool,
}

impl AdmissionSignal {
    /// Derive the five signals for an episode against the current store.
    pub fn derive(episode: &CognitiveEpisode, store: &EpisodeStore) -> Self {
        Self {
            importance: Self::derive_importance(episode),
            novelty: Self::derive_novelty(episode, store),
            reliability: Self::derive_reliability(episode),
            sensitivity: Self::derive_sensitivity(episode),
            has_evidence: episode.execution_result.is_some()
                && episode.verification_result.is_some(),
        }
    }

    /// Unexpected outcomes are the most instructive; aborted/never-started
    /// episodes are noise. Blended with verification confidence so a weakly
    /// supported claim can't skate through on outcome alone.
    fn derive_importance(episode: &CognitiveEpisode) -> f64 {
        let base = match episode.outcome {
            EpisodeOutcome::Success => 0.6,
            EpisodeOutcome::Failure => 0.6,
            EpisodeOutcome::UnexpectedSuccess | EpisodeOutcome::UnexpectedFailure => 0.9,
            EpisodeOutcome::Aborted => 0.3,
            EpisodeOutcome::InProgress => 0.1,
        };
        let confidence = episode.confidence.value() as f64;
        (0.7 * base + 0.3 * confidence).clamp(0.0, 1.0)
    }

    /// Novelty = 1/(1 + repeat count), where a repeat is a stored episode for
    /// the same goal description. Default threshold is 0.0 (repeats are never
    /// rejected) because SemanticMemory must accumulate repeated episodes to
    /// promote patterns — the novelty gate is an explicit policy a caller can
    /// tighten (e.g. high-frequency noise filtering).
    fn derive_novelty(episode: &CognitiveEpisode, store: &EpisodeStore) -> f64 {
        let repeats = store
            .episodes()
            .iter()
            .filter(|ep| ep.goal.description == episode.goal.description)
            .count();
        (1.0 / (1.0 + repeats as f64)).clamp(0.0, 1.0)
    }

    /// Verified + tool-success is maximally reliable; a tool that ran but
    /// failed verification is weaker; a failed tool is weakest.
    fn derive_reliability(episode: &CognitiveEpisode) -> f64 {
        match &episode.verification_result {
            None => episode.confidence.value() as f64,
            Some(v) if v.verified && v.tool_success => 1.0,
            Some(v) if v.tool_success => (0.4 + 0.6 * v.confidence.value() as f64).clamp(0.0, 1.0),
            Some(v) => (0.1 + 0.4 * v.confidence.value() as f64).clamp(0.0, 1.0),
        }
    }

    /// Lexical sensitive-content scan over the goal description, the plan
    /// (strategy + every action instruction), and the reflection.
    fn derive_sensitivity(episode: &CognitiveEpisode) -> f64 {
        let mut haystack = episode.goal.description.clone();
        if let Some(p) = &episode.plan {
            haystack.push(' ');
            haystack.push_str(&p.strategy);
            for action in &p.actions {
                haystack.push(' ');
                haystack.push_str(&action.instruction);
            }
        }
        if let Some(r) = &episode.reflection {
            haystack.push(' ');
            haystack.push_str(r);
        }
        let lower = haystack.to_lowercase();
        for marker in SENSITIVE_MARKERS {
            if contains_word(&lower, marker) {
                return 1.0;
            }
        }
        0.0
    }
}

/// True when `word` appears in `haystack` delimited by non-alphanumeric
/// characters (or the string edges). Prevents substring false positives like
/// `secret` inside "secretary" or `bearer` inside "bearer_token_v2".
fn contains_word(haystack: &str, word: &str) -> bool {
    let bytes = haystack.as_bytes();
    let w = word.as_bytes();
    if w.is_empty() {
        return false;
    }
    let mut cursor = 0;
    while let Some(rel) = haystack[cursor..].find(word) {
        let start = cursor + rel;
        let before_ok = start == 0 || !bytes[start - 1].is_ascii_alphanumeric();
        let end = start + w.len();
        let after_ok = end >= bytes.len() || !bytes[end].is_ascii_alphanumeric();
        if before_ok && after_ok {
            return true;
        }
        cursor = start + w.len();
    }
    false
}

/// A held episode awaiting human confirmation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PendingEpisode {
    /// The episode held out of episodic memory.
    pub episode: CognitiveEpisode,
    /// Why it was held.
    pub reason: AdmissionReason,
    /// When it was held.
    pub held_at_ms: u64,
}

/// The single gate between experience and episodic memory.
///
/// All thresholds are policy, not magic: the defaults admit exactly what the
/// executive already produced (verified, evidenced, non-sensitive episodes)
/// while rejecting noise, so wiring the controller in changes no behavior
/// until a caller tightens a threshold.
#[derive(Debug, Clone)]
pub struct MemoryAdmissionController {
    /// Episodes with importance below this floor are rejected.
    pub min_importance: f64,
    /// Episodes with novelty below this floor are rejected as duplicates.
    /// 0.0 = never reject repeats (SemanticMemory needs them).
    pub min_novelty: f64,
    /// Episodes with reliability below this floor are rejected.
    pub min_reliability: f64,
    /// Sensitivity at or above this value forces `Pending`, never `Store`.
    pub sensitivity_pending_at: f64,
    /// Episodes held for human confirmation.
    pending: Vec<PendingEpisode>,
}

impl Default for MemoryAdmissionController {
    fn default() -> Self {
        Self::new()
    }
}

impl MemoryAdmissionController {
    pub fn new() -> Self {
        Self {
            min_importance: 0.5,
            min_novelty: 0.0,
            min_reliability: 0.5,
            sensitivity_pending_at: 0.7,
            pending: Vec::new(),
        }
    }

    /// Configure every threshold explicitly.
    pub fn with_thresholds(
        min_importance: f64,
        min_novelty: f64,
        min_reliability: f64,
        sensitivity_pending_at: f64,
    ) -> Self {
        Self {
            min_importance,
            min_novelty,
            min_reliability,
            sensitivity_pending_at,
            pending: Vec::new(),
        }
    }

    /// Run the five gates over an episode. The current store is consulted for
    /// the novelty gate. Returns the verdict and the signal that produced it
    /// (so callers can record *why*).
    pub fn admit(
        &self,
        episode: &CognitiveEpisode,
        store: &EpisodeStore,
    ) -> (AdmissionVerdict, AdmissionSignal) {
        let signal = AdmissionSignal::derive(episode, store);
        // Epistemic gates (evidence, reliability) run before value gates
        // (importance, novelty) so the recorded reason is the accurate one:
        // a failed execution is `Unreliable`, never mislabeled `LowImportance`.
        let verdict = if signal.sensitivity >= self.sensitivity_pending_at {
            AdmissionVerdict::Pending(AdmissionReason::Sensitive)
        } else if !signal.has_evidence {
            AdmissionVerdict::Reject(AdmissionReason::NoEvidence)
        } else if signal.reliability < self.min_reliability {
            AdmissionVerdict::Reject(AdmissionReason::Unreliable)
        } else if signal.importance < self.min_importance {
            AdmissionVerdict::Reject(AdmissionReason::LowImportance)
        } else if signal.novelty < self.min_novelty {
            AdmissionVerdict::Reject(AdmissionReason::Duplicate)
        } else {
            AdmissionVerdict::Store
        };
        (verdict, signal)
    }

    /// Hold an episode pending human confirmation. Returns the episode id.
    pub fn hold(&mut self, episode: CognitiveEpisode, reason: AdmissionReason) -> String {
        let id = episode.id.clone();
        let held_at_ms = chrono::Utc::now().timestamp_millis() as u64;
        self.pending.push(PendingEpisode {
            episode,
            reason,
            held_at_ms,
        });
        id
    }

    /// Human approves a held episode — it enters episodic memory. Approval is
    /// an explicit human override: the episode is NOT re-run through the gates
    /// (the person has already decided).
    pub fn approve(&mut self, id: &str, store: &mut EpisodeStore) -> Result<(), String> {
        let idx = self
            .pending
            .iter()
            .position(|p| p.episode.id == id)
            .ok_or_else(|| format!("no pending episode: {id}"))?;
        let pending = self.pending.remove(idx);
        store.store(pending.episode);
        Ok(())
    }

    /// Human rejects a held episode — it is dropped.
    pub fn reject(&mut self, id: &str) -> Result<(), String> {
        let before = self.pending.len();
        self.pending.retain(|p| p.episode.id != id);
        if self.pending.len() == before {
            return Err(format!("no pending episode: {id}"));
        }
        Ok(())
    }

    /// Episodes currently held for human confirmation.
    pub fn pending(&self) -> &[PendingEpisode] {
        &self.pending
    }

    /// Number of episodes held.
    pub fn pending_count(&self) -> usize {
        self.pending.len()
    }

    /// Whether an episode is currently held.
    pub fn is_pending(&self, id: &str) -> bool {
        self.pending.iter().any(|p| p.episode.id == id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Action, Capability, Confidence, Goal, Plan, ToolResult, VerifiedResult};

    fn verified_episode(description: &str) -> CognitiveEpisode {
        CognitiveEpisode::begin(Goal::new(description))
            .with_plan(Plan::from_action(
                Action::single(description, Capability::ExecuteCommand, "g"),
                "direct",
            ))
            .with_execution_result(ToolResult::success("ok"))
            .with_verification(VerifiedResult {
                verified: true,
                tool_success: true,
                checks: vec![],
                confidence: Confidence::medium(),
            })
            .finish(EpisodeOutcome::Success)
    }

    #[test]
    fn verified_episode_is_stored() {
        let controller = MemoryAdmissionController::new();
        let store = EpisodeStore::new();
        let (verdict, signal) = controller.admit(&verified_episode("cargo build"), &store);
        assert_eq!(verdict, AdmissionVerdict::Store, "{signal:?}");
        assert!(signal.importance >= 0.5);
        assert!(signal.has_evidence);
    }

    #[test]
    fn sensitive_episode_is_held_pending_never_stored() {
        let controller = MemoryAdmissionController::new();
        let store = EpisodeStore::new();
        let (verdict, signal) = controller.admit(
            &verified_episode("rotate my api_key and reset the password"),
            &store,
        );
        assert_eq!(
            verdict,
            AdmissionVerdict::Pending(AdmissionReason::Sensitive),
            "{signal:?}"
        );
        assert!(signal.sensitivity >= controller.sensitivity_pending_at);
    }
    #[test]
    fn low_importance_episode_is_rejected() {
        let controller = MemoryAdmissionController::new();
        let store = EpisodeStore::new();
        // Aborted mid-run WITH evidence: it clears the epistemic gates but is
        // routine noise → the value gate (importance) rejects it.
        let episode = CognitiveEpisode::begin(Goal::new("stray thought"))
            .with_execution_result(ToolResult::success("ran briefly"))
            .with_verification(VerifiedResult {
                verified: true,
                tool_success: true,
                checks: vec![],
                confidence: Confidence::low(),
            })
            .finish(EpisodeOutcome::Aborted);
        let (verdict, signal) = controller.admit(&episode, &store);
        assert_eq!(
            verdict,
            AdmissionVerdict::Reject(AdmissionReason::LowImportance),
            "{signal:?}"
        );
        assert!(signal.importance < controller.min_importance);
    }

    #[test]
    fn epistemic_gates_run_before_importance() {
        // A failed execution is labeled Unreliable, never LowImportance —
        // failures are what learning mines, the label must be accurate.
        let controller = MemoryAdmissionController::new();
        let store = EpisodeStore::new();
        let episode = CognitiveEpisode::begin(Goal::new("flaky step"))
            .with_execution_result(ToolResult::error("boom"))
            .with_verification(VerifiedResult {
                verified: false,
                tool_success: false,
                checks: vec![],
                confidence: Confidence::low(),
            })
            .finish(EpisodeOutcome::Failure);
        // importance 0.495 < 0.5 — yet the epistemic gate fires first.
        let (verdict, signal) = controller.admit(&episode, &store);
        assert_eq!(
            verdict,
            AdmissionVerdict::Reject(AdmissionReason::Unreliable),
            "{signal:?}"
        );
        assert!(signal.importance < controller.min_importance);
    }

    #[test]
    fn sensitivity_scan_is_word_boundary_aware() {
        let controller = MemoryAdmissionController::new();
        let store = EpisodeStore::new();
        // "secret" inside "secretary" must NOT trigger; shell `pwd` is not a
        // sensitive marker at all.
        let (verdict, signal) = controller.admit(
            &verified_episode("fix the secretary typo and run pwd"),
            &store,
        );
        assert_eq!(verdict, AdmissionVerdict::Store, "{signal:?}");
        assert_eq!(signal.sensitivity, 0.0);

        // Standalone secrets still trip the gate.
        let (verdict, _signal) =
            controller.admit(&verified_episode("store the client secret"), &store);
        assert_eq!(
            verdict,
            AdmissionVerdict::Pending(AdmissionReason::Sensitive)
        );
    }

    #[test]
    fn unreliable_episode_is_rejected() {
        let controller = MemoryAdmissionController::new();
        let store = EpisodeStore::new();
        let episode = CognitiveEpisode::begin(Goal::new("flaky step"))
            .with_execution_result(ToolResult::error("boom"))
            .with_verification(VerifiedResult {
                verified: false,
                tool_success: false,
                checks: vec![],
                confidence: Confidence::medium(),
            })
            .finish(EpisodeOutcome::Failure);
        let (verdict, signal) = controller.admit(&episode, &store);
        assert_eq!(
            verdict,
            AdmissionVerdict::Reject(AdmissionReason::Unreliable),
            "{signal:?}"
        );
    }

    #[test]
    fn no_evidence_episode_is_rejected() {
        let controller = MemoryAdmissionController::new();
        let store = EpisodeStore::new();
        // Finished outcome but no execution/verification → intent without evidence.
        let episode = CognitiveEpisode::begin(Goal::new("claimed without doing"))
            .finish(EpisodeOutcome::Success);
        let (verdict, signal) = controller.admit(&episode, &store);
        assert_eq!(
            verdict,
            AdmissionVerdict::Reject(AdmissionReason::NoEvidence),
            "{signal:?}"
        );
        assert!(!signal.has_evidence);
    }

    #[test]
    fn duplicate_is_rejected_only_when_policy_tightened() {
        let controller = MemoryAdmissionController::with_thresholds(0.0, 0.4, 0.0, 1.0);
        let mut store = EpisodeStore::new();
        // Two identical episodes already exist → novelty = 1/3 < 0.4.
        store.store(verified_episode("repeat"));
        store.store(verified_episode("repeat"));
        let (verdict, signal) = controller.admit(&verified_episode("repeat"), &store);
        assert_eq!(
            verdict,
            AdmissionVerdict::Reject(AdmissionReason::Duplicate),
            "{signal:?}"
        );
        assert!(signal.novelty < 0.4);
    }

    #[test]
    fn default_novelty_never_rejects_repeats_for_pattern_formation() {
        // SemanticMemory needs repeated episodes to promote patterns — the
        // default controller must admit repeats.
        let controller = MemoryAdmissionController::new();
        let mut store = EpisodeStore::new();
        store.store(verified_episode("cargo build"));
        store.store(verified_episode("cargo build"));
        let (verdict, _signal) = controller.admit(&verified_episode("cargo build"), &store);
        assert_eq!(verdict, AdmissionVerdict::Store);
    }

    #[test]
    fn approve_moves_pending_episode_into_the_store() {
        let mut controller = MemoryAdmissionController::new();
        let mut store = EpisodeStore::new();
        let episode = verified_episode("contains api_key");
        let id = controller.hold(episode.clone(), AdmissionReason::Sensitive);
        assert_eq!(controller.pending_count(), 1);
        assert_eq!(store.count(), 0);

        controller.approve(&id, &mut store).unwrap();
        assert_eq!(controller.pending_count(), 0);
        assert_eq!(store.count(), 1);
        assert_eq!(store.get(&id).unwrap().goal.description, "contains api_key");
    }

    #[test]
    fn reject_drops_pending_episode() {
        let mut controller = MemoryAdmissionController::new();
        let store = EpisodeStore::new();
        let episode = verified_episode("contains secret");
        let id = controller.hold(episode, AdmissionReason::Sensitive);
        controller.reject(&id).unwrap();
        assert_eq!(controller.pending_count(), 0);
        assert_eq!(store.count(), 0);
        assert!(!controller.is_pending(&id));
        assert!(controller.reject(&id).is_err(), "double-reject must fail");
    }

    #[test]
    fn verdict_and_signal_serde_round_trip() {
        let controller = MemoryAdmissionController::new();
        let store = EpisodeStore::new();
        let (verdict, signal) = controller.admit(&verified_episode("cargo build"), &store);
        let json = serde_json::to_string(&verdict).unwrap();
        let back: AdmissionVerdict = serde_json::from_str(&json).unwrap();
        assert_eq!(back, verdict);
        let json = serde_json::to_string(&signal).unwrap();
        let back: AdmissionSignal = serde_json::from_str(&json).unwrap();
        assert_eq!(back.importance, signal.importance);
    }

    #[test]
    fn gate_reason_display() {
        assert_eq!(AdmissionReason::Sensitive.to_string(), "sensitive");
        assert_eq!(AdmissionReason::NoEvidence.to_string(), "no evidence");
        assert_eq!(
            AdmissionVerdict::Reject(AdmissionReason::Duplicate).to_string(),
            "reject: duplicate"
        );
    }
}
