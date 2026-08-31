//! The command boundary between the collector and the OS (R5-2-5-1).
//!
//! The collector is a pure OS capability: it issues capture commands and
//! interprets their output. **It must not decide whether context may leave the
//! machine** — that is [`friday_intelligence::context_gate::ContextGate`]'s job,
//! which sits *downstream* of this collector and is deliberately not referenced
//! anywhere here. When the collector meets the gate (R5-2-5-2), the gate is the
//! only place a channel is allowed or blocked.
//!
//! Two implementations exist:
//! - [`RealCommandRunner`] — spawns the OS capture commands (`grim`, `hyprctl`,
//!   `wl-paste`, `omarchy-*`). Used on the Omarchy/Linux target (R5-2-5-4
//!   real qualification). It is fully written here but **qualified on the
//!   target**, exactly as frozen in R5-2-5-0.
//! - [`FakeCommandRunner`] — deterministic scripted output for unit tests on a
//!   Windows dev host, proving the collector's command recipes and ordering
//!   without needing a Wayland session.

use std::collections::HashMap;
use std::process::Command;
use std::time::Duration;

/// Why a capture command could not run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunnerError {
    /// The command could not be spawned or completed cleanly (real OS failure).
    Io(String),
    /// The command exceeded its allowed time budget.
    Timeout(String),
}

impl std::fmt::Display for RunnerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RunnerError::Io(m) => write!(f, "{m}"),
            RunnerError::Timeout(m) => write!(f, "{m}"),
        }
    }
}

/// The output of a single OS capture command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandOutput {
    /// Captured stdout bytes (for image channels: PNG bytes).
    pub stdout: Vec<u8>,
    /// Captured stderr (for diagnostics only).
    pub stderr: Vec<u8>,
    /// Process exit code (`None` if terminated by a signal).
    pub status: Option<i32>,
}

impl CommandOutput {
    pub fn ok_binary(bytes: impl Into<Vec<u8>>) -> Self {
        Self {
            stdout: bytes.into(),
            stderr: Vec::new(),
            status: Some(0),
        }
    }

    pub fn ok_text(text: &str) -> Self {
        Self {
            stdout: text.as_bytes().to_vec(),
            stderr: Vec::new(),
            status: Some(0),
        }
    }

    pub fn failed(stderr: impl Into<String>, code: i32) -> Self {
        Self {
            stdout: Vec::new(),
            stderr: stderr.into().into_bytes(),
            status: Some(code),
        }
    }
}

/// The only seam through which the collector touches the OS.
///
/// A capture is one program + args that produces [`CommandOutput`]. Keeping this
/// as a trait means the collector itself can be proven deterministically against
/// a fake, and the real commands are qualified on the target later (R5-2-5-4).
pub trait CommandRunner: Send + Sync {
    fn run(&self, program: &str, args: &[String]) -> Result<CommandOutput, RunnerError>;
}

/// Real implementation: spawns `program` as a subprocess (the same pattern the
/// Copilot CLI provider already uses in `friday-intelligence`, sources
/// `providers.rs:719/827/4007`), with an optional timeout. On a Windows dev host
/// these tools are absent and calls fail loudly — tests must use
/// [`FakeCommandRunner`].
#[derive(Debug, Clone, Copy, Default)]
pub struct RealCommandRunner {
    pub timeout: Option<Duration>,
}

impl CommandRunner for RealCommandRunner {
    fn run(&self, program: &str, args: &[String]) -> Result<CommandOutput, RunnerError> {
        let program = program.to_string();
        let args = args.to_vec();
        let timeout = self.timeout;
        let name = program.clone();

        let run_once = move || {
            let child = Command::new(&program)
                .args(&args)
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .stdin(std::process::Stdio::null())
                .spawn()
                .map_err(|e| RunnerError::Io(format!("spawn '{program}': {e}")))?;

            let output = child
                .wait_with_output()
                .map_err(|e| RunnerError::Io(format!("wait '{program}': {e}")))?;

            Ok::<_, RunnerError>(CommandOutput {
                stdout: output.stdout,
                stderr: output.stderr,
                status: output.status.code(),
            })
        };

        match timeout {
            None => run_once(),
            Some(d) => {
                let (tx, rx) = std::sync::mpsc::channel();
                std::thread::spawn(move || {
                    let _ = tx.send(run_once());
                });
                Ok(rx
                    .recv_timeout(d)
                    .map_err(|_| RunnerError::Timeout(format!("'{name}' exceeded {d:?}")))?
                    .expect("spawn thread must send"))
            }
        }
    }
}

/// Deterministic test double. Scripts canned [`CommandOutput`] per exact
/// `(program, args)`; an unscripted call fails loudly so a collector that wires
/// the wrong command recipe is caught by the test, not silently mis-rendered.
#[derive(Debug, Clone, Default)]
pub struct FakeCommandRunner {
    scripted: HashMap<(String, Vec<String>), CommandOutput>,
}

impl FakeCommandRunner {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn script_stdout(
        mut self,
        program: &str,
        args: &[&str],
        stdout: impl Into<Vec<u8>>,
    ) -> Self {
        self.scripted
            .insert(key(program, args), CommandOutput::ok_binary(stdout));
        self
    }

    pub fn script_error(mut self, program: &str, args: &[&str], stderr: &str, code: i32) -> Self {
        self.scripted
            .insert(key(program, args), CommandOutput::failed(stderr, code));
        self
    }

    #[cfg(test)]
    fn is_scripted(&self, program: &str, args: &[&str]) -> bool {
        self.scripted.contains_key(&key(program, args))
    }
}

impl CommandRunner for FakeCommandRunner {
    fn run(&self, program: &str, args: &[String]) -> Result<CommandOutput, RunnerError> {
        let arg_slice: Vec<&str> = args.iter().map(String::as_str).collect();
        match self.scripted.get(&key(program, &arg_slice)) {
            Some(out) => Ok(out.clone()),
            None => Err(RunnerError::Io(format!(
                "unscripted capture command: '{program}' {} — don't run real OS capture in tests",
                arg_slice.join(" ")
            ))),
        }
    }
}

fn key(program: &str, args: &[&str]) -> (String, Vec<String>) {
    (
        program.to_string(),
        args.iter().map(|s| s.to_string()).collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fake_returns_scripted_output_for_exact_argv() {
        let fake = FakeCommandRunner::new().script_stdout(
            "grim",
            &["-g", "0,0 800x600", "-"],
            b"\x89PNG-fake",
        );
        let args = vec!["-g".to_string(), "0,0 800x600".to_string(), "-".to_string()];
        let out = fake.run("grim", &args).unwrap();
        assert_eq!(out.status, Some(0));
        assert_eq!(out.stdout, b"\x89PNG-fake");
    }

    #[test]
    fn fake_rejects_unscripted_calls_loudly() {
        let fake = FakeCommandRunner::new().script_stdout("grim", &["-"], []);
        let err = fake
            .run("hyprctl", &["-j".into(), "activewindow".into()])
            .unwrap_err();
        assert!(err.to_string().contains("unscripted"), "got: {err}");
    }

    #[test]
    fn fake_can_assert_a_command_was_scripted() {
        let fake =
            FakeCommandRunner::new().script_stdout("wl-paste", &["--no-newline"], "clipboard text");
        assert!(fake.is_scripted("wl-paste", &["--no-newline"]));
        assert!(!fake.is_scripted("wl-paste", &["--unexpected"]));
    }
}
