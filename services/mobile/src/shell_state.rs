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
            "token": self.token,
        })
    }
}

/// Apply a bus event to a shell-state document (pure; unit-testable). This is
/// the state-transition core the daemon drives from bus traffic. Recognised
/// lifecycle topics:
///
///   - `plan.goal`          → begin a `current` goal
///   - `goal.completed|failed|cancelled` → close it into `last_goal`
///   - `goal.paused`        → mark `current` paused
///   - `telemetry.system`   → store the latest host sample
///
/// Anything else leaves the state unchanged.
pub fn apply_event(mut state: ShellState, ev: &Event) -> ShellState {
    match ev.kind.as_str() {
        "plan.goal" => {
            let goal = ev
                .payload
                .get("goal")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            if !goal.is_empty() {
                let id = ev
                    .payload
                    .get("trace_id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("goal")
                    .to_string();
                state.current = Some(GoalState {
                    id,
                    goal,
                    phase: "started".to_string(),
                    started_ms: Some(now_ms()),
                    completed_ms: None,
                    result: None,
                });
            }
        }
        "goal.completed" | "goal.failed" | "goal.cancelled" | "goal.paused" => {
            if let Some(cur) = &mut state.current {
                cur.completed_ms = Some(now_ms());
                if let Some(r) = ev.payload.get("result").and_then(|v| v.as_str()) {
                    cur.result = Some(r.to_string());
                }
            }
            if matches!(
                ev.kind.as_str(),
                "goal.completed" | "goal.failed" | "goal.cancelled"
            ) {
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
    fn plan_goal_opens_current() {
        let next = apply_event(state(), &goal_event());
        let cur = next.current.expect("current goal set");
        assert_eq!(cur.goal, "List the files");
        assert_eq!(cur.id, "g-1");
        assert_eq!(cur.phase, "started");
    }

    #[test]
    fn completed_goal_moves_to_last() {
        let s1 = apply_event(state(), &goal_event());
        let done = Event::new(
            "goal.completed",
            "planner",
            serde_json::json!({ "result": "did it" }),
        );
        let s2 = apply_event(s1, &done);
        assert!(s2.current.is_none());
        let last = s2.last_goal.expect("last goal preserved");
        assert_eq!(last.result.as_deref(), Some("did it"));
    }

    #[test]
    fn paused_goal_stays_current() {
        let s1 = apply_event(state(), &goal_event());
        let p = Event::new("goal.paused", "planner", serde_json::json!({}));
        let s2 = apply_event(s1, &p);
        assert!(s2.current.is_some(), "paused goal stays current");
        assert!(s2.last_goal.is_none());
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
            &Event::new("telemetry.system", "probe", serde_json::json!({ "cpu_percent": 12.0 })),
        );
        // The whole document must round-trip through the JSON shape the shell
        // reads, including the token (which the plugin must present).
        let json = s1.to_json(123, "ts".to_string());
        assert_eq!(json["version"], 1);
        assert_eq!(json["token"], "tok");
        assert_eq!(json["telemetry"]["cpu_percent"], 12.0);
    }
}
