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
//!   friday-ctl system-info
//!   friday-ctl context screen          (interactive consent)
//!   friday-ctl context screen --yes    (consent already collected by the shell panel)
//!
//! `FRIDAY_CTL_ADDR` and `FRIDAY_CTL_TOKEN` override the state-file values for
//! scripting/testing. `FRIDAY_CONTEXT_PROVIDER` names the (remote) provider a
//! `context screen` share goes to, defaulting to `fabric` to match the daemon.

use std::time::Duration;

use friday_mobile::protocol::{ClientMessage, ServerMessage, decode_server};
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
        .unwrap_or_else(|_| "fabric".to_string());

    println!("FRIDAY — share the current Screen with the assistant");
    println!();
    println!("What would be shared:");
    println!("  a screen capture of the current display (a still image)");
    println!();
    println!("Provider: {provider} (remote)");
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
    let mut addr_str = std::env::var("FRIDAY_CTL_ADDR")
        .unwrap_or_else(|_| "127.0.0.1:18432".to_string());
    let mut token = std::env::var("FRIDAY_CTL_TOKEN").ok().unwrap_or_default();

    // Scan ALL tokens for flags (`--addr`/`--token` may appear before or after
    // the subcommand) so they never leak into the goal text. The first
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
        } else {
            positional.push(w.clone());
            i += 1;
        }
    }

    let Some(sub) = positional.first() else {
        eprintln!("usage: friday-ctl <goal|system-info|context screen> [goal text] [--addr <host:port>] [--token <tok>]");
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
        run_context(&addr_str, &cmd_token).await
    } else {
        run(&addr_str, &cmd_token, &sub, goal_words).await
    }
}

fn read_token(path: &std::path::Path) -> Option<String> {
    let text = std::fs::read_to_string(path).ok()?;
    let st: ShellState = serde_json::from_str(&text).ok()?;
    if st.token.is_empty() {
        return None;
    }
    Some(st.token)
}

async fn run(
    addr: &str,
    token: &str,
    sub: &str,
    args: Vec<String>,
) -> std::process::ExitCode {
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
async fn run_context(
    addr: &str,
    token: &str,
) -> std::process::ExitCode {
    send_command(
        addr,
        friday_mobile::method::CONTEXT_READ,
        serde_json::json!({ "token": token, "consent": true }),
    )
    .await
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
