//! Autonomous Cognitive Tick — the self-directed drive (MasterPlan #17).
//!
//! The constitution's gap: *"No self-directed continuous loop … not an
//! unsupervised drive."* And its guardrail: *"Don't create a heartbeat that
//! blindly calls the LLM. That would be a fancy CPU heater, not autonomy."*
//!
//! This module is the decision function, not a CPU heater:
//!
//! ```text
//!         CognitiveRuntime (or standalone run loop)
//!                      │
//!            Observe → Evaluate → Maybe-Act → Sleep → Repeat
//!                      │              │
//!            GlobalWorkspace     ONE action, justified by an
//!            (sensor events)     evidence trace (event ids)
//! ```
//!
//! Design rules:
//!
//! - **No user goal required.** The tick runs without a mission; it acts on
//!   what the workspace shows it. The constitution's acceptance is verbatim:
//!   *"A tick loop runs without a user goal, observes the workspace, and fires
//!   one action justified by an evidence trace."*
//! - **One action per tick, and only when warranted.** [`AutonomousTick::tick`]
//!   returns at most one [`TickAction`], and returns `None` when nothing new
//!   and salient has happened. An empty workspace never fires — silence is
//!   honest, never fabricated.
//! - **The decision function is deterministic and pluggable.**
//!   [`DecisionFunction`] is the frozen seam; [`SalienceDecision`] is the
//!   built-in rule set (salience over evidence, no LLM). A caller can plug a
//!   richer decision function at the same seam later — the loop doesn't care.
//! - **Evidence is the justification.** Every [`TickAction`] carries the
//!   workspace event ids it was decided from (`evidence`), a human-readable
//!   `reason`, and the action is published as `Topic::ActionDecided`
//!   (`autonomy.action`) so whatever consumes decisions can audit *why*.
//! - **Deduplicated, not spammed.** An event already decided on is never
//!   decided on again ([`DecisionState`] tracks the last decided event per
//!   topic); a static scene stays quiet tick after tick.
//! - **Truthful input, truthful output.** A turn without a transcript
//!   (`asr: null`) does not become a "respond to what was said" action; a
//!   screen with `change: "none"` does not become a content change. The
//!   decision function reads the payload, never guesses.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::handler::{PhaseHandler, PhaseResult};
use crate::journal::{ActionJournal, BeginAction};
use crate::phase::CognitivePhase;
use crate::workspace::{EngineId, GlobalWorkspace, Priority, Topic, WorkspaceEvent};

// ── The decision output ───────────────────────────────────────────────

/// ONE action the autonomous tick decided to fire, with its justification.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TickAction {
    /// What to do — a capability or behavior, e.g. `web.search`.
    pub action: String,
    /// Why — the human/planner-readable justification for the action.
    pub reason: String,
    /// The workspace event ids that justify this action (the evidence trace).
    pub evidence: Vec<Uuid>,
    /// Salience of the trigger (priority ordering for the consumer).
    pub priority: Priority,
    /// The world-model revision this action was grounded against (R2-1),
    /// stamped by the decision function while reading the authoritative
    /// model. `None` when no world-model seam is wired. Execution admission
    /// refuses the action when the live revision has moved past it.
    #[serde(default)]
    pub grounding_revision: Option<u64>,
}

impl TickAction {
    pub fn new(
        action: impl Into<String>,
        reason: impl Into<String>,
        evidence: Vec<Uuid>,
        priority: Priority,
    ) -> Self {
        Self {
            action: action.into(),
            reason: reason.into(),
            evidence,
            priority,
            grounding_revision: None,
        }
    }

    /// Stamp the world-model revision this action is grounded against (R2-1).
    pub fn with_grounding_revision(mut self, revision: u64) -> Self {
        self.grounding_revision = Some(revision);
        self
    }
}

/// The observed result of dispatching one autonomous action. This is separate
/// from verification: a successful dispatch is not proof that the desired
/// outcome happened.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionExecution {
    pub action: String,
    pub success: bool,
    pub detail: String,
    /// Structured adapter output retained for verification and recovery.
    pub output: serde_json::Value,
    pub error: Option<String>,
}

/// The auditable result of the execution + verification continuation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionOutcome {
    pub action: TickAction,
    pub execution: ActionExecution,
    pub verified: bool,
}

