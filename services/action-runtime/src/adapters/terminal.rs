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
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStderr, ChildStdin, ChildStdout, Command};
use tokio::sync::{mpsc, Mutex as AsyncMutex};
use uuid::Uuid;

use crate::adapters::ActionAdapter;
use crate::request::{ActionRequest, ActionResult};

pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(60);
pub const MAX_TIMEOUT: Duration = Duration::from_secs(600);

fn kill_process_tree(child: &mut Child) {
    let pid = child.id();
    #[cfg(unix)]
    if let Some(pid) = pid {
        unsafe {
            libc::kill(-(pid as libc::pid_t), libc::SIGKILL);
        }
    }
    #[cfg(windows)]
    if let Some(pid) = pid {
        let _ = std::process::Command::new("taskkill")
            .args(["/PID", &pid.to_string(), "/T", "/F"])
            .status();
    }
    let _ = child.start_kill();
}

/// A long-lived shell process with a merged output channel.
struct TerminalSession {
    child: StdMutex<Option<Child>>,
    stdin: AsyncMutex<Option<ChildStdin>>,
    rx: AsyncMutex<mpsc::Receiver<String>>,
}

impl TerminalSession {
    fn spawn() -> Result<Self, String> {
        let shell = if cfg!(windows) { "cmd.exe" } else { "sh" };
        let mut cmd = Command::new(shell);
        cmd.stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        #[cfg(unix)]
        cmd.process_group(0);

        let mut child = cmd
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

    fn kill(&self) {
        if let Ok(mut guard) = self.child.lock() {
            if let Some(mut child) = guard.take() {
                kill_process_tree(&mut child);
            }
        }
    }

    fn is_closed(&self) -> bool {
        self.child.lock().map(|g| g.is_none()).unwrap_or(true)
    }

    async fn run(&self, command: &str, timeout: Duration) -> ActionResult {
        let sentinel = format!("__friday_done_{}__", Uuid::new_v4().simple());
        let run_future = async {
            {
                let mut guard = self.stdin.lock().await;
                let stdin = match guard.as_mut() {
                    Some(s) => s,
                    None => return ActionResult::error("terminal session closed"),
                };
                let script = if cfg!(windows) {
                    format!("{command}\r\necho {sentinel}\r\n")
                } else {
                    format!("{command}\necho {sentinel}\n")
                };
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
        };

        match tokio::time::timeout(timeout, run_future).await {
            Ok(res) => res,
            Err(_) => {
                self.kill();
                let error_msg = if timeout.as_millis() % 1000 == 0 {
                    format!(
                        "the command did not finish in {} s and was stopped",
                        timeout.as_secs()
                    )
                } else {
                    format!(
                        "the command did not finish in {:.1} s and was stopped",
                        timeout.as_secs_f64()
                    )
                };
                ActionResult::error(error_msg)
            }
        }
    }
}

impl Drop for TerminalSession {
    fn drop(&mut self) {
        self.kill();
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
        let timeout = {
            let requested = req
                .parameters
                .get("timeout_ms")
                .and_then(|v| {
                    v.as_u64()
                        .or_else(|| v.as_i64().filter(|&n| n > 0).map(|n| n as u64))
                        .or_else(|| v.as_str().and_then(|s| s.parse::<u64>().ok()))
                })
                .map(Duration::from_millis)
                .or(req.timeout);
            requested.unwrap_or(DEFAULT_TIMEOUT).min(MAX_TIMEOUT)
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
        let result = session.run(&command, timeout).await;
        if !result.success && session.is_closed() {
            *guard = match TerminalSession::spawn() {
                Ok(new_session) => Some(new_session),
                Err(e) => {
                    tracing::error!("failed to restart shell after timeout: {e}");
                    None
                }
            };
        }
        result
    }
}

/// Capability record published to the Capability Intelligence registry.
pub fn capabilities() -> Vec<friday_capabilities::model::Capability> {
    friday_capabilities::builtins::terminal()
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::time::Duration;

    #[tokio::test]
    #[cfg(unix)]
    async fn terminal_timeout_kills_child_and_restarts_shell() {
        let adapter = TerminalAdapter::new();
        let req = ActionRequest::new(
            "terminal",
            "run",
            json!({
                "command": "sleep 30",
                "timeout_ms": 1000
            }),
        );
        let start = std::time::Instant::now();
        let res = adapter.execute(req).await;
        let elapsed = start.elapsed();

        assert!(!res.success, "expected command to time out and fail");
        let err = res.error.expect("should have error message");
        assert!(
            err.contains("the command did not finish in 1 s and was stopped"),
            "unexpected error message: {err}"
        );
        assert!(
            elapsed < Duration::from_secs(5),
            "command should time out within a few seconds, took {elapsed:?}"
        );

        // Next command should still work because shell was restarted
        let next_req = ActionRequest::new(
            "terminal",
            "run",
            json!({
                "command": "echo ok"
            }),
        );
        let next_res = adapter.execute(next_req).await;
        assert!(
            next_res.success,
            "next command after timeout should succeed: {:?}",
            next_res.error
        );
        let stdout = next_res.output["stdout"].as_str().unwrap_or_default();
        assert!(
            stdout.contains("ok"),
            "expected 'ok' in stdout, got: {stdout}"
        );
    }
}
