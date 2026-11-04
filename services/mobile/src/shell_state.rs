//! FRIDAY shell state — the file-backed contract between the FRIDAY Linux
//! daemon and the `friday.shell` plugin (R5-1-1).
//!
//! The daemon publishes this as a thin JSON document (by default
//! `$XDG_STATE_HOME/friday/state.json`) on every meaningful change and on a
//! fixed heartbeat. The Quickshell plugin watches it via
//! [`FileView`](https://quickshell.outfoxxed.me/docs/libquickshell/utility/) /
//! inotify and re-renders, mirroring Omarchy's `agents` plugin precedent:
//! an external CLI writes JSON to state, a bar widget renders it.
//!
//! This is deliberately the *smallest* surface: `status`, `current` (goal +
//! task + phase), `cognition`, `voice`, `telemetry`, and `command` (the
//! last-accepted shell command, so the UI can echo it). Live per-cycle detail
//! stays on the WebSocket read-model; `state.json` is the boot-time + widget
//! fallback layer.

use std::collections::HashMap;

use friday_core::event::Event;
use serde_json::json;

/// The phase names a goal passes through (mirrors the Executive's 11-phase
/// cycle and `ExecutivePhaseStateDto`).
pub const PHASE_ORDER: &[&str] = &[
    "observe",
    "attend",
    "investigate",
    "plan",
    "simulate",
    "execute",
    "verify",
    "reflect",
    "consolidate",
    "learn",
    "idle",
];

/// A goal the shell knows about.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
pub struct GoalState {
    pub id: String,
    pub goal: String,
    pub phase: String,
    pub started_ms: Option<u64>,
    pub completed_ms: Option<u64>,
    pub result: Option<String>,
    /// Genuine terminal outcome one of
    /// `completed` | `failed` | `cancelled` | `paused` | `recoverable`; `None`
    /// while the goal is still active. Only ever set from a real bus event —
    /// never manufactured.
    pub outcome: Option<String>,
    /// True while a goal is genuinely in flight (`goal.started` observed,
    /// no terminal event yet).
    pub active: bool,
    /// The Executive's `phases` field carried by the terminal event (the count
    /// of phases the cycle actually completed). Only present when the runtime
    /// supplied it.
    pub phases: Option<u64>,
    /// The `error`/`reason` the runtime attached to a non-completed terminal
    /// event (failed/cancelled/paused/recoverable).
    pub error: Option<String>,
    /// The `strategy` field a completed goal carries (`llm` | `heuristic`).
    pub strategy: Option<String>,
    /// The `evidence_verified` count a completed goal carries.
    pub evidence_verified: Option<u64>,
}

/// The live file-backed contract the shell reads.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
pub struct ShellState {
    /// Daemon service status: `starting` | `running` | `stopping` | `error`.
    pub status: String,
    /// Epoch millis of the last write.
    pub updated_ms: u64,
    /// Wall clock of the last write (ISO 8601, UTC).
    pub updated_utc: String,
    /// Protocol version (bump on incompatible `state.json` changes).
    pub version: u32,
    /// The currently active goal (if any). `None` when idle.
    pub current: Option<GoalState>,
    /// The last goal that reached a terminal state (completed/failed/cancelled).
    pub last_goal: Option<GoalState>,
    /// Cognition summary — list of named slots the daemon last observed.
    pub cognition: HashMap<String, serde_json::Value>,
    /// Voice capability state: `on` | `off` | `misconfigured`.
    pub voice: String,
    /// Last host telemetry sample (from the system probe).
    pub telemetry: Option<serde_json::Value>,
    /// Last shell command accepted (for echo in the UI), or `None`.
    pub command: Option<CommandRecord>,
    /// Last explicit context read outcome (R5-2-5-2) surfaced to the shell, or
    /// `None` if none has been attempted.
    pub context_read: Option<ContextReadRecord>,
    /// The address the command ingress actually bound (e.g. `127.0.0.1:18432`).
    /// Clients connect here; it is not always the default port (another
    /// user's daemon may hold it). Empty in documents from older daemons.
    #[serde(default)]
    pub listen: String,
    /// A stable session token the plugin must present when it issues commands
    /// over the WebSocket ingress (loopback is the coarse trust boundary; this
    /// token is the fine-grained one, mirroring Omarchy shell validation).
    pub token: String,
}

