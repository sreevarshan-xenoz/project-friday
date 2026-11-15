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
    /// The answer so far while it streams (`goal.progress`); `None` once the
    /// goal ends (its `result` holds the whole answer) or before it starts.
    #[serde(default)]
    pub partial: Option<String>,
    /// The gated tool action requiring user confirmation, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub needs_confirmation: Option<NeedConfirmation>,
    /// The AI provider and model that served the answer, plus total elapsed ms.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub served_by: Option<ServedBy>,
    /// Number of secrets masked before sending to the model (if any were masked).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub masked: Option<usize>,
}

/// Details of the provider and model that served the answer.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct ServedBy {
    pub provider: String,
    pub model: String,
    pub ms: u64,
}

/// A pending confirmation requirement for a gated action.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct NeedConfirmation {
    pub tool: String,
    pub action: String,
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
    /// Whether FRIDAY is running in private mode (local models only).
    #[serde(default, rename = "private")]
    pub r#private: bool,
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
        let is_private = std::env::var("FRIDAY_PRIVATE")
            .map(|v| v == "true" || v == "1")
            .unwrap_or(false);
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
            r#private: is_private,
        }
    }

    /// Build the default `state.json` path from the environment.
    ///
    /// Precedence: `FRIDAY_STATE_FILE` (explicit override), then
    /// `$XDG_STATE_HOME/friday/state.json`, then on Windows
    /// `%LOCALAPPDATA%\friday\state.json`, then
    /// `$HOME/.local/state/friday/state.json`. On Omarchy, `$XDG_STATE_HOME`
    /// is `~/.local/state`, so this lands alongside the Omarchy state dirs the
    /// shell already watches.
    pub fn default_path() -> std::path::PathBuf {
        Self::path_from(|k| std::env::var(k).ok(), cfg!(windows))
    }

    /// The resolution behind [`Self::default_path`], with the environment and
    /// the platform passed in so both platforms' rules are testable anywhere.
    pub fn path_from(var: impl Fn(&str) -> Option<String>, windows: bool) -> std::path::PathBuf {
        if let Some(p) = var("FRIDAY_STATE_FILE") {
            return std::path::PathBuf::from(p);
        }
        friday_intelligence::egress::state_dir_from(var, windows).join("state.json")
    }

    /// Render the document as a JSON value with the updated timestamp stamped.
    pub fn to_json(&self, now_ms: u64, now_utc: String) -> serde_json::Value {
        json!({
            "status": self.status,
            "updated_ms": now_ms,
            "updated_utc": now_utc,
            "version": self.version,
            "private": self.r#private,
            "current": self.current,
            "last_goal": self.last_goal,
            "cognition": self.cognition,
            "voice": self.voice,
            "telemetry": self.telemetry,
            "command": self.command,
            "context_read": self.context_read,
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
///   - `goal.progress`      → the answer so far for that goal (`partial`)
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
            let raw_goal = ev
                .payload
                .get("goal")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let (goal, _) = friday_goal_execution::mask_secrets(&raw_goal);
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
                    partial: None,
                    needs_confirmation: None,
                    served_by: None,
                    masked: None,
                });
            }
        }
        "goal.completed" | "goal.failed" | "goal.cancelled" | "goal.paused"
        | "goal.recoverable" => {
            if let Some(cur) = &mut state.current {
                cur.completed_ms = Some(now_ms());
                cur.active = false;
                cur.partial = None;
                cur.phase = match kind {
                    "goal.recoverable" => "failed".to_string(),
                    other => other.trim_start_matches("goal.").to_string(),
                };
                cur.outcome = Some(kind.trim_start_matches("goal.").to_string());
                // `result` is a GoalResult object ({kind, text, ...}); older
                // events carried a plain string. Either way the text is what
                // friday-ctl --wait and `friday ask` show; an empty result
                // stays None.
                let text = match ev.payload.get("result") {
                    Some(serde_json::Value::String(t)) => Some(t.as_str()),
                    Some(v) => v.get("text").and_then(|t| t.as_str()),
                    None => None,
                };
                if let Some(t) = text.filter(|t| !t.trim().is_empty()) {
                    cur.result = Some(t.to_string());
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
                if let Some(nc) = ev.payload.get("needs_confirmation") {
                    cur.needs_confirmation = serde_json::from_value(nc.clone()).ok();
                }
                if let Some(s) = ev.payload.get("strategy").and_then(|v| v.as_str()) {
                    cur.strategy = Some(s.to_string());
                }
                if let Some(v) = ev.payload.get("evidence_verified") {
                    cur.evidence_verified = v.as_u64();
                }
                if let Some(sb) = ev.payload.get("served_by") {
                    cur.served_by = serde_json::from_value(sb.clone()).ok();
                }
                if let Some(m) = ev.payload.get("masked").and_then(|v| v.as_u64()) {
                    cur.masked = Some(m as usize);
                }
            }
            if matches!(kind, "goal.completed" | "goal.failed" | "goal.cancelled") {
                state.last_goal = state.current.take();
            }
        }
        "goal.progress" => {
            // Only the goal in flight, and only while it is.
            let id = ev.payload.get("id").and_then(|v| v.as_str());
            if let (Some(cur), Some(partial)) = (
                state.current.as_mut(),
                ev.payload.get("partial").and_then(|v| v.as_str()),
            ) {
                if cur.active && id == Some(cur.id.as_str()) {
                    cur.partial = Some(partial.to_string());
                }
            }
        }
        "telemetry.system" => {
            state.telemetry = Some(ev.payload.clone());
        }
        _ => {}
    }
    state
}

