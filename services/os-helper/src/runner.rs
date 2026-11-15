//! Package manager process runner.
//!
//! Spawns `pacman` with strictly fixed argument vectors, enforced timeouts,
//! piped output streams, and `LC_ALL=C`. Never invokes a shell.

use async_trait::async_trait;
use std::time::Duration;

/// Absolute, so a changed PATH can never point this root service elsewhere.
pub const PACMAN: &str = "/usr/bin/pacman";

/// Timeout for full system updates (30 minutes).
pub const TIMEOUT_UPDATE: Duration = Duration::from_secs(30 * 60);

/// Timeout for installing packages (10 minutes).
pub const TIMEOUT_INSTALL: Duration = Duration::from_secs(10 * 60);

/// Timeout for removing packages (10 minutes).
pub const TIMEOUT_REMOVE: Duration = Duration::from_secs(10 * 60);

/// Timeout for repo existence check via `pacman -Si` (30 seconds).
pub const TIMEOUT_QUERY: Duration = Duration::from_secs(30);

/// Constructs the fixed argument vector for system update: `["-Syu", "--noconfirm"]`.
pub fn update_argv() -> Vec<String> {
    vec!["-Syu".to_string(), "--noconfirm".to_string()]
}

/// Constructs the fixed argument vector for package installation:
/// `["-S", "--needed", "--noconfirm", "--", <NAME>]`.
pub fn install_argv(name: &str) -> Vec<String> {
    vec![
        "-S".to_string(),
        "--needed".to_string(),
        "--noconfirm".to_string(),
        "--".to_string(),
        name.to_string(),
    ]
}

/// Constructs the fixed argument vector for package removal:
/// `["-R", "--noconfirm", "--", <NAME>]`.
///
/// Note: Never uses `-Rdd` or `-Rns` cascade removal.
pub fn remove_argv(name: &str) -> Vec<String> {
    vec![
        "-R".to_string(),
        "--noconfirm".to_string(),
        "--".to_string(),
        name.to_string(),
    ]
}

/// Constructs the fixed argument vector for verifying package existence in the repositories:
/// `["-Si", "--", <NAME>]`.
pub fn check_pkg_argv(name: &str) -> Vec<String> {
    vec!["-Si".to_string(), "--".to_string(), name.to_string()]
}

/// Trait abstracting package manager process execution.
#[async_trait]
pub trait Runner: Send + Sync {
    /// Executes pacman with the specified fixed arguments and timeout.
    ///
    /// Streams lines to `on_line` as they are read from stdout/stderr.
    /// Returns the process exit code, or an error if spawning/waiting failed or timed out.
    async fn run(
        &self,
        args: &[String],
        timeout: Duration,
        on_line: &mut (dyn for<'a> FnMut(&'a str) + Send),
    ) -> Result<i32, String>;
}

/// Real implementation executing `/usr/bin/pacman`.
pub struct SystemRunner;

