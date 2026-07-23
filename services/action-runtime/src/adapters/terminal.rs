//! Terminal capability adapter (M4.4).
//!
//! Unlike one-shot command execution, this keeps a **persistent shell session**:
//! the same process, working directory, and environment survive across calls,
//! so state like `cd`, exported variables, and running background jobs behave as
//! a user expects. Commands are written to the shell's stdin followed by a unique
//! sentinel echo; the runtime reads merged stdout/stderr until the sentinel
//! appears, marking the command's completion.

use async_trait::async_trait;
use serde_json::json;
use std::process::Stdio;
use std::sync::Mutex as StdMutex;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStderr, ChildStdin, ChildStdout, Command};
use tokio::sync::{mpsc, Mutex as AsyncMutex};
use uuid::Uuid;

use crate::adapters::ActionAdapter;
use crate::request::{ActionRequest, ActionResult};

/// A long-lived shell process with a merged output channel.
struct TerminalSession {
    child: StdMutex<Option<Child>>,
    stdin: AsyncMutex<Option<ChildStdin>>,
    rx: AsyncMutex<mpsc::Receiver<String>>,
}

impl TerminalSession {
    fn spawn() -> Result<Self, String> {
        let shell = if cfg!(windows) { "cmd.exe" } else { "sh" };
        let mut child = Command::new(shell)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("failed to spawn {shell}: {e}"))?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| "shell had no stdin".to_string())?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| "shell had no stdout".to_string())?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| "shell had no stderr".to_string())?;
        let (tx, rx) = mpsc::channel(1024);
        tokio::spawn(merge_streams(stdout, stderr, tx));
        Ok(Self {
            child: StdMutex::new(Some(child)),
            stdin: AsyncMutex::new(Some(stdin)),
            rx: AsyncMutex::new(rx),
        })
    }

    async fn run(&self, command: &str) -> ActionResult {
        let sentinel = format!("__friday_done_{}__", Uuid::new_v4().simple());
        {
            let mut guard = self.stdin.lock().await;
            let stdin = match guard.as_mut() {
                Some(s) => s,
                None => return ActionResult::error("terminal session closed"),
            };
            let script = format!("{command}\r\necho {sentinel}\r\n");
            if let Err(e) = stdin.write_all(script.as_bytes()).await {
                return ActionResult::error(format!("stdin write failed: {e}"));
            }
            if let Err(e) = stdin.flush().await {
                return ActionResult::error(format!("stdin flush failed: {e}"));
            }
        }

        let mut out = String::new();
        let mut rx = self.rx.lock().await;
        while let Some(line) = rx.recv().await {
            if line.trim() == sentinel {
                break;
            }
            out.push_str(&line);
            out.push('\n');
        }
        ActionResult::success(json!({ "stdout": out }))
    }
}

impl Drop for TerminalSession {
    fn drop(&mut self) {
        if let Ok(mut guard) = self.child.lock() {
            if let Some(mut child) = guard.take() {
                let _ = child.start_kill();
            }
        }
    }
}

/// Forward both output streams into one ordered channel. Ordering between the
/// two pipes is best-effort; for ordinary command output this is sufficient.
async fn merge_streams(stdout: ChildStdout, stderr: ChildStderr, tx: mpsc::Sender<String>) {
    let mut out = BufReader::new(stdout).lines();
    let mut err = BufReader::new(stderr).lines();
    loop {
        tokio::select! {
            line = out.next_line() => match line {
                Ok(Some(l)) => { if tx.send(l).await.is_err() { break; } }
                Ok(None) => break,
                Err(_) => break,
            },
            line = err.next_line() => match line {
                Ok(Some(l)) => { if tx.send(l).await.is_err() { break; } }
                Ok(None) => break,
                Err(_) => break,
            },
        }
    }
}

/// Terminal adapter: lazily opens one shared [`TerminalSession`] on first use
/// and reuses it for every `run`/`exec` action.
pub struct TerminalAdapter {
    session: AsyncMutex<Option<TerminalSession>>,
}

impl Default for TerminalAdapter {
    fn default() -> Self {
        Self::new()
    }
}

impl TerminalAdapter {
    pub fn new() -> Self {
        Self {
            session: AsyncMutex::new(None),
        }
    }
}

#[async_trait]
impl ActionAdapter for TerminalAdapter {
    fn tool(&self) -> &str {
        "terminal"
    }

    fn actions(&self) -> Vec<&str> {
        vec!["run", "exec"]
    }

    async fn execute(&self, req: ActionRequest) -> ActionResult {
        let command = match req.param_str("command") {
            Ok(c) => c,
            Err(e) => return ActionResult::error(e),
        };
        let mut guard = self.session.lock().await;
        if guard.is_none() {
            let session = match TerminalSession::spawn() {
                Ok(s) => s,
                Err(e) => return ActionResult::error(format!("could not start shell: {e}")),
            };
            *guard = Some(session);
        }
        let session = guard.as_mut().expect("session just initialised");
        session.run(&command).await
    }
}

/// Capability record published to the Capability Intelligence registry.
    pub fn capabilities() -> Vec<friday_capabilities::model::Capability> {
        friday_capabilities::builtins::terminal()
    }
