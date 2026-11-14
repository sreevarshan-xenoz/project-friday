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
/// Default timeout applied to commands run via [`RealCommandRunner`].
pub const DEFAULT_RUNNER_TIMEOUT: Duration = Duration::from_secs(10);

/// Real implementation: spawns `program` as a subprocess (the same pattern the
/// Copilot CLI provider already uses in `friday-intelligence`, sources
/// `providers.rs:719/827/4007`), with a timeout (default 10 s). On a Windows dev host
/// these tools are absent and calls fail loudly — tests must use
/// [`FakeCommandRunner`].
#[derive(Debug, Clone, Copy)]
pub struct RealCommandRunner {
    pub timeout: Option<Duration>,
}

impl Default for RealCommandRunner {
    fn default() -> Self {
        Self {
            timeout: Some(DEFAULT_RUNNER_TIMEOUT),
        }
    }
}

impl CommandRunner for RealCommandRunner {
    fn run(&self, program: &str, args: &[String]) -> Result<CommandOutput, RunnerError> {
        let timeout = self.timeout.unwrap_or(DEFAULT_RUNNER_TIMEOUT);
        let name = program.to_string();

        let mut cmd = Command::new(program);
        cmd.args(args)
            .envs(session_env())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .stdin(std::process::Stdio::null());

        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            cmd.process_group(0);
        }

        let mut child = cmd
            .spawn()
            .map_err(|e| RunnerError::Io(format!("spawn '{program}': {e}")))?;

        let stdout = child.stdout.take();
        let stderr = child.stderr.take();

        let stdout_thread = std::thread::spawn(move || {
            let mut out = Vec::new();
            if let Some(mut r) = stdout {
                use std::io::Read;
                let _ = r.read_to_end(&mut out);
            }
            out
        });

        let stderr_thread = std::thread::spawn(move || {
            let mut err = Vec::new();
            if let Some(mut r) = stderr {
                use std::io::Read;
                let _ = r.read_to_end(&mut err);
            }
            err
        });

        let start = std::time::Instant::now();
        let mut poll_interval = Duration::from_millis(5);

        loop {
            match child.try_wait() {
                Ok(Some(status)) => {
                    let stdout = stdout_thread.join().unwrap_or_default();
                    let stderr = stderr_thread.join().unwrap_or_default();
                    return Ok(CommandOutput {
                        stdout,
                        stderr,
                        status: status.code(),
                    });
                }
                Ok(None) => {}
                Err(e) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    let _ = stdout_thread.join();
                    let _ = stderr_thread.join();
                    return Err(RunnerError::Io(format!("wait '{program}': {e}")));
                }
            }

            if start.elapsed() >= timeout {
                #[cfg(unix)]
                {
                    let pid = child.id();
                    unsafe {
                        libc::kill(-(pid as libc::pid_t), libc::SIGKILL);
                    }
                }
                #[cfg(windows)]
                {
                    let _ = std::process::Command::new("taskkill")
                        .args(["/PID", &child.id().to_string(), "/T", "/F"])
                        .status();
                }
                let _ = child.kill();
                let _ = child.wait();
                let _ = stdout_thread.join();
                let _ = stderr_thread.join();
                return Err(RunnerError::Timeout(format!(
                    "'{name}' exceeded {timeout:?}"
                )));
            }

            let remaining = timeout.saturating_sub(start.elapsed());
            let sleep_dur = poll_interval.min(remaining);
            if sleep_dur > Duration::ZERO {
                std::thread::sleep(sleep_dur);
            }
            poll_interval = (poll_interval * 2).min(Duration::from_millis(50));
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

    let live_niri = current("NIRI_SOCKET").filter(|s| Path::new(s).exists());
    if live_niri.is_none() {
        if let Some(socket) = newest_niri_socket(dir) {
            env.push(("NIRI_SOCKET".to_string(), socket));
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

/// The niri IPC socket (`niri.<display>.<pid>.sock`) touched most recently.
fn newest_niri_socket(dir: &Path) -> Option<String> {
    std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .filter(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            name.starts_with("niri.") && name.ends_with(".sock")
        })
        .filter_map(|entry| {
            let modified = entry
                .metadata()
                .ok()?
                .modified()
                .unwrap_or(SystemTime::UNIX_EPOCH);
            Some((modified, entry.path().display().to_string()))
        })
        .max()
        .map(|(_, path)| path)
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
    #[cfg(unix)]
    fn real_command_runner_default_has_timeout() {
        let runner = RealCommandRunner::default();
        assert_eq!(runner.timeout, Some(Duration::from_secs(10)));
    }

    #[test]
    #[cfg(unix)]
    fn real_command_runner_times_out_and_kills_child() {
        let runner = RealCommandRunner {
            timeout: Some(Duration::from_millis(100)),
        };
        let start = std::time::Instant::now();
        let err = runner
            .run("sleep", &["30".to_string()])
            .expect_err("sleep 30 should time out");
        let elapsed = start.elapsed();
        assert!(
            matches!(err, RunnerError::Timeout(_)),
            "expected Timeout error, got {err:?}"
        );
        assert!(
            elapsed < Duration::from_secs(3),
            "command should time out quickly, took {elapsed:?}"
        );
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

    /// niri's socket (`niri.<display>.<pid>.sock`) is found the same way, so
    /// `niri msg` works from a daemon started without NIRI_SOCKET.
    #[test]
    fn session_env_discovers_niri() {
        let dir = runtime_dir(
            "niri",
            &["ended"],
            &[],
            &["wayland-1", "niri.wayland-1.4242.sock"],
        );
        let env = discover_session_env(Some(&dir), |_| None);
        let sock = dir.join("niri.wayland-1.4242.sock").display().to_string();
        assert!(
            env.contains(&("NIRI_SOCKET".to_string(), sock.clone())),
            "{env:?}"
        );
        // A live NIRI_SOCKET is kept.
        let live = |k: &str| (k == "NIRI_SOCKET").then(|| sock.clone());
        assert!(!discover_session_env(Some(&dir), live)
            .iter()
            .any(|(k, _)| k == "NIRI_SOCKET"));
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