/// Adapter seam for bounded action dispatch. Implementations own permissions,
/// timeouts, retries, and provider-specific execution; the cognitive crate
/// only supplies the already-decided action and its evidence.
#[async_trait]
pub trait ActionExecutor: Send + Sync {
    async fn execute(&self, action: &TickAction) -> ActionExecution;
}

/// Verification seam for the observed execution result. Verification must be
/// grounded in the workspace or another explicit observation; it must not infer
/// success from dispatch alone.
#[async_trait]
pub trait ActionVerifier: Send + Sync {
    async fn verify(
        &self,
        workspace: &GlobalWorkspace,
        action: &TickAction,
        execution: &ActionExecution,
    ) -> bool;
}

// ── The decision seam ─────────────────────────────────────────────────

/// The frozen decision seam: inspect the workspace and decide whether to fire
/// ONE action. Implementations must be deterministic and must **not** invent
/// evidence — an action is only as good as the event ids it cites.
pub trait DecisionFunction: Send + Sync {
    /// `decide` is called every tick with the workspace and the current
    /// decision state. Return `Some(action)` to fire exactly one action, or
    /// `None` to stay quiet this tick.
    fn decide(&self, workspace: &GlobalWorkspace, state: &mut DecisionState) -> Option<TickAction>;
}

/// Tracks what the drive has already decided on, so a static scene never
/// re-fires. One last-decided event id per topic prefix.
#[derive(Debug, Default)]
pub struct DecisionState {
    /// topic prefix → the id of the newest event already decided on.
    last_decided: HashMap<String, Uuid>,
}

impl DecisionState {
    pub fn new() -> Self {
        Self::default()
    }

    /// The newest event on `prefix` that is NEWER than the last decided event
    /// on that prefix — `None` when nothing new happened. This is the
    /// dedup core: same prefix + same last id = same world, no action.
    fn newest_undecided(
        &mut self,
        workspace: &GlobalWorkspace,
        prefix: &str,
    ) -> Option<WorkspaceEvent> {
        let latest = workspace.get_latest(prefix)?;
        match self.last_decided.get(prefix) {
            Some(decided) if *decided == latest.id => None,
            _ => Some(latest),
        }
    }

    /// Record that `event` has been decided on (for every prefix that matches
    /// its topic), so it can never fire again.
    fn mark_decided(&mut self, event: &WorkspaceEvent) {
        let topic = event.topic.as_str();
        // Prefixes that could match this topic — the longest matching prefix
        // wins, plus any registered prefix it starts with. We only ever store
        // per-prefix here; the decision function chooses the prefixes it
        // watches, so recording under the exact topic is sufficient.
        self.last_decided.insert(topic, event.id);
    }
}

// ── The built-in decision function ────────────────────────────────────

/// Salience of a trigger: drives priority AND which rule wins when several
/// topics fired in the same tick (higher salience fires; the tick still only
/// fires ONE action).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Salience {
    /// A speaker turn with a transcript — the user said something.
    UserSpoke,
    /// Objects detected on camera.
    ObjectsDetected,
    /// The foreground screen content changed.
    ScreenChanged,
    /// The foreground window focus switched.
    FocusChanged,
}

impl Salience {
    pub fn priority(self) -> Priority {
        match self {
            Salience::UserSpoke => Priority::High,
            Salience::ObjectsDetected => Priority::High,
            Salience::ScreenChanged => Priority::Normal,
            Salience::FocusChanged => Priority::Low,
        }
    }

    /// Stable rule ordering for ties in output priority. A user turn takes
    /// precedence over environmental observations when both are new.
    fn rank(self) -> u8 {
        match self {
            Salience::UserSpoke => 4,
            Salience::ObjectsDetected => 3,
            Salience::ScreenChanged => 2,
            Salience::FocusChanged => 1,
        }
    }
}

/// A decision rule: watch a topic prefix, evaluate the payload, and if the
/// evidence is real, propose an action.
#[derive(Clone)]
struct Rule {
    prefix: &'static str,
    salience: Salience,
}

impl Rule {
    const fn new(prefix: &'static str, salience: Salience) -> Self {
        Self { prefix, salience }
    }

