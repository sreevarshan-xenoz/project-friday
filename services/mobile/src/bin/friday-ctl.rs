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
//!
//! `FRIDAY_CTL_ADDR` and `FRIDAY_CTL_TOKEN` override the state-file values for
//! scripting/testing.

use std::time::Duration;

use friday_mobile::protocol::{ClientMessage, decode_server};
use friday_mobile::shell_state::ShellState;
use futures::{SinkExt, StreamExt};
use tokio_tungstenite::tungstenite::Message;

#[tokio::main]
async fn main() -> std::process::ExitCode {
    let mut args = std::env::args().skip(1);
    let Some(sub) = args.next() else {
        eprintln!("usage: friday-ctl <goal|system-info> [goal text]");
        return std::process::ExitCode::FAILURE;
    };

    // Resolve the daemon address + token from state.json, with env overrides.
    let state_path = if let Ok(p) = std::env::var("FRIDAY_STATE_FILE") {
        std::path::PathBuf::from(p)
    } else {
        ShellState::default_path()
    };
    let token = std::env::var("FRIDAY_CTL_TOKEN").ok().unwrap_or_default();
    let addr_str = std::env::var("FRIDAY_CTL_ADDR")
        .unwrap_or_else(|_| "127.0.0.1:18432".to_string());
    let rest: Vec<String> = std::env::args().skip(2).collect();
    if token.is_empty() {
        match read_token(&state_path) {
            Some(t) => run(&addr_str, &t, &sub, rest).await,
            None => {
                eprintln!(
                    "no token found; daemon not running at {} (is friday-linux-daemon up?)",
                    state_path.display()
                );
                std::process::ExitCode::FAILURE
            }
        }
    } else {
        run(&addr_str, &token, &sub, rest).await
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
    let ws_url = format!("ws://{addr}");
    let (mut ws, _) = match tokio_tungstenite::connect_async(&ws_url).await {
        Ok(v) => v,
        Err(e) => {
            eprintln!("cannot reach daemon at {ws_url}: {e}");
            return std::process::ExitCode::FAILURE;
        }
    };

    // Build the command. Only `goal` and `system-info` are wired for now — the
    // "one controlled command" surface. Everything else is rejected.
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

    let id = uuid::Uuid::new_v4().to_string();
    let want_id = id.clone();

    // In open mode the server accepts a command directly; send ours and read
    // the matching CommandResult. Any welcome/events are consumed first.
    let client_msg = ClientMessage::Command {
        id: id.clone(),
        method,
        params,
    };
    let _ = ws
        .send(Message::Text(
            serde_json::to_string(&client_msg).unwrap(),
        ))
        .await;

    // Loop until we see our CommandResult (skipping welcome/events).
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    loop {
        if tokio::time::Instant::now() > deadline {
            eprintln!("timed out waiting for command result");
            return std::process::ExitCode::FAILURE;
        }
        let msg = match tokio::time::timeout(
            Duration::from_secs(5),
            ws.next(),
        ).await {
            Ok(Some(Ok(m))) => m,
            Ok(Some(Err(e))) => {
                eprintln!("ws error: {e}");
                return std::process::ExitCode::FAILURE;
            }
            _ => continue,
        };
        if let Message::Text(t) = msg {
            if let Ok(sm) = decode_server(&t) {
                match sm {
                    friday_mobile::protocol::ServerMessage::CommandResult { id, ok, result } => {
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
                    _ => {}
                }
            }
        }
    }
}