/// A record of a command the daemon accepted from the shell.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
pub struct CommandRecord {
    pub method: String,
    pub accepted_ms: u64,
    pub goal: Option<String>,
}

/// Outcome of one explicit context read (R5-2-5-2) for the shell read-model.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
pub struct ContextReadRecord {
    pub method: String,
    pub ms: u64,
    /// `true` = captured, authorized, egress-safe, and delivered to the provider.
    pub ok: bool,
    /// The channel that was requested (`screen` in this slice).
    pub channel: String,
}

impl ShellState {
    /// A starting-state document with a freshly generated session token.
    pub fn starting(token: String) -> Self {
        Self {
            status: "starting".into(),
            updated_ms: 0,
            updated_utc: String::new(),
            version: 1,
            current: None,
            last_goal: None,
            cognition: HashMap::new(),
            voice: "off".into(),
            telemetry: None,
            command: None,
            context_read: None,
            listen: String::new(),
            token,
        }
    }

    /// Build the default `state.json` path from the environment.
    ///
    /// Precedence: `FRIDAY_STATE_FILE` (explicit override), then
    /// `$XDG_STATE_HOME/friday/state.json`, then
    /// `$HOME/.local/state/friday/state.json`. On Omarchy, `$XDG_STATE_HOME`
    /// is `~/.local/state`, so this lands alongside the Omarchy state dirs the
    /// shell already watches.
    pub fn default_path() -> std::path::PathBuf {
        if let Ok(p) = std::env::var("FRIDAY_STATE_FILE") {
            return std::path::PathBuf::from(p);
        }
        let base = std::env::var("XDG_STATE_HOME")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|_| {
                std::env::var("HOME")
                    .map(|h| std::path::PathBuf::from(h).join(".local/state"))
                    .unwrap_or_else(|_| std::path::PathBuf::from("/tmp"))
            });
        base.join("friday/state.json")
    }

    /// Render the document as a JSON value with the updated timestamp stamped.
    pub fn to_json(&self, now_ms: u64, now_utc: String) -> serde_json::Value {
        json!({
            "status": self.status,
            "updated_ms": now_ms,
            "updated_utc": now_utc,
            "version": self.version,
            "current": self.current,
            "last_goal": self.last_goal,
            "cognition": self.cognition,
            "voice": self.voice,
            "telemetry": self.telemetry,
            "command": self.command,
            "listen": self.listen,
            "token": self.token,
        })
    }
}