    /// Evaluate the event and return the action + reason if the payload is
    /// genuinely salient. **Reads the payload; never guesses.**
    fn evaluate(&self, event: &WorkspaceEvent) -> Option<(String, String)> {
        let payload = &event.payload;
        match self.salience {
            Salience::UserSpoke => {
                // Honest: only fire when a transcript exists. A turn without
                // ASR is just noise — we cannot know what was said.
                let transcript = payload.get("transcript")?.as_str()?;
                if transcript.trim().is_empty() {
                    return None;
                }
                Some((
                    "respond.to_user".to_string(),
                    format!("the user said: \"{transcript}\""),
                ))
            }
            Salience::ObjectsDetected => {
                let count = payload.get("count").and_then(|v| v.as_u64()).unwrap_or(0);
                if count == 0 {
                    return None;
                }
                Some((
                    "vision.inspect".to_string(),
                    format!("camera detected {count} object(s)"),
                ))
            }
            Salience::ScreenChanged => {
                // A content change only counts when the change type is
                // actually content — a focus switch is FocusChanged.
                let change = payload.get("change").and_then(|v| v.as_str())?;
                if change != "content" {
                    return None;
                }
                Some((
                    "screen.observe".to_string(),
                    "the foreground screen content changed".to_string(),
                ))
            }
            Salience::FocusChanged => {
                let change = payload.get("change").and_then(|v| v.as_str()).unwrap_or("");
                if change == "content" {
                    // Not a focus change — content. Let the screen rule own it.
                    return None;
                }
                Some((
                    "screen.attend".to_string(),
                    "the foreground window focus changed".to_string(),
                ))
            }
        }
    }
}

/// The built-in deterministic decision function: a fixed rule set over the
/// real sensor topics, ordered by salience. No LLM, no randomness — the same
/// workspace always yields the same decision.
pub struct SalienceDecision {
    rules: Vec<Rule>,
}

impl Default for SalienceDecision {
    fn default() -> Self {
        Self {
            rules: vec![
                Rule::new("sensor.audio.turn", Salience::UserSpoke),
                Rule::new("sensor.vision.objects", Salience::ObjectsDetected),
                Rule::new("sensor.screen", Salience::ScreenChanged),
                Rule::new("sensor.window.focus_changed", Salience::FocusChanged),
            ],
        }
    }
}

impl SalienceDecision {
    pub fn new() -> Self {
        Self::default()
    }
}

impl DecisionFunction for SalienceDecision {
    fn decide(&self, workspace: &GlobalWorkspace, state: &mut DecisionState) -> Option<TickAction> {
        // Evaluate rules from most to least salient; the first rule with new
        // AND genuinely salient evidence wins. At most one action per tick.
        let mut rules = self.rules.clone();
        rules.sort_by_key(|r| std::cmp::Reverse(r.salience.rank()));

        for rule in rules {
            let Some(event) = state.newest_undecided(workspace, rule.prefix) else {
                continue;
            };
            // Only decide on events that arrived after our own decisions
            // (never act on our own ActionDecided output — the rules don't
            // watch autonomy.action, and dedup protects the prefixes we do).
            if event.source == EngineId::Automation {
                continue;
            }
            let Some((action, reason)) = rule.evaluate(&event) else {
                // The event is new but not salient — remember it anyway so it
                // does not re-fire next tick (e.g. a transcript-less turn).
                state.mark_decided(&event);
                continue;
            };
            let action = TickAction::new(action, reason, vec![event.id], rule.salience.priority());
            state.mark_decided(&event);
            return Some(action);
        }
        None
    }
}

// ── The drive ─────────────────────────────────────────────────────────

/// The explicit result of one durable tick (R2-1). Distinguishes "execution
/// failed" from "execution was correctly refused because its world grounding
/// went stale" — radically different outcomes for the qualification ledger.
#[derive(Debug)]
pub enum DurableTickOutcome {
    /// Normal continuation. `None`: quiet world, or in-flight recovery.
    Completed(Option<ActionOutcome>),
    /// The decided action was grounded against an older world revision and
    /// was refused BEFORE any journal write or executor side effect.
    StalePlanRejected {
        planned: Option<u64>,
        current: Option<u64>,
    },
}

impl DurableTickOutcome {
    /// Unwrap the normal-continuation case. Panics on `StalePlanRejected` —
    /// use in tests/callers that expect execution to proceed.
    pub fn into_completed(self) -> Option<ActionOutcome> {
        match self {
            DurableTickOutcome::Completed(outcome) => outcome,
            DurableTickOutcome::StalePlanRejected { planned, current } => panic!(
                "stale plan rejected (planned={planned:?}, current={current:?}) but completed expected"
            ),
        }
    }
}