pub fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

pub fn now_utc() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

pub fn render(state: &ShellState) -> serde_json::Value {
    state.to_json(now_ms(), now_utc())
}

/// Atomically write the state document (tmp + rename) so any inotify watch
/// never observes a half-written file. Owner-only (0600): it carries the
/// session token that authorizes commands.
/// Where the daemon keeps its memory journal: `FRIDAY_MEMORY_DIR` when set;
/// otherwise `memory` beside `state.json`. An older install wrote `friday-data`
/// in whatever directory the daemon started in (the home folder, under
/// systemd): that one stays in use while it exists, so nothing is lost.
pub fn memory_dir(
    env_dir: Option<String>,
    state_file: &std::path::Path,
    legacy: &std::path::Path,
) -> std::path::PathBuf {
    if let Some(dir) = env_dir.filter(|d| !d.trim().is_empty()) {
        return dir.into();
    }
    if legacy.join("episodes.jsonl").exists() {
        return legacy.to_path_buf();
    }
    state_file
        .parent()
        .map(|d| d.join("memory"))
        .unwrap_or_else(|| legacy.to_path_buf())
}

pub fn write_state(path: &std::path::Path, value: &serde_json::Value) {
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let tmp = path.with_extension("json.tmp");
    let mut f = match std::fs::File::create(&tmp) {
        Ok(f) => f,
        Err(e) => {
            tracing::warn!("state: cannot open tmp {}: {e}", tmp.display());
            return;
        }
    };
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Err(e) = f.set_permissions(std::fs::Permissions::from_mode(0o600)) {
            tracing::warn!("state: cannot restrict {}: {e}", tmp.display());
            return;
        }
    }
    let bytes = match serde_json::to_vec_pretty(value) {
        Ok(b) => b,
        Err(e) => {
            tracing::warn!("state: serialize failed: {e}");
            return;
        }
    };
    use std::io::Write;
    if let Err(e) = f.write_all(&bytes) {
        tracing::warn!("state: write failed: {e}");
        return;
    }
    if let Err(e) = f.sync_all() {
        tracing::warn!("state: sync failed: {e}");
    }
    drop(f);
    if let Err(e) = std::fs::rename(&tmp, path) {
        tracing::warn!("state: rename failed: {e}");
    }
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
    fn failed_goal_records_needs_confirmation() {
        let s1 = apply_event(state(), &goal_event());
        let failed = Event::new(
            "goal.failed",
            "planner",
            serde_json::json!({
                "error": "Confirm to run: git status",
                "needs_confirmation": {
                    "tool": "terminal",
                    "action": "git status"
                }
            }),
        );
        let s2 = apply_event(s1, &failed);
        let last = s2.last_goal.expect("failed goal preserved");
        assert_eq!(last.outcome.as_deref(), Some("failed"));
        let nc = last.needs_confirmation.expect("needs confirmation set");
        assert_eq!(nc.tool, "terminal");
        assert_eq!(nc.action, "git status");
    }

    #[test]
    fn older_state_with_goal_lacking_needs_confirmation_parses() {
        let old = r#"{
            "status": "running",
            "updated_ms": 1,
            "updated_utc": "t",
            "version": 1,
            "current": null,
            "last_goal": {
                "id": "g-1",
                "goal": "git status",
                "phase": "failed",
                "started_ms": 1,
                "completed_ms": 2,
                "result": null,
                "outcome": "failed",
                "active": false,
                "phases": 1,
                "error": "Confirm to run: git status",
                "strategy": null,
                "evidence_verified": null
            },
            "cognition": {},
            "voice": "off",
            "telemetry": null,
            "command": null,
            "token": "tok"
        }"#;
        let st: ShellState = serde_json::from_str(old).expect("older state.json parses");
        let last = st.last_goal.expect("last goal present");
        assert_eq!(last.needs_confirmation, None);
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

    #[test]
    fn memory_lives_beside_state_json_unless_told_or_already_elsewhere() {
        let tmp = std::env::temp_dir().join(format!("friday-mem-{}", std::process::id()));
        let legacy = tmp.join("friday-data");
        let state = tmp.join("state/friday/state.json");
        let _ = std::fs::remove_dir_all(&tmp);
        // Nothing yet: beside state.json.
        assert_eq!(
            memory_dir(None, &state, &legacy),
            tmp.join("state/friday/memory")
        );
        // An explicit directory always wins; a blank one is no setting.
        assert_eq!(
            memory_dir(Some("/data/mem".into()), &state, &legacy),
            std::path::PathBuf::from("/data/mem")
        );
        assert_eq!(
            memory_dir(Some("  ".into()), &state, &legacy),
            tmp.join("state/friday/memory")
        );
        // An older install's journal stays where it is.
        std::fs::create_dir_all(&legacy).unwrap();
        std::fs::write(legacy.join("episodes.jsonl"), "").unwrap();
        assert_eq!(memory_dir(None, &state, &legacy), legacy);
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn state_path_follows_platform_conventions() {
        use std::path::PathBuf;
        let env = |pairs: &'static [(&'static str, &'static str)]| {
            move |k: &str| {
                pairs
                    .iter()
                    .find(|(n, _)| *n == k)
                    .map(|(_, v)| v.to_string())
            }
        };
        // An explicit override wins everywhere.
        let p = ShellState::path_from(
            env(&[("FRIDAY_STATE_FILE", "/x/s.json"), ("LOCALAPPDATA", "C:/L")]),
            true,
        );
        assert_eq!(p, PathBuf::from("/x/s.json"));
        // Linux: XDG state home, then ~/.local/state; LOCALAPPDATA is ignored.
        let p = ShellState::path_from(env(&[("XDG_STATE_HOME", "/s"), ("HOME", "/h")]), false);
        assert_eq!(p, PathBuf::from("/s").join("friday").join("state.json"));
        let p = ShellState::path_from(env(&[("HOME", "/h"), ("LOCALAPPDATA", "C:/L")]), false);
        assert_eq!(
            p,
            PathBuf::from("/h")
                .join(".local")
                .join("state")
                .join("friday")
                .join("state.json")
        );
        // Windows: %LOCALAPPDATA%\friday\state.json, even with HOME set
        // (Git Bash and MSYS export one).
        let p = ShellState::path_from(env(&[("HOME", "/h"), ("LOCALAPPDATA", "C:/L")]), true);
        assert_eq!(p, PathBuf::from("C:/L").join("friday").join("state.json"));
    }

    /// The runner's real payload: `result` is a GoalResult object. Its text
    /// must reach state.json (friday-ctl --wait and `friday ask` read it); the
    /// test above only ever fed a plain string, which hid that it didn't.
    #[test]
    fn completed_goal_result_object_text_reaches_state() {
        let done = Event::new(
            "goal.completed",
            "planner",
            serde_json::json!({
                "result": { "kind": "text", "text": "the answer", "tool": "answer", "success": true },
            }),
        );
        let last = apply_event(apply_event(state(), &goal_event()), &done)
            .last_goal
            .expect("last goal preserved");
        assert_eq!(last.result.as_deref(), Some("the answer"));

        // An explicit empty result records no result, not an empty string.
        let empty = Event::new(
            "goal.completed",
            "planner",
            serde_json::json!({ "result": { "kind": "empty", "text": "" } }),
        );
        let last = apply_event(apply_event(state(), &goal_event()), &empty)
            .last_goal
            .expect("last goal preserved");
        assert_eq!(last.result, None);
    }

    #[test]
    fn progress_fills_partial_for_the_goal_in_flight_then_clears() {
        let st = apply_event(
            state(),
            &Event::new(
                "goal.started",
                "planner",
                serde_json::json!({"goal": "q", "id": "g1"}),
            ),
        );
        let progress = |id: &str, text: &str| {
            Event::new(
                "goal.progress",
                "planner",
                serde_json::json!({"id": id, "partial": text}),
            )
        };
        let st = apply_event(st, &progress("g1", "The answer"));
        assert_eq!(
            st.current.as_ref().unwrap().partial.as_deref(),
            Some("The answer")
        );
        // Another goal's progress is ignored.
        let st = apply_event(st, &progress("other", "nope"));
        assert_eq!(
            st.current.as_ref().unwrap().partial.as_deref(),
            Some("The answer")
        );
        let st = apply_event(
            st,
            &Event::new(
                "goal.completed",
                "planner",
                serde_json::json!({"result": {"text": "The answer is 42."}}),
            ),
        );
        let done = st.last_goal.as_ref().unwrap();
        assert_eq!(done.partial, None, "the result replaces the partial");
        assert_eq!(done.result.as_deref(), Some("The answer is 42."));
        // Progress after the end changes nothing.
        let st = apply_event(st, &progress("g1", "late"));
        assert!(st.current.is_none());
    }

    #[test]
    fn completed_goal_records_served_by_in_last_goal_and_state_json() {
        let s1 = apply_event(state(), &goal_event());
        let done = Event::new(
            "goal.completed",
            "planner",
            serde_json::json!({
                "result": "did it",
                "served_by": {
                    "provider": "opencode-zen",
                    "model": "inclusionai/ling-3.0-flash:free",
                    "ms": 2300,
                }
            }),
        );
        let s2 = apply_event(s1, &done);
        let last = s2.last_goal.as_ref().expect("last goal preserved");
        let sb = last.served_by.as_ref().expect("served_by preserved");
        assert_eq!(sb.provider, "opencode-zen");
        assert_eq!(sb.model, "inclusionai/ling-3.0-flash:free");
        assert_eq!(sb.ms, 2300);

        // state.json output includes served_by
        let rendered = render(&s2);
        let last_rendered = &rendered["last_goal"];
        assert_eq!(last_rendered["served_by"]["provider"], "opencode-zen");
        assert_eq!(
            last_rendered["served_by"]["model"],
            "inclusionai/ling-3.0-flash:free"
        );
        assert_eq!(last_rendered["served_by"]["ms"], 2300);
    }

    #[test]
    fn completed_goal_omits_served_by_when_absent() {
        let s1 = apply_event(state(), &goal_event());
        let done = Event::new(
            "goal.completed",
            "planner",
            serde_json::json!({
                "result": "did it",
            }),
        );
        let s2 = apply_event(s1, &done);
        let last = s2.last_goal.as_ref().expect("last goal preserved");
        assert!(last.served_by.is_none());

        // state.json output omits served_by
        let rendered = render(&s2);
        let last_rendered = &rendered["last_goal"];
        assert!(last_rendered.get("served_by").is_none());
    }

    #[test]
    fn completed_goal_records_masked_in_last_goal_and_state_json() {
        let s1 = apply_event(state(), &goal_event());
        let done = Event::new(
            "goal.completed",
            "planner",
            serde_json::json!({
                "result": "did it",
                "masked": 2,
            }),
        );
        let s2 = apply_event(s1, &done);
        let last = s2.last_goal.as_ref().expect("last goal preserved");
        assert_eq!(last.masked, Some(2));

        // state.json output includes masked
        let rendered = render(&s2);
        let last_rendered = &rendered["last_goal"];
        assert_eq!(last_rendered["masked"], 2);
    }

    #[test]
    fn completed_goal_omits_masked_when_absent() {
        let s1 = apply_event(state(), &goal_event());
        let done = Event::new(
            "goal.completed",
            "planner",
            serde_json::json!({
                "result": "did it",
            }),
        );
        let s2 = apply_event(s1, &done);
        let last = s2.last_goal.as_ref().expect("last goal preserved");
        assert!(last.masked.is_none());

        // state.json output omits masked
        let rendered = render(&s2);
        let last_rendered = &rendered["last_goal"];
        assert!(last_rendered.get("masked").is_none());
    }

    #[test]
    fn state_json_private_field() {
        // Default starting state
        let mut st = ShellState::starting("token-123".into());
        assert!(!st.r#private);
        let rendered = render(&st);
        assert_eq!(rendered["private"], false);

        // Explicitly private
        st.r#private = true;
        let rendered_priv = render(&st);
        assert_eq!(rendered_priv["private"], true);

        // Deserializing JSON without "private" defaults to false
        let older = r#"{"status":"running","updated_ms":0,"updated_utc":"","version":1,"cognition":{},"voice":"off","listen":"","token":"abc"}"#;
        let parsed: ShellState = serde_json::from_str(older).expect("older state.json parses");
        assert!(!parsed.r#private);

        // Deserializing JSON with "private": true
        let priv_json = r#"{"status":"running","updated_ms":0,"updated_utc":"","version":1,"private":true,"cognition":{},"voice":"off","listen":"","token":"abc"}"#;
        let parsed_priv: ShellState =
            serde_json::from_str(priv_json).expect("private state.json parses");
        assert!(parsed_priv.r#private);
    }
}