/// Apply a bus event to a shell-state document (pure; unit-testable). This is
/// the state-transition core the daemon drives from bus traffic. Recognised
/// lifecycle topics:
///
///   - `plan.goal`          → begin a `current` goal (command ingress)
///   - `goal.started`       → mark the `current` goal genuinely in flight
///   - `goal.completed|failed|cancelled|paused|recoverable` → close it into
///     `last_goal` with any genuine outcome detail the runtime attached
///     (phases, result, error, strategy, evidence_verified)
///   - `telemetry.system`   → store the latest host sample
///
/// Anything else leaves the state unchanged. This core only records what the
/// runtime genuinely publishes — it never invents a phase, a metric, or an
/// outcome.
pub fn apply_event(mut state: ShellState, ev: &Event) -> ShellState {
    let kind = ev.kind.as_str();
    match kind {
        "plan.goal" | "goal.started" => {
            let goal = ev
                .payload
                .get("goal")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            // `plan.goal` keys the id off `trace_id`; `goal.started` keys it
            // off `id` directly.
            let id = ev
                .payload
                .get("id")
                .and_then(|v| v.as_str())
                .or_else(|| ev.payload.get("trace_id").and_then(|v| v.as_str()))
                .unwrap_or("goal")
                .to_string();
            if !goal.is_empty() {
                state.current = Some(GoalState {
                    id,
                    goal,
                    phase: if kind == "plan.goal" {
                        // The command was accepted; the runner will fire
                        // `goal.started` when the Executive actually begins.
                        "accepted".to_string()
                    } else {
                        "active".to_string()
                    },
                    started_ms: Some(now_ms()),
                    completed_ms: None,
                    result: None,
                    outcome: None,
                    active: true,
                    phases: None,
                    error: None,
                    strategy: None,
                    evidence_verified: None,
                });
            }
        }
        "goal.completed" | "goal.failed" | "goal.cancelled" | "goal.paused"
        | "goal.recoverable" => {
            if let Some(cur) = &mut state.current {
                cur.completed_ms = Some(now_ms());
                cur.active = false;
                cur.phase = match kind {
                    "goal.recoverable" => "failed".to_string(),
                    other => other.trim_start_matches("goal.").to_string(),
                };
                cur.outcome = Some(kind.trim_start_matches("goal.").to_string());
                if let Some(r) = ev.payload.get("result").and_then(|v| v.as_str()) {
                    cur.result = Some(r.to_string());
                }
                // Genuine outcome detail — only when the runtime supplied it.
                if let Some(p) = ev.payload.get("phases") {
                    cur.phases = p.as_u64();
                }
                if let Some(e) = ev
                    .payload
                    .get("error")
                    .and_then(|v| v.as_str())
                    .or_else(|| ev.payload.get("reason").and_then(|v| v.as_str()))
                {
                    cur.error = Some(e.to_string());
                }
                if let Some(s) = ev.payload.get("strategy").and_then(|v| v.as_str()) {
                    cur.strategy = Some(s.to_string());
                }
                if let Some(v) = ev.payload.get("evidence_verified") {
                    cur.evidence_verified = v.as_u64();
                }
            }
            if matches!(kind, "goal.completed" | "goal.failed" | "goal.cancelled") {
                state.last_goal = state.current.take();
            }
        }
        "telemetry.system" => {
            state.telemetry = Some(ev.payload.clone());
        }
        _ => {}
    }
    state
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {

    #[test]
    fn listen_address_is_published() {
        let mut st = ShellState::starting("tok".into());
        st.listen = "127.0.0.1:42301".into();
        let json = st.to_json(1, "t".into());
        assert_eq!(json["listen"], "127.0.0.1:42301");
    }

    #[test]
    fn state_from_an_older_daemon_without_listen_still_parses() {
        let old = r#"{"status":"running","updated_ms":1,"updated_utc":"t","version":1,
            "current":null,"last_goal":null,"cognition":{},"voice":"off",
            "telemetry":null,"command":null,"token":"tok"}"#;
        let st: ShellState = serde_json::from_str(old).expect("older state.json parses");
        assert_eq!(st.listen, "");
        assert_eq!(st.token, "tok");
    }
    use super::*;

    fn state() -> ShellState {
        ShellState::starting("tok".to_string())
    }

    fn goal_event() -> Event {
        Event::new(
            "plan.goal",
            "shell",
            serde_json::json!({ "goal": "List the files", "trace_id": "g-1" }),
        )
    }

    #[test]
    fn plan_goal_opens_current_accepted() {
        let next = apply_event(state(), &goal_event());
        let cur = next.current.expect("current goal set");
        assert_eq!(cur.goal, "List the files");
        assert_eq!(cur.id, "g-1");
        assert_eq!(cur.phase, "accepted");
        assert!(cur.active, "an accepted goal is tracked as in flight");
    }

    #[test]
    fn goal_started_marks_current_active() {
        let started = apply_event(
            state(),
            &Event::new(
                "goal.started",
                "planner",
                serde_json::json!({ "goal": "Research Gemma", "id": "g-2" }),
            ),
        );
        let cur = started.current.expect("started goal set");
        assert_eq!(cur.phase, "active");
        assert!(cur.active);
        assert_eq!(cur.id, "g-2");
    }

    #[test]
    fn completed_goal_moves_to_last_with_genuine_detail() {
        let s1 = apply_event(state(), &goal_event());
        let done = Event::new(
            "goal.completed",
            "planner",
            serde_json::json!({
                "result": "did it",
                "phases": 11,
                "strategy": "llm",
                "evidence_verified": 3,
            }),
        );
        let s2 = apply_event(s1, &done);
        assert!(s2.current.is_none());
        let last = s2.last_goal.expect("last goal preserved");
        assert_eq!(last.result.as_deref(), Some("did it"));
        assert_eq!(last.outcome.as_deref(), Some("completed"));
        assert_eq!(last.phases, Some(11));
        assert_eq!(last.strategy.as_deref(), Some("llm"));
        assert_eq!(last.evidence_verified, Some(3));
        assert!(!last.active, "terminal goal is no longer active");
    }

    #[test]
    fn failed_goal_records_error_and_phases() {
        let s1 = apply_event(state(), &goal_event());
        let s2 = apply_event(
            s1,
            &Event::new(
                "goal.failed",
                "planner",
                serde_json::json!({ "error": "action failed", "phases": 4 }),
            ),
        );
        let last = s2.last_goal.expect("failed goal preserved");
        assert_eq!(last.outcome.as_deref(), Some("failed"));
        assert_eq!(last.error.as_deref(), Some("action failed"));
        assert_eq!(last.phases, Some(4));
        // A failed goal carries no fabricated result.
        assert_eq!(last.result, None);
    }

    #[test]
    fn paused_recoverable_stay_current_without_manufactured_state() {
        let s1 = apply_event(state(), &goal_event());
        let paused = apply_event(
            s1,
            &Event::new("goal.paused", "planner", serde_json::json!({})),
        );
        assert!(paused.current.is_some(), "paused goal stays current");
        assert!(paused.last_goal.is_none());
        assert_eq!(
            paused.current.as_ref().unwrap().outcome.as_deref(),
            Some("paused")
        );

        // A recoverable failure stays current (retryable) and carries its reason.
        let s2 = apply_event(state(), &goal_event());
        let rec = apply_event(
            s2,
            &Event::new(
                "goal.recoverable",
                "planner",
                serde_json::json!({ "failed_reason": "retryable", "recovery_attempts": 3 }),
            ),
        );
        assert!(rec.current.is_some(), "recoverable goal stays current");
        assert_eq!(
            rec.current.as_ref().unwrap().outcome.as_deref(),
            Some("recoverable")
        );
    }

    #[test]
    fn terminal_outcome_is_only_from_a_real_event() {
        // A goal that never receives a terminal event must not show an outcome.
        let next = apply_event(state(), &goal_event());
        assert_eq!(next.current.as_ref().unwrap().outcome, None);
    }

    #[test]
    fn unknown_events_leave_state_untouched() {
        let s = state();
        let ev = Event::new("something.else", "x", serde_json::json!({}));
        let next = apply_event(s.clone(), &ev);
        assert_eq!(next.current, s.current);
        assert_eq!(next.last_goal, s.last_goal);
        assert_eq!(next.telemetry, s.telemetry);
    }

    #[test]
    fn telemetry_is_stored_and_serializable() {
        let s1 = apply_event(
            state(),
            &Event::new(
                "telemetry.system",
                "probe",
                serde_json::json!({ "cpu_percent": 12.0 }),
            ),
        );
        // The whole document must round-trip through the JSON shape the shell
        // reads, including the token (which the plugin must present).
        let json = s1.to_json(123, "ts".to_string());
        assert_eq!(json["version"], 1);
        assert_eq!(json["token"], "tok");
        assert_eq!(json["telemetry"]["cpu_percent"], 12.0);
    }
}