/// The autonomous drive: one decision function + the workspace, exposed as a
/// single `tick` (usable inside the [`crate::CognitiveRuntime`] via
/// [`PhaseHandler`], or standalone via [`AutonomousTick::run`]).
pub struct AutonomousTick {
    decision: Arc<dyn DecisionFunction>,
    state: std::sync::Mutex<DecisionState>,
}

impl AutonomousTick {
    pub fn new(decision: Arc<dyn DecisionFunction>) -> Self {
        Self {
            decision,
            state: std::sync::Mutex::new(DecisionState::new()),
        }
    }

    /// The built-in drive: salience-based decisions over the real sensors.
    pub fn with_default_decision() -> Self {
        Self::new(Arc::new(SalienceDecision::new()))
    }

    /// **The tick.** Observe → evaluate → maybe act. Returns `Some(action)`
    /// exactly when the decision function fired, and publishes it to the
    /// workspace as `Topic::ActionDecided` (`autonomy.action`) carrying the
    /// action, reason, and evidence trace — the auditable decision record.
    pub fn tick(&self, workspace: &GlobalWorkspace) -> Option<TickAction> {
        let mut state = self.state.lock().expect("decision state poisoned");
        let action = self.decision.decide(workspace, &mut state)?;
        workspace.publish_from(
            Topic::ActionDecided,
            EngineId::Automation,
            serde_json::to_value(&action).unwrap_or_default(),
        );
        tracing::debug!(
            action = %action.action,
            evidence = ?action.evidence,
            "autonomous tick fired one action"
        );
        Some(action)
    }

    /// Execute one decision with write-ahead persistence. A prepared or
    /// completed matching record is never dispatched again after restart:
    /// in-flight work returns `Completed(None)` for explicit recovery, while
    /// completed work returns its recorded outcome.
    ///
    /// R2-1 admission: when the decided action carries a `grounding_revision`
    /// and the caller supplies the live world revision, a mismatch is refused
    /// as [`DurableTickOutcome::StalePlanRejected`] BEFORE `journal.begin` or
    /// any executor side effect — a refusal, not an execution failure.
    pub async fn execute_and_verify_durable(
        &self,
        workspace: &GlobalWorkspace,
        executor: &dyn ActionExecutor,
        verifier: &dyn ActionVerifier,
        journal: &dyn ActionJournal,
        world_revision: Option<u64>,
    ) -> DurableTickOutcome {
        let action = match self.tick(workspace) {
            Some(action) => action,
            None => return DurableTickOutcome::Completed(None),
        };
        if let (Some(planned), Some(current)) = (action.grounding_revision, world_revision) {
            if planned != current {
                workspace.publish_from(
                    Topic::Custom("plan.stale_rejected".into()),
                    EngineId::Automation,
                    serde_json::json!({
                        "action": action.action,
                        "planned_revision": planned,
                        "current_revision": current,
                    }),
                );
                tracing::warn!(
                    action = %action.action,
                    planned, current,
                    "stale plan refused before any side effect"
                );
                return DurableTickOutcome::StalePlanRejected {
                    planned: Some(planned),
                    current: Some(current),
                };
            }
        }
        let record_id = match journal.begin(&action).ok() {
            Some(BeginAction::New(id)) => id,
            Some(BeginAction::InFlight(_)) => return DurableTickOutcome::Completed(None),
            Some(BeginAction::Completed(id)) => {
                let outcome = journal.records().ok().and_then(|records| {
                    records
                        .into_iter()
                        .find(|record| record.id == id)
                        .and_then(|record| record.outcome)
                });
                return DurableTickOutcome::Completed(outcome);
            }
            None => return DurableTickOutcome::Completed(None),
        };
        let execution = executor.execute(&action).await;
        workspace.publish_from(
            Topic::ToolExecuted,
            EngineId::Automation,
            serde_json::json!({
                "action": action.action,
                "success": execution.success,
                "detail": execution.detail,
                "evidence": action.evidence,
            }),
        );
        let verified = verifier.verify(workspace, &action, &execution).await;
        let outcome = ActionOutcome {
            action,
            execution,
            verified,
        };
        if journal.complete(record_id, &outcome).is_err() {
            return DurableTickOutcome::Completed(None);
        }
        workspace.publish_from(
            Topic::OutcomeObserved,
            EngineId::Automation,
            serde_json::json!({
                "action": outcome.action.action,
                "success": outcome.execution.success,
                "verified": outcome.verified,
                "evidence": outcome.action.evidence,
            }),
        );
        DurableTickOutcome::Completed(Some(outcome))
    }