#[async_trait]
impl Runner for SystemRunner {
    async fn run(
        &self,
        args: &[String],
        timeout: Duration,
        on_line: &mut (dyn for<'a> FnMut(&'a str) + Send),
    ) -> Result<i32, String> {
        let mut cmd = tokio::process::Command::new(PACMAN);
        cmd.args(args)
            .env("LC_ALL", "C")
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());

        let mut child = cmd
            .spawn()
            .map_err(|e| format!("failed to spawn pacman: {e}"))?;

        let stdout = child.stdout.take();
        let stderr = child.stderr.take();

        let run_future = async {
            use tokio::io::{AsyncBufReadExt, BufReader};
            let mut stdout_lines = stdout.map(|s| BufReader::new(s).lines());
            let mut stderr_lines = stderr.map(|s| BufReader::new(s).lines());

            loop {
                tokio::select! {
                    res = async {
                        match &mut stdout_lines {
                            Some(lines) => lines.next_line().await,
                            None => std::future::pending().await,
                        }
                    } => {
                        match res {
                            Ok(Some(line)) => on_line(&line),
                            Ok(None) => stdout_lines = None,
                            Err(_) => stdout_lines = None,
                        }
                    }
                    res = async {
                        match &mut stderr_lines {
                            Some(lines) => lines.next_line().await,
                            None => std::future::pending().await,
                        }
                    } => {
                        match res {
                            Ok(Some(line)) => on_line(&line),
                            Ok(None) => stderr_lines = None,
                            Err(_) => stderr_lines = None,
                        }
                    }
                    status_res = child.wait() => {
                        // Drain any remaining lines from buffers
                        if let Some(mut lines) = stdout_lines {
                            while let Ok(Some(line)) = lines.next_line().await {
                                on_line(&line);
                            }
                        }
                        if let Some(mut lines) = stderr_lines {
                            while let Ok(Some(line)) = lines.next_line().await {
                                on_line(&line);
                            }
                        }
                        let status = status_res.map_err(|e| format!("child wait error: {e}"))?;
                        return Ok(status.code().unwrap_or(-1));
                    }
                }
            }
        };

        match tokio::time::timeout(timeout, run_future).await {
            Ok(result) => result,
            Err(_) => {
                let _ = child.kill().await;
                Err(format!("command timed out after {}s", timeout.as_secs()))
            }
        }
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};
    use tokio::sync::oneshot;

    /// Stub runner recording command invocations and simulating execution.
    pub struct StubRunner {
        calls: Mutex<Vec<Vec<String>>>,
        available_packages: Vec<String>,
        simulate_timeout: bool,
        simulate_failure: bool,
        pause_receiver: Mutex<Option<oneshot::Receiver<()>>>,
        output_lines: Vec<String>,
    }

    impl StubRunner {
        pub fn new(available_packages: &[&str]) -> Arc<Self> {
            Arc::new(Self {
                calls: Mutex::new(Vec::new()),
                available_packages: available_packages.iter().map(|s| s.to_string()).collect(),
                simulate_timeout: false,
                simulate_failure: false,
                pause_receiver: Mutex::new(None),
                output_lines: Vec::new(),
            })
        }

        pub fn with_pause(available_packages: &[&str], rx: oneshot::Receiver<()>) -> Arc<Self> {
            Arc::new(Self {
                calls: Mutex::new(Vec::new()),
                available_packages: available_packages.iter().map(|s| s.to_string()).collect(),
                simulate_timeout: false,
                simulate_failure: false,
                pause_receiver: Mutex::new(Some(rx)),
                output_lines: Vec::new(),
            })
        }

        pub fn timing_out() -> Arc<Self> {
            Arc::new(Self {
                calls: Mutex::new(Vec::new()),
                available_packages: Vec::new(),
                simulate_timeout: true,
                simulate_failure: false,
                pause_receiver: Mutex::new(None),
                output_lines: Vec::new(),
            })
        }

        pub fn with_output(available_packages: &[&str], lines: Vec<String>) -> Arc<Self> {
            Arc::new(Self {
                calls: Mutex::new(Vec::new()),
                available_packages: available_packages.iter().map(|s| s.to_string()).collect(),
                simulate_timeout: false,
                simulate_failure: false,
                pause_receiver: Mutex::new(None),
                output_lines: lines,
            })
        }

        pub fn calls(&self) -> Vec<Vec<String>> {
            self.calls.lock().unwrap().clone()
        }

        pub fn call_count(&self) -> usize {
            self.calls.lock().unwrap().len()
        }
    }

    #[async_trait]
    impl Runner for StubRunner {
        async fn run(
            &self,
            args: &[String],
            timeout: Duration,
            on_line: &mut (dyn for<'a> FnMut(&'a str) + Send),
        ) -> Result<i32, String> {
            self.calls.lock().unwrap().push(args.to_vec());

            // Handle pause if configured
            let rx = self.pause_receiver.lock().unwrap().take();
            if let Some(rx) = rx {
                let _ = rx.await;
            }

            if self.simulate_timeout {
                return Err(format!("command timed out after {}s", timeout.as_secs()));
            }

            for line in &self.output_lines {
                on_line(line);
            }

            if args.contains(&"-Si".to_string()) {
                if let Some(pkg) = args.last() {
                    if self.available_packages.contains(pkg) {
                        return Ok(0);
                    } else {
                        return Ok(1); // not in repository
                    }
                }
            }

            if self.simulate_failure {
                return Ok(1);
            }

            Ok(0)
        }
    }

    #[test]
    fn test_fixed_argv_correctness() {
        assert_eq!(update_argv(), vec!["-Syu", "--noconfirm"]);

        let inst = install_argv("htop");
        assert_eq!(inst, vec!["-S", "--needed", "--noconfirm", "--", "htop"]);

        let rem = remove_argv("htop");
        assert_eq!(rem, vec!["-R", "--noconfirm", "--", "htop"]);
        assert!(!rem.contains(&"-Rdd".to_string()));
        assert!(!rem.contains(&"-Rns".to_string()));

        let check = check_pkg_argv("htop");
        assert_eq!(check, vec!["-Si", "--", "htop"]);
    }
}
