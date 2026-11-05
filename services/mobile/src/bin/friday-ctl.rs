//! friday-ctl — the shell's one-command interface to the FRIDAY Linux daemon.
//!
//! This is what `friday.shell` (the Quickshell plugin) calls to issue a
//! controlled command from the desktop, and what a human can run from a
//! terminal. It reads the daemon's session token + listen address from the
//! file-backed `state.json` (the same contract the plugin watches), connects
//! to the loopback WebSocket ingress, and sends one `planner.goal` command.
//!
//! Usage:
//!   friday-ctl goal "List the files in this folder"
//!   friday-ctl goal --wait "open chromium"   (block until the goal finishes;
//!                                            exit status = its outcome)
//!   friday-ctl system-info
//!   friday-ctl cancel                  (cancel the goal in flight)
//!   friday-ctl context screen          (interactive consent)
//!   friday-ctl context screen --yes    (consent already collected by the shell panel)
//!   friday-ctl context screen --yes --ask "what does this error mean?"
//!                                      (answer a question about the screen)
//!
//! The daemon's address comes from `state.json` (`listen`). `FRIDAY_CTL_ADDR`
//! and `FRIDAY_CTL_TOKEN` override the state-file values for
//! scripting/testing. `FRIDAY_CONTEXT_PROVIDER` names the provider a `context
//! screen` share goes to; unset, the daemon routes it to a vision model (the
//! user's provider before keyless OpenCode Zen).

use std::time::Duration;

use friday_mobile::protocol::{decode_server, ClientMessage, ServerMessage};
use friday_mobile::shell_state::ShellState;
use futures::{SinkExt, StreamExt};
use tokio_tungstenite::tungstenite::Message;

/// The disclosure text the terminal chooser must show **before** transmission.
/// Pulled from the frozen ContextGate so the client never duplicates the
/// system-of-record wording. A remote provider disclosure is always present.
fn screen_disclosure() -> String {
    friday_intelligence::ContextGate::new()
        .disclosure_for(
            friday_intelligence::ContextChannel::Screen,
            friday_intelligence::ProviderTarget::Subscription,
        )
        .unwrap_or("This sends a screen capture to a remote provider.")
        .to_string()
}

/// R5-2-5-3: the terminal reference implementation of the consent UX. It
/// explains what would be shared and to whom, shows data-use disclosure, then
/// demands an explicit `y` before anything is sent. Declining returns `false`
/// so the caller can abort before ever contacting the daemon — the provider is
/// guaranteed to see nothing.
fn confirm_context_consent() -> bool {
    let provider = std::env::var("FRIDAY_CONTEXT_PROVIDER")
        .ok()
        .filter(|p| !p.trim().is_empty())
        .unwrap_or_else(|| "your AI provider's vision model".to_string());

    println!("FRIDAY — share the current Screen with the assistant");
    println!();
    println!("What would be shared:");
    println!("  a screen capture of the current display (a still image)");
    println!();
    println!("Provider: {provider}");
    println!("Data use: {}", screen_disclosure());
    println!();
    print!("Share this screen? [y/N] ");
    let _ = std::io::Write::flush(&mut std::io::stdout());

    let mut answer = String::new();
    if std::io::stdin().read_line(&mut answer).is_err() {
        return false;
    }
    answer.trim().eq_ignore_ascii_case("y")
}