    /// Continue one decision through bounded execution and explicit
    /// verification. The executor and verifier are injected so the cognitive
    /// runtime stays independent of the action-runtime crate while production
    /// wiring can reuse its permission/audit boundary.
    pub async fn execute_and_verify(
        &self,
        workspace: &GlobalWorkspace,
        executor: &dyn ActionExecutor,
        verifier: &dyn ActionVerifier,
    ) -> Option<ActionOutcome> {
        let action = self.tick(workspace)?;
        let execution = executor.execute(&action).await;
        workspace.publish_from(
            Topic::ToolExecuted,
            EngineId::Automation,
            serde_json::json!({
                "action": action.action,
                "success": execution.success,
                "detail": execution.detail,
                "evidence": action.evidence,
            }),
        );
        let verified = verifier.verify(workspace, &action, &execution).await;
        workspace.publish_from(
            Topic::OutcomeObserved,
            EngineId::Automation,
            serde_json::json!({
                "action": action.action,
                "success": execution.success,
                "verified": verified,
                "evidence": action.evidence,
            }),
        );
        Some(ActionOutcome {
            action,
            execution,
            verified,
        })
    }

    /// **The loop.** Runs without a user goal: tick → sleep → repeat, until
    /// `shutdown` flips true. Never a blind heartbeat — each tick decides
    /// honestly, so a quiet world produces no actions.
    pub async fn run(
        &self,
        workspace: &GlobalWorkspace,
        interval: Duration,
        mut shutdown: tokio::sync::watch::Receiver<bool>,
    ) {
        let mut interval = tokio::time::interval(interval);
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            tokio::select! {
                _ = interval.tick() => {
                    self.tick(workspace);
                }
                _ = shutdown.changed() => {
                    if *shutdown.borrow() {
                        break;
                    }
                }
            }
        }
    }

    /// How many ticks fired so far (metrics/observability).
    pub fn fired_count(&self) -> usize {
        // The decision state's decided entries are a lower bound; kept simple
        // and testable via the workspace history.
        self.state
            .lock()
            .expect("decision state poisoned")
            .last_decided
            .len()
    }
}

/// The drive participates in the cognitive loop on the **Act** phase — after
/// perception (Observe) has published, the tick decides what to do.
#[async_trait]
impl PhaseHandler for AutonomousTick {
    fn name(&self) -> &str {
        "autonomy"
    }

    fn phases(&self) -> Vec<CognitivePhase> {
        vec![CognitivePhase::Act]
    }

