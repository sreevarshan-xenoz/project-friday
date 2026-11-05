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
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, SystemTime};

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
                .envs(session_env())
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
                // The worker always sends run_once's own Result; a failure to
                // start the program (e.g. hyprctl not installed) is an Err to
                // return, never a panic: it took the whole goal down with it.
                rx.recv_timeout(d)
                    .map_err(|_| RunnerError::Timeout(format!("'{name}' exceeded {d:?}")))?
            }
        }
    }
}

/// The Wayland/Hyprland session variables an OS command needs, for any the
/// current process lacks or holds stale values for.
///
/// The FRIDAY daemon is a systemd user service and can start before the
/// compositor (or outlive a session), so its own environment may not name the
/// live session. `grim`, `wl-paste`, `wtype` and `hyprctl` all fail without it;
/// the session is rediscovered from `$XDG_RUNTIME_DIR` on every call instead.
pub fn session_env() -> Vec<(String, String)> {
    let runtime_dir = std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from);
    discover_session_env(runtime_dir.as_deref(), |k| std::env::var(k).ok())
}

/// [`session_env`] against an explicit runtime dir and environment lookup.
pub fn discover_session_env(
    runtime_dir: Option<&Path>,
    current: impl Fn(&str) -> Option<String>,
) -> Vec<(String, String)> {
    let Some(dir) = runtime_dir else {
        return Vec::new();
    };
    let mut env = Vec::new();

    let hypr_dir = dir.join("hypr");
    let live_signature = current("HYPRLAND_INSTANCE_SIGNATURE")
        .filter(|sig| hypr_dir.join(sig).join(".socket.sock").exists());
    if live_signature.is_none() {
        if let Some(sig) = newest_hyprland_instance(&hypr_dir) {
            env.push(("HYPRLAND_INSTANCE_SIGNATURE".to_string(), sig));
        }
    }

    let live_display = current("WAYLAND_DISPLAY").filter(|d| dir.join(d).exists());
    if live_display.is_none() {
        if let Some(display) = first_wayland_display(dir) {
            env.push(("WAYLAND_DISPLAY".to_string(), display));
        }
    }
    env
}

/// The Hyprland instance whose control socket was touched most recently —
/// crashed sessions can leave stale instance directories behind.
fn newest_hyprland_instance(hypr_dir: &Path) -> Option<String> {
    std::fs::read_dir(hypr_dir)
        .ok()?
        .flatten()
        .filter_map(|entry| {
            let socket = entry.path().join(".socket.sock");
            let modified = socket
                .metadata()
                .ok()?
                .modified()
                .unwrap_or(SystemTime::UNIX_EPOCH);
            Some((modified, entry.file_name().to_string_lossy().into_owned()))
        })
        .max()
        .map(|(_, sig)| sig)
}

/// The lowest-numbered `wayland-N` socket in the runtime dir.
fn first_wayland_display(dir: &Path) -> Option<String> {
    let mut displays: Vec<String> = std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .filter(|name| name.starts_with("wayland-") && !name.ends_with(".lock"))
        .collect();
    displays.sort();
    displays.into_iter().next()
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

    /// A program that isn't installed (hyprctl outside Hyprland) is an error
    /// the caller handles, with or without a timeout: it used to panic in the
    /// timeout path, which killed the goal and left every ask hanging.
    #[test]
    fn missing_program_is_an_error_not_a_panic() {
        for timeout in [None, Some(Duration::from_secs(5))] {
            let runner = RealCommandRunner { timeout };
            let err = runner
                .run("friday-test-no-such-program", &[])
                .expect_err("a missing program must be an Err");
            assert!(
                matches!(err, RunnerError::Io(_)),
                "timeout={timeout:?}: {err:?}"
            );
        }
    }

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

    /// A throwaway `$XDG_RUNTIME_DIR` with the given hypr instances (those in
    /// `live` get a control socket) and plain files.
    fn runtime_dir(name: &str, instances: &[&str], live: &[&str], files: &[&str]) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("friday-session-env-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        for sig in instances {
            std::fs::create_dir_all(dir.join("hypr").join(sig)).unwrap();
        }
        for sig in live {
            std::fs::write(dir.join("hypr").join(sig).join(".socket.sock"), b"").unwrap();
        }
        for file in files {
            std::fs::write(dir.join(file), b"").unwrap();
        }
        dir
    }

    #[test]
    fn session_env_discovers_a_session_the_process_does_not_know() {
        let dir = runtime_dir(
            "discover",
            &["stale", "live"],
            &["live"],
            &["wayland-1", "wayland-1.lock"],
        );
        let env = discover_session_env(Some(&dir), |_| None);
        assert_eq!(
            env,
            vec![
                (
                    "HYPRLAND_INSTANCE_SIGNATURE".to_string(),
                    "live".to_string()
                ),
                ("WAYLAND_DISPLAY".to_string(), "wayland-1".to_string()),
            ]
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn session_env_keeps_live_values_and_replaces_stale_ones() {
        let dir = runtime_dir("stale", &["old", "new"], &["new"], &["wayland-0"]);
        // A live signature + display already in the environment: nothing to add.
        let current = |k: &str| match k {
            "HYPRLAND_INSTANCE_SIGNATURE" => Some("new".to_string()),
            "WAYLAND_DISPLAY" => Some("wayland-0".to_string()),
            _ => None,
        };
        assert!(discover_session_env(Some(&dir), current).is_empty());

        // Values from a session that has since ended are replaced.
        let stale = |k: &str| match k {
            "HYPRLAND_INSTANCE_SIGNATURE" => Some("old".to_string()),
            "WAYLAND_DISPLAY" => Some("wayland-9".to_string()),
            _ => None,
        };
        let env = discover_session_env(Some(&dir), stale);
        assert_eq!(
            env,
            vec![
                ("HYPRLAND_INSTANCE_SIGNATURE".to_string(), "new".to_string()),
                ("WAYLAND_DISPLAY".to_string(), "wayland-0".to_string()),
            ]
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn session_env_is_empty_without_a_runtime_dir_or_session() {
        assert!(discover_session_env(None, |_| None).is_empty());
        let dir = runtime_dir("none", &[], &[], &[]);
        assert!(discover_session_env(Some(&dir), |_| None).is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn fake_can_assert_a_command_was_scripted() {
        let fake =
            FakeCommandRunner::new().script_stdout("wl-paste", &["--no-newline"], "clipboard text");
        assert!(fake.is_scripted("wl-paste", &["--no-newline"]));
        assert!(!fake.is_scripted("wl-paste", &["--unexpected"]));
    }
}