#[tokio::main]
async fn main() -> std::process::ExitCode {
    let raw: Vec<String> = std::env::args().skip(1).collect();

    // Resolve the daemon address + token, in precedence order: explicit CLI
    // flags, then env overrides, then the file-backed state.
    let state_path = if let Ok(p) = std::env::var("FRIDAY_STATE_FILE") {
        std::path::PathBuf::from(p)
    } else {
        ShellState::default_path()
    };
    // Address precedence: --addr, then FRIDAY_CTL_ADDR, then the address the
    // daemon published in state.json (it is not always the default port),
    // then the default.
    let mut addr_str = std::env::var("FRIDAY_CTL_ADDR")
        .ok()
        .or_else(|| read_listen(&state_path))
        .unwrap_or_else(|| "127.0.0.1:18432".to_string());
    let mut token = std::env::var("FRIDAY_CTL_TOKEN").ok().unwrap_or_default();
    let mut wait = false;
    let mut question: Option<String> = None;

    // Scan ALL tokens for flags (`--addr`/`--token`/`--wait` may appear before
    // or after the subcommand) so they never leak into the goal text. The first
    // non-flag token is the subcommand; the rest are goal words.
    let mut positional: Vec<String> = Vec::new();
    let mut i = 0;
    while i < raw.len() {
        let w = &raw[i];
        if w == "--addr" && i + 1 < raw.len() {
            addr_str = raw[i + 1].clone();
            i += 2;
        } else if w == "--token" && i + 1 < raw.len() {
            token = raw[i + 1].clone();
            i += 2;
        } else if w == "--ask" && i + 1 < raw.len() {
            question = Some(raw[i + 1].clone());
            i += 2;
        } else if w == "--wait" {
            wait = true;
            i += 1;
        } else {
            positional.push(w.clone());
            i += 1;
        }
    }

    let Some(sub) = positional.first() else {
        eprintln!("usage: friday-ctl <goal|cancel|system-info|context screen> [goal text] [--addr <host:port>] [--token <tok>]");
        return std::process::ExitCode::FAILURE;
    };
    let sub = sub.clone();
    let goal_words = positional.into_iter().skip(1).collect::<Vec<_>>();

    if sub == "context" {
        // R5-2-5-3: consent + disclosure come first, before any daemon or
        // state-file access, so a decline never touches the daemon at all. The
        // provider sees nothing unless the user explicitly answers `y`.
        //
        // R5-2-5-3-1: `--yes` means the caller (the Omarchy panel) already
        // collected explicit consent via its Share button, so the terminal
        // prompt is skipped. `consent: true` is still sent and still enforced
        // by the frozen ContextGate — this only skips the TTY prompt, it never
        // weakens the gate.
        let channel = goal_words.first().map(String::as_str).unwrap_or("");
        if channel != "screen" {
            eprintln!("friday-ctl context <screen> — only 'screen' context is supported");
            return std::process::ExitCode::FAILURE;
        }
        let assume_consent = goal_words.iter().any(|w| w == "--yes");
        if !assume_consent && !confirm_context_consent() {
            eprintln!("Aborted — nothing was shared; the provider was never contacted.");
            return std::process::ExitCode::SUCCESS;
        }
    }

    let cmd_token = if token.is_empty() {
        match read_token(&state_path) {
            Some(t) => t,
            None => {
                eprintln!(
                    "no token found; daemon not running at {} (is friday-linux-daemon up?)",
                    state_path.display()
                );
                return std::process::ExitCode::FAILURE;
            }
        }
    } else {
        token
    };

    if sub == "context" {
        return run_context(&addr_str, &cmd_token, question.as_deref()).await;
    }
    if sub == "cancel" {
        // Cancel the goal the daemon reports as in flight (state.json
        // `current`); the daemon matches it by run id.
        let Some(task_id) = read_current_goal_id(&state_path) else {
            eprintln!("no goal is running");
            return std::process::ExitCode::FAILURE;
        };
        return send_command(
            &addr_str,
            friday_mobile::method::TASK_CANCEL,
            serde_json::json!({ "token": cmd_token, "task_id": task_id }),
        )
        .await;
    }
    let goal = goal_words.join(" ");
    let submitted_ms = now_ms();
    let accepted = run(&addr_str, &cmd_token, &sub, goal_words).await;
    if sub == "goal" && wait && accepted == std::process::ExitCode::SUCCESS {
        return wait_for_goal(&state_path, &goal, submitted_ms).await;
    }
    accepted
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// `--wait`: follow the daemon's `state.json` until `goal` (submitted at
/// `since_ms`) reaches a terminal phase, then print its outcome. Exits
/// non-zero unless the goal completed. `FRIDAY_CTL_WAIT_SECS` bounds the wait
/// (default 180 s).
async fn wait_for_goal(
    state_path: &std::path::Path,
    goal: &str,
    since_ms: u64,
) -> std::process::ExitCode {
    let limit = std::env::var("FRIDAY_CTL_WAIT_SECS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(180);
    let deadline = tokio::time::Instant::now() + Duration::from_secs(limit);
    while tokio::time::Instant::now() < deadline {
        let state = std::fs::read_to_string(state_path)
            .ok()
            .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok());
        if let Some(done) = state.and_then(|s| finished_goal(&s, goal, since_ms)) {
            let summary = serde_json::json!({
                "outcome": done["outcome"],
                "result": done["result"],
                "error": done["error"],
            });
            println!(
                "{}",
                serde_json::to_string_pretty(&summary).unwrap_or_default()
            );
            return if done["outcome"] == "completed" {
                std::process::ExitCode::SUCCESS
            } else {
                std::process::ExitCode::FAILURE
            };
        }
        tokio::time::sleep(Duration::from_millis(300)).await;
    }
    eprintln!("timed out after {limit}s waiting for the goal to finish");
    std::process::ExitCode::FAILURE
}

/// The state document's `last_goal` when it is `goal`, finished, and
/// completed no earlier than `since_ms` (so an older run of the same goal
/// text is not mistaken for this one).
fn finished_goal(
    state: &serde_json::Value,
    goal: &str,
    since_ms: u64,
) -> Option<serde_json::Value> {
    let last = state.get("last_goal")?;
    let finished = last["goal"] == goal
        && last["active"] != true
        && last["completed_ms"]
            .as_u64()
            .is_some_and(|ms| ms >= since_ms);
    finished.then(|| last.clone())
}

/// The id of the goal the daemon reports as in flight, if any.
fn read_current_goal_id(path: &std::path::Path) -> Option<String> {
    let text = std::fs::read_to_string(path).ok()?;
    let st: ShellState = serde_json::from_str(&text).ok()?;
    st.current.map(|g| g.id).filter(|id| !id.is_empty())
}

/// The ingress address the daemon published in state.json, if any.
fn read_listen(path: &std::path::Path) -> Option<String> {
    let text = std::fs::read_to_string(path).ok()?;
    let st: ShellState = serde_json::from_str(&text).ok()?;
    (!st.listen.is_empty()).then_some(st.listen)
}

fn read_token(path: &std::path::Path) -> Option<String> {
    let text = std::fs::read_to_string(path).ok()?;
    let st: ShellState = serde_json::from_str(&text).ok()?;
    if st.token.is_empty() {
        return None;
    }
    Some(st.token)
}

async fn run(addr: &str, token: &str, sub: &str, args: Vec<String>) -> std::process::ExitCode {
    // Build the command. Only `goal`, `system-info`, and `context` are wired
    // for now — the "one controlled command" surface.
    let (method, params) = match sub {
        "goal" => {
            let goal = args.join(" ");
            if goal.trim().is_empty() {
                eprintln!("friday-ctl goal <text>: goal text required");
                return std::process::ExitCode::FAILURE;
            }
            (
                friday_mobile::method::PLANNER_GOAL.to_string(),
                serde_json::json!({ "goal": goal, "token": token }),
            )
        }
        "system-info" => (
            friday_mobile::method::SYSTEM_INFO.to_string(),
            serde_json::json!({ "token": token }),
        ),
        other => {
            eprintln!("unknown subcommand: {other}");
            return std::process::ExitCode::FAILURE;
        }
    };
    send_command(addr, &method, params).await
}

/// R5-2-5-3: send the Screen `context.read`. `main` has already presented
/// disclosure and collected explicit consent; this only runs after the user
/// answered `y`, so it sends with a per-use `consent: true`.
async fn run_context(addr: &str, token: &str, question: Option<&str>) -> std::process::ExitCode {
    let mut params = serde_json::json!({ "token": token, "consent": true });
    if let Some(q) = question {
        params["question"] = serde_json::Value::String(q.to_string());
    }
    send_command(addr, friday_mobile::method::CONTEXT_READ, params).await
}

/// Connect to the daemon, send one command, and await the matching
/// `CommandResult`. Any welcome/events are consumed silently beforehand.
async fn send_command(
    addr: &str,
    method: &str,
    params: serde_json::Value,
) -> std::process::ExitCode {
    let ws_url = format!("ws://{addr}");
    let (mut ws, _) = match tokio_tungstenite::connect_async(&ws_url).await {
        Ok(v) => v,
        Err(e) => {
            eprintln!("cannot reach daemon at {ws_url}: {e}");
            return std::process::ExitCode::FAILURE;
        }
    };

    let id = uuid::Uuid::new_v4().to_string();
    let want_id = id.clone();

    // In open mode the server accepts a command directly; send ours and read
    // the matching CommandResult. Any welcome/events are consumed first.
    let client_msg = ClientMessage::Command {
        id: id.clone(),
        method: method.to_string(),
        params,
    };
    let _ = ws
        .send(Message::Text(serde_json::to_string(&client_msg).unwrap()))
        .await;

    // Loop until we see our CommandResult (skipping welcome/events).
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    loop {
        if tokio::time::Instant::now() > deadline {
            eprintln!("timed out waiting for command result");
            return std::process::ExitCode::FAILURE;
        }
        let msg = match tokio::time::timeout(Duration::from_secs(5), ws.next()).await {
            Ok(Some(Ok(m))) => m,
            Ok(Some(Err(e))) => {
                eprintln!("ws error: {e}");
                return std::process::ExitCode::FAILURE;
            }
            _ => continue,
        };
        if let Message::Text(t) = msg {
            if let Ok(ServerMessage::CommandResult { id, ok, result }) = decode_server(&t) {
                if id == want_id {
                    let json = serde_json::to_string_pretty(&result).unwrap_or_default();
                    println!("{json}");
                    return if ok {
                        std::process::ExitCode::SUCCESS
                    } else {
                        std::process::ExitCode::FAILURE
                    };
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn state(goal: &str, active: bool, completed_ms: Option<u64>) -> serde_json::Value {
        json!({ "last_goal": {
            "goal": goal, "active": active, "completed_ms": completed_ms,
            "outcome": "completed", "result": "done", "error": null
        }})
    }

    #[test]
    fn wait_matches_only_this_goal_once_it_has_finished() {
        assert!(finished_goal(
            &state("open chromium", false, Some(200)),
            "open chromium",
            100
        )
        .is_some());
        assert!(
            finished_goal(
                &state("open chromium", true, Some(200)),
                "open chromium",
                100
            )
            .is_none(),
            "still running"
        );
        assert!(
            finished_goal(&state("open chromium", false, None), "open chromium", 100).is_none(),
            "not completed"
        );
        assert!(
            finished_goal(
                &state("open chromium", false, Some(50)),
                "open chromium",
                100
            )
            .is_none(),
            "an earlier run"
        );
        assert!(
            finished_goal(&state("open files", false, Some(200)), "open chromium", 100).is_none(),
            "another goal"
        );
        assert!(finished_goal(&json!({ "last_goal": null }), "open chromium", 100).is_none());
    }
}