    async fn execute(
        &self,
        _phase: CognitivePhase,
        workspace: &GlobalWorkspace,
        _tick_number: u64,
    ) -> PhaseResult {
        match self.tick(workspace) {
            Some(action) => PhaseResult::Published {
                topics: vec![format!("autonomy.action:{}", action.action)],
            },
            None => PhaseResult::Ok,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn workspace_with(topic: Topic, payload: serde_json::Value) -> GlobalWorkspace {
        let ws = GlobalWorkspace::new();
        ws.publish_from(topic, EngineId::Perception, payload);
        ws
    }

    fn decision() -> Arc<dyn DecisionFunction> {
        Arc::new(SalienceDecision::new())
    }

    #[test]
    fn empty_workspace_never_fires() {
        let ws = GlobalWorkspace::new();
        let mut state = DecisionState::new();
        assert!(
            decision().decide(&ws, &mut state).is_none(),
            "no evidence, no action — never a blind heartbeat"
        );
    }

    #[test]
    fn screen_content_change_fires_with_evidence_trace() {
        let ws = workspace_with(
            Topic::Screen,
            serde_json::json!({
                "title": "Code",
                "change": "content",
                "content_hash": "abc123",
            }),
        );
        let mut state = DecisionState::new();
        let action = decision().decide(&ws, &mut state).unwrap();
        assert_eq!(action.action, "screen.observe");
        assert!(action.reason.contains("content changed"));
        assert_eq!(action.evidence.len(), 1, "cites exactly the evidence event");
        let id = ws.get("sensor.screen").unwrap().id;
        assert_eq!(action.evidence[0], id, "the evidence id is the real event");
        assert_eq!(action.priority, Priority::Normal);
    }

    #[test]
    fn focus_change_fires_lower_priority_and_does_not_steal_content() {
        let focus = workspace_with(
            Topic::WindowFocusChanged,
            serde_json::json!({ "title": "Terminal", "change": "focus" }),
        );
        let mut state = DecisionState::new();
        let action = decision().decide(&focus, &mut state).unwrap();
        assert_eq!(action.action, "screen.attend");
        assert_eq!(action.priority, Priority::Low);
    }

    #[test]
    fn transcript_gated_turn_never_guesses() {
        // Honest: no transcript → no "respond to what was said" action.
        let no_transcript = workspace_with(
            Topic::TurnDetected,
            serde_json::json!({ "frames": 10, "asr": null, "transcript": null }),
        );
        let mut state = DecisionState::new();
        assert!(
            decision().decide(&no_transcript, &mut state).is_none(),
            "a transcript-less turn is noise, not a user utterance"
        );

        let with_transcript = workspace_with(
            Topic::TurnDetected,
            serde_json::json!({ "frames": 10, "asr": "mock", "transcript": "please fix the tests" }),
        );
        let mut state = DecisionState::new();
        let action = decision().decide(&with_transcript, &mut state).unwrap();
        assert_eq!(action.action, "respond.to_user");
        assert!(action.reason.contains("please fix the tests"));
        assert_eq!(action.priority, Priority::High);
    }

    #[test]
    fn objects_fire_only_when_there_are_objects() {
        let none = workspace_with(
            Topic::ObjectDetected,
            serde_json::json!({ "count": 0, "detector": "diff" }),
        );
        let mut state = DecisionState::new();
        assert!(decision().decide(&none, &mut state).is_none());

        let some = workspace_with(
            Topic::ObjectDetected,
            serde_json::json!({
                "count": 3,
                "detector": "diff",
                "objects": [{"label": "motion", "confidence": 80}],
            }),
        );
        let mut state = DecisionState::new();
        let action = decision().decide(&some, &mut state).unwrap();
        assert_eq!(action.action, "vision.inspect");
        assert!(action.reason.contains("3"));
        assert_eq!(action.priority, Priority::High);
    }

    #[test]
    fn user_speech_beats_objects_when_both_fire() {
        let ws = GlobalWorkspace::new();
        ws.publish_from(
            Topic::ObjectDetected,
            EngineId::Perception,
            serde_json::json!({ "count": 2, "detector": "diff" }),
        );
        ws.publish_from(
            Topic::TurnDetected,
            EngineId::Perception,
            serde_json::json!({ "asr": "mock", "transcript": "stop" }),
        );
        let mut state = DecisionState::new();
        let action = decision().decide(&ws, &mut state).unwrap();
        assert_eq!(
            action.action, "respond.to_user",
            "one action per tick, highest salience wins"
        );
    }

    #[test]
    fn same_event_never_fires_twice() {
        let ws = workspace_with(
            Topic::Screen,
            serde_json::json!({ "change": "content", "content_hash": "abc" }),
        );
        let mut state = DecisionState::new();
        assert!(decision().decide(&ws, &mut state).is_some());
        assert!(
            decision().decide(&ws, &mut state).is_none(),
            "static scene stays quiet — dedup, not spam"
        );
    }

    #[test]
    fn non_salient_new_event_is_remembered_but_does_not_fire() {
        // A transcript-less turn is new but not salient; it must neither fire
        // nor re-appear next tick.
        let ws = workspace_with(
            Topic::TurnDetected,
            serde_json::json!({ "asr": null, "transcript": null }),
        );
        let mut state = DecisionState::new();
        assert!(decision().decide(&ws, &mut state).is_none());
        assert!(
            decision().decide(&ws, &mut state).is_none(),
            "still quiet — the already-seen event is remembered"
        );
    }

    #[test]
    fn tick_publishes_the_decision_with_its_evidence_trace() {
        let ws = workspace_with(
            Topic::Screen,
            serde_json::json!({ "change": "content", "content_hash": "x" }),
        );
        let tick = AutonomousTick::with_default_decision();
        let action = tick.tick(&ws).unwrap();
        assert_eq!(action.action, "screen.observe");

        let fired = ws.get_latest("autonomy.action").unwrap();
        assert_eq!(fired.topic, Topic::ActionDecided);
        assert_eq!(fired.source, EngineId::Automation);
        assert_eq!(fired.payload["action"], "screen.observe");
        assert!(!fired.payload["reason"].as_str().unwrap().is_empty());
        let evidence = fired.payload["evidence"].as_array().unwrap();
        assert_eq!(evidence.len(), 1, "the evidence trace is in the payload");
        assert_eq!(evidence[0], serde_json::json!(action.evidence[0]));
    }

    #[test]
    fn tick_returns_none_when_nothing_new_happens() {
        let ws = GlobalWorkspace::new();
        let tick = AutonomousTick::with_default_decision();
        assert!(tick.tick(&ws).is_none());
        assert!(tick.tick(&ws).is_none(), "twice quiet");
        assert!(ws.get_latest("autonomy.action").is_none());
    }

    struct SuccessfulExecutor;

    #[async_trait]
    impl ActionExecutor for SuccessfulExecutor {
        async fn execute(&self, action: &TickAction) -> ActionExecution {
            ActionExecution {
                action: action.action.clone(),
                success: true,
                detail: "dispatched".to_string(),
                output: serde_json::Value::Null,
                error: None,
            }
        }
    }

    struct EvidenceVerifier;

    #[async_trait]
    impl ActionVerifier for EvidenceVerifier {
        async fn verify(
            &self,
            _workspace: &GlobalWorkspace,
            action: &TickAction,
            execution: &ActionExecution,
        ) -> bool {
            execution.success && !action.evidence.is_empty()
        }
    }

    #[tokio::test]
    async fn execution_and_verification_publish_auditable_outcomes() {
        let ws = workspace_with(
            Topic::Screen,
            serde_json::json!({ "change": "content", "content_hash": "x" }),
        );
        let tick = AutonomousTick::with_default_decision();
        let outcome = tick
            .execute_and_verify(&ws, &SuccessfulExecutor, &EvidenceVerifier)
            .await
            .unwrap();

        assert!(outcome.verified);
        assert!(outcome.execution.success);
        assert_eq!(ws.history(Some("tool.executed")).len(), 1);
        assert_eq!(ws.history(Some("outcome.observed")).len(), 1);
        let observed = ws.get_latest("outcome.observed").unwrap();
        assert_eq!(observed.payload["verified"], true);
        assert_eq!(observed.payload["evidence"].as_array().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn execution_continuation_stays_quiet_without_decision() {
        let ws = GlobalWorkspace::new();
        let tick = AutonomousTick::with_default_decision();
        assert!(tick
            .execute_and_verify(&ws, &SuccessfulExecutor, &EvidenceVerifier)
            .await
            .is_none());
        assert!(ws.get_latest("tool.executed").is_none());
        assert!(ws.get_latest("outcome.observed").is_none());
    }

    #[tokio::test]
    async fn run_loop_fires_on_real_events_and_stops_on_shutdown() {
        let ws = GlobalWorkspace::new();
        let tick = Arc::new(AutonomousTick::with_default_decision());
        let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);

        let ws_handle = Arc::new(ws.clone());
        let tick_handle = Arc::clone(&tick);
        let loop_task = tokio::spawn(async move {
            tick_handle
                .run(&ws_handle, Duration::from_millis(5), shutdown_rx)
                .await;
        });

        // No user goal: publish a real screen change and let the drive see it.
        tokio::time::sleep(Duration::from_millis(20)).await;
        ws.publish_from(
            Topic::Screen,
            EngineId::Perception,
            serde_json::json!({ "change": "content", "content_hash": "1" }),
        );
        tokio::time::sleep(Duration::from_millis(30)).await;
        shutdown_tx.send(true).unwrap();
        loop_task.await.unwrap();

        let fired = ws.get_latest("autonomy.action").unwrap();
        assert_eq!(fired.topic, Topic::ActionDecided);
        assert_eq!(fired.payload["action"], "screen.observe");
        assert!(
            !ws.history(Some("autonomy.action")).is_empty(),
            "the drive fired without a user goal"
        );
    }

    #[test]
    fn phase_handler_registers_on_act_only() {
        let tick = AutonomousTick::with_default_decision();
        assert_eq!(tick.phases(), vec![CognitivePhase::Act]);
        assert_eq!(tick.name(), "autonomy");
    }
}
