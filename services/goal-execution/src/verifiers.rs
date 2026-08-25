//! Concrete (non-mock) verifiers — the first real proof that FRIDAY's
//! verification architecture *interprets observed output* instead of trusting
//! it.
//!
//! Until now the only shipped verifiers were structural (`MockVerifier`): they
//! return a canned pass/fail and prove nothing about the actual execution.
//! This module is the **local code/test verifier first** — deliberately before
//! Sonar/security/evidence verifiers, because a local verifier proves the
//! architecture works with zero external services or configuration:
//!
//! ```text
//! CodeTestVerifier
//!       ↓
//! real command/test execution        (run_command_in → stdout/stderr/exit code)
//!       ↓
//! per-expectation checks[]           (exit code, expect markers, forbid markers)
//!       ↓
//! VerifiedResult                     (all checks pass → verified; else Failed)
//! ```
//!
//! **Two faces, one set of expectations:**
//! - [`CodeTestVerifier::verify_command`] is the executor: it runs a command
//!   in a workspace, captures the REAL stdout/stderr/exit code, and produces a
//!   [`VerifiedResult`] whose `checks` are inspectable.
//! - [`CodeTestVerifier::to_verification`] is the frozen-seam face: it
//!   registers the same expectations as concrete [`Verifier`]s so an already
//!   observed [`ToolResult`] (executed by the action runtime) is interpreted
//!   into the same inspectable `checks[]`.
//!
//! **Guardrails:**
//! - No new trait. The frozen [`Verifier`] seam (`(&Action, &ToolResult) →
//!   Check`) is the only contract; this module builds BEHIND it.
//! - The seam is synchronous, so real command execution here is deliberately
//!   blocking ([`run_command_in`]); the caller decides when to invoke it —
//!   never from inside an async tool loop. A hung command can never freeze
//!   the engine: every run is capped by [`DEFAULT_COMMAND_TIMEOUT`] and
//!   terminated (see [`run_command_in_with_timeout`]).
//! - A failed check is recorded `Failed`, never `Verified`
//!   (`ExecutionState::from_verified` is the single derivation rule).
//! - The exit-code check exists only on the executor face: the frozen seam
//!   receives an already-observed [`ToolResult`], which today carries no exit
//!   code — there `tool_success` means "the adapter reported success". The
//!   two faces therefore derive `tool_success` from different signals, both
//!   honest; markers/exit-code expectations are identical where both exist.

use std::io::Read;
use std::path::Path;
use std::process::{Command as StdCommand, Stdio};
use std::time::{Duration, Instant};

use crate::common::Action;
use crate::confidence::Confidence;
use crate::verification::{Check, ToolResult, Verification, VerifiedResult, Verifier};

/// Raw outcome of one real command run — the verifier's ground truth when it
/// is the executor (the "workspace + command → execute → stdout/stderr/exit
/// code" half of the plan).
#[derive(Debug, Clone)]
pub struct CommandObservation {
    pub exit_code: i32,
    pub stdout: String,
    pub stderr: String,
    pub duration_ms: u64,
}

/// Default cap for one verification command run. Long enough for real test
/// runs, short enough that a hung `cargo test` cannot freeze the engine.
pub const DEFAULT_COMMAND_TIMEOUT: Duration = Duration::from_secs(120);

/// Run one command in `workspace`, capturing stdout/stderr/exit code + wall
/// time, capped by [`DEFAULT_COMMAND_TIMEOUT`]. Blocking by design — the
/// frozen [`Verifier`] seam is synchronous, so the verifier never awaits; the
/// caller chooses when to run it.
pub fn run_command_in(workspace: &Path, command: &str) -> CommandObservation {
    run_command_in_with_timeout(workspace, command, DEFAULT_COMMAND_TIMEOUT)
}

/// [`run_command_in`] with an explicit cap: a command that does not exit
/// within `timeout` is killed and reported as a timed-out observation
/// (`exit_code -1`), never allowed to hang the caller. Polling is
/// thread-free — the child is spawned with piped stdio and reaped via
/// `try_wait`; on timeout it is killed and reaped before we return.
pub fn run_command_in_with_timeout(
    workspace: &Path,
    command: &str,
    timeout: Duration,
) -> CommandObservation {
    let started = Instant::now();
    let mut cmd = if cfg!(windows) {
        let mut c = StdCommand::new("cmd");
        c.arg("/C").arg(command);
        c
    } else {
        let mut c = StdCommand::new("sh");
        c.arg("-c").arg(command);
        c
    };
    cmd.current_dir(workspace)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            return CommandObservation {
                exit_code: -1,
                stdout: String::new(),
                stderr: format!("could not spawn command: {e}"),
                duration_ms: started.elapsed().as_millis() as u64,
            };
        }
    };
    let mut stdout = child.stdout.take();
    let mut stderr = child.stderr.take();
    let deadline = started + timeout;

    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                // Child reaped — the pipes are closed at exit, so draining them
                // now cannot block.
                let mut out = String::new();
                let mut err = String::new();
                if let Some(h) = stdout.as_mut() {
                    let _ = h.read_to_string(&mut out);
                }
                if let Some(h) = stderr.as_mut() {
                    let _ = h.read_to_string(&mut err);
                }
                return CommandObservation {
                    exit_code: status.code().unwrap_or(-1),
                    stdout: out,
                    stderr: err,
                    duration_ms: started.elapsed().as_millis() as u64,
                };
            }
            Ok(None) => {
                if Instant::now() >= deadline {
                    // Kill and reap so no orphan process outlives the call.
                    let _ = child.kill();
                    let _ = child.wait();
                    return CommandObservation {
                        exit_code: -1,
                        stdout: String::new(),
                        stderr: format!("command exceeded {timeout:?} timeout and was terminated"),
                        duration_ms: started.elapsed().as_millis() as u64,
                    };
                }
                std::thread::sleep(Duration::from_millis(20));
            }
            Err(_) => {
                return CommandObservation {
                    exit_code: -1,
                    stdout: String::new(),
                    stderr: "failed to wait for command".into(),
                    duration_ms: started.elapsed().as_millis() as u64,
                };
            }
        }
    }
}

/// Build one inspectable [`Check`] per expectation over a real command run:
/// exit code equals `expected_exit`, every `expect` marker present in the
/// combined output (stdout + stderr — a runner may print its summary to
/// either), every `forbid` marker absent from the combined output.
pub fn command_run_checks(
    obs: &CommandObservation,
    expected_exit: i32,
    expect: &[String],
    forbid: &[String],
) -> Vec<Check> {
    let combined = format!("{}\n{}", obs.stdout, obs.stderr);
    let mut checks = Vec::new();
    checks.push(Check {
        name: "exit_code".into(),
        passed: obs.exit_code == expected_exit,
        detail: format!("exit code {} (expected {expected_exit})", obs.exit_code),
    });
    for marker in expect {
        let passed = combined.contains(marker);
        checks.push(Check {
            name: format!("expect \"{marker}\""),
            passed,
            detail: if passed {
                format!("output contains \"{marker}\"")
            } else {
                format!("output does NOT contain \"{marker}\"")
            },
        });
    }
    for marker in forbid {
        let found = combined.contains(marker);
        checks.push(Check {
            name: format!("no \"{marker}\""),
            passed: !found,
            detail: if found {
                format!("output contains failure marker \"{marker}\"")
            } else {
                format!("output clean of \"{marker}\"")
            },
        });
    }
    checks
}

/// Concrete verifier: the observed tool output must contain `marker` (e.g.
/// `test result: ok`).
pub struct OutputContainsVerifier {
    marker: String,
}

impl OutputContainsVerifier {
    pub fn new(marker: impl Into<String>) -> Self {
        Self {
            marker: marker.into(),
        }
    }
}

impl Verifier for OutputContainsVerifier {
    fn name(&self) -> &str {
        "output_contains"
    }

    fn verify(&self, _action: &Action, result: &ToolResult) -> Check {
        let passed = result.output.contains(&self.marker);
        Check {
            name: format!("output contains \"{}\"", self.marker),
            passed,
            detail: if passed {
                "marker present".into()
            } else {
                "marker absent".into()
            },
        }
    }
}

/// Concrete verifier: the observed tool output must NOT contain `marker`
/// (failure markers such as `FAILED` / `panicked`).
pub struct OutputExcludesVerifier {
    marker: String,
}

impl OutputExcludesVerifier {
    pub fn new(marker: impl Into<String>) -> Self {
        Self {
            marker: marker.into(),
        }
    }
}

impl Verifier for OutputExcludesVerifier {
    fn name(&self) -> &str {
        "output_excludes"
    }

    fn verify(&self, _action: &Action, result: &ToolResult) -> Check {
        let passed = !result.output.contains(&self.marker);
        Check {
            name: format!("no \"{}\"", self.marker),
            passed,
            detail: if passed {
                "failure marker absent".into()
            } else {
                format!("failure marker \"{}\" present in output", self.marker)
            },
        }
    }
}

/// Concrete verifier: the tool itself reported success — the observed-level
/// exit signal (the terminal adapter merges stdout/stderr and surfaces
/// success at the action boundary; exit codes are not yet surfaced there).
pub struct ToolSuccessVerifier;

impl Verifier for ToolSuccessVerifier {
    fn name(&self) -> &str {
        "tool_success"
    }

    fn verify(&self, _action: &Action, result: &ToolResult) -> Check {
        Check {
            name: "tool_success".into(),
            passed: result.success,
            detail: result
                .error
                .clone()
                .unwrap_or_else(|| "tool reported success".into()),
        }
    }
}

/// The first real (non-mock) command/test verifier.
///
/// Defaults to the honest minimum: exit code 0 and no common failure markers
/// (`FAILED`, `panicked`). [`CodeTestVerifier::for_test_run`] adds the Rust
/// test-run expectation (`test result: ok`); markers are additive.
pub struct CodeTestVerifier {
    expected_exit: i32,
    expect: Vec<String>,
    forbid: Vec<String>,
}

impl Default for CodeTestVerifier {
    fn default() -> Self {
        Self::new()
    }
}

impl CodeTestVerifier {
    /// Exit code 0; forbid the common failure markers (`FAILED`, `panicked`).
    pub fn new() -> Self {
        Self {
            expected_exit: 0,
            expect: Vec::new(),
            forbid: vec!["FAILED".to_string(), "panicked".to_string()],
        }
    }

    /// A Rust test-run profile: additionally requires `test result: ok`.
    pub fn for_test_run() -> Self {
        let mut v = Self::new();
        v.expect.push("test result: ok".to_string());
        v
    }

    pub fn expecting(mut self, marker: impl Into<String>) -> Self {
        self.expect.push(marker.into());
        self
    }

    pub fn forbidding(mut self, marker: impl Into<String>) -> Self {
        self.forbid.push(marker.into());
        self
    }

    pub fn with_exit_code(mut self, code: i32) -> Self {
        self.expected_exit = code;
        self
    }

    /// The plan's flow: run `command` in `workspace`, capture the real
    /// stdout/stderr/exit code, and produce a [`VerifiedResult`] whose checks
    /// are inspectable. Blocking (the seam is sync) — call from a context
    /// that can tolerate a synchronous command run.
    pub fn verify_command(&self, workspace: &Path, command: &str) -> VerifiedResult {
        let obs = run_command_in(workspace, command);
        self.verify_observation(&obs)
    }

    /// Verify a captured command run.
    pub fn verify_observation(&self, obs: &CommandObservation) -> VerifiedResult {
        let checks = command_run_checks(obs, self.expected_exit, &self.expect, &self.forbid);
        verified_from_checks(checks, obs.exit_code == self.expected_exit)
    }

    /// The frozen-seam face: a [`Verification`] registry of the concrete
    /// sub-verifiers, so `Verification::verify(action, observed)` yields the
    /// same expectations over an already-executed [`ToolResult`].
    pub fn to_verification(&self) -> Verification {
        let mut v = Verification::new().register(Box::new(ToolSuccessVerifier));
        for m in &self.expect {
            v = v.register(Box::new(OutputContainsVerifier::new(m.clone())));
        }
        for m in &self.forbid {
            v = v.register(Box::new(OutputExcludesVerifier::new(m.clone())));
        }
        v
    }
}

/// Aggregate checks into a [`VerifiedResult`] (all-checks-must-pass; a
/// non-zero exit means `tool_success == false`, so it can never be
/// "verified").
fn verified_from_checks(checks: Vec<Check>, tool_success: bool) -> VerifiedResult {
    let total = checks.len();
    let passed = checks.iter().filter(|c| c.passed).count();
    let verified = tool_success && passed == total;
    let confidence = if total == 0 {
        Confidence::medium()
    } else {
        Confidence::from_ratio(passed as u32, total as u32)
    };
    VerifiedResult {
        verified,
        tool_success,
        checks,
        confidence,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::verification::ExecutionState;

    // ── Real command execution ───────────────────────────────────

    #[test]
    fn run_command_captures_stdout_and_exit_code() {
        let obs = run_command_in(std::env::temp_dir().as_path(), "echo friday-verifier-probe");
        assert_eq!(obs.exit_code, 0, "stderr: {}", obs.stderr);
        assert!(
            obs.stdout.contains("friday-verifier-probe"),
            "stdout: {}",
            obs.stdout
        );
    }

    #[test]
    fn run_command_honors_the_workspace_directory() {
        let dir = std::env::temp_dir().join(format!("friday-verifier-ws-{}", uuid_lite()));
        std::fs::create_dir_all(&dir).unwrap();
        // `cmd /C cd` (Windows) and `pwd` (unix) both print the cwd.
        let obs = run_command_in(&dir, if cfg!(windows) { "cd" } else { "pwd" });
        let tail = dir
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        assert!(
            obs.stdout.contains(&tail),
            "the workspace must be the cwd: {}",
            obs.stdout
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    // ── CodeTestVerifier over real runs ─────────────────────────

    #[test]
    fn verify_command_passes_a_clean_test_run_with_inspectable_checks() {
        let v = CodeTestVerifier::for_test_run();
        let obs = run_command_in(std::env::temp_dir().as_path(), "echo test result: ok");
        let result = v.verify_observation(&obs);
        assert!(result.verified, "checks: {:?}", result.checks);
        assert!(result.tool_success);
        // exit_code + expect("test result: ok") + two forbid markers.
        assert_eq!(result.checks.len(), 4);
        let names: Vec<&str> = result.checks.iter().map(|c| c.name.as_str()).collect();
        assert!(names.contains(&"exit_code"), "{names:?}");
        assert!(names.contains(&"expect \"test result: ok\""), "{names:?}");
        assert!(names.contains(&"no \"FAILED\""), "{names:?}");
        // The exit-code check records the real observed code.
        let exit = result
            .checks
            .iter()
            .find(|c| c.name == "exit_code")
            .unwrap();
        assert_eq!(exit.detail, "exit code 0 (expected 0)");
    }

    #[test]
    fn verify_command_rejects_a_failure_marker_in_the_output() {
        let v = CodeTestVerifier::for_test_run();
        let obs = run_command_in(std::env::temp_dir().as_path(), "echo test result: FAILED");
        let result = v.verify_observation(&obs);
        assert!(!result.verified, "checks: {:?}", result.checks);
        // The forbid check and the expect check both fail — inspectable.
        assert!(
            result
                .checks
                .iter()
                .filter(|c| !c.passed)
                .any(|c| c.name == "no \"FAILED\""),
            "the FAILED forbid check is recorded as failed: {:?}",
            result.checks
        );
        assert_eq!(
            crate::verification::ExecutionState::from_verified(&result),
            ExecutionState::Failed,
            "a failed check is Failed, never Verified"
        );
    }

    #[test]
    fn verify_command_records_a_non_zero_exit_as_unverified() {
        let v = CodeTestVerifier::new();
        let obs = run_command_in(std::env::temp_dir().as_path(), "exit 1");
        let result = v.verify_observation(&obs);
        assert_eq!(obs.exit_code, 1, "the command really failed");
        assert!(!result.tool_success);
        assert!(!result.verified);
        let exit = result
            .checks
            .iter()
            .find(|c| c.name == "exit_code")
            .unwrap();
        assert!(!exit.passed);
        assert!(exit.detail.contains("exit code 1"), "{}", exit.detail);
    }

    // ── The frozen-seam face over observed ToolResults ──────────

    #[test]
    fn concrete_verification_blocks_false_success_on_observed_output() {
        // The tool "succeeded" at the adapter boundary, but the OBSERVED output
        // carries a failure marker — verification must fail, and the lifecycle
        // must say Failed, never Verified. This is the constitution's
        // acceptance for a concrete verifier.
        let verification = CodeTestVerifier::new().to_verification();
        let action = Action::single("run cargo test", crate::Capability::ExecuteCommand, "g1");
        let result = ToolResult::success("test result: FAILED. 1 failed; 0 passed;");
        let verified = verification.verify(&action, &result);

        assert!(verified.tool_success, "the tool itself succeeded");
        assert!(!verified.verified, "the FAILED marker blocks success");
        assert_eq!(
            ExecutionState::from_verified(&verified),
            ExecutionState::Failed,
            "a failed check is recorded Failed, never Verified"
        );
        // The failing check is inspectable.
        let fail = verified
            .checks
            .iter()
            .find(|c| c.name == "no \"FAILED\"")
            .expect("the FAILED check is present");
        assert!(!fail.passed);
        assert!(
            fail.detail.contains("FAILED"),
            "detail names the marker: {}",
            fail.detail
        );
    }

    #[test]
    fn concrete_verification_passes_clean_observed_output() {
        let verification = CodeTestVerifier::for_test_run().to_verification();
        let action = Action::single("run cargo test", crate::Capability::ExecuteCommand, "g1");
        let result =
            ToolResult::success("running 2 tests ... test result: ok. 2 passed; 0 failed;");
        let verified = verification.verify(&action, &result);

        assert!(verified.verified, "checks: {:?}", verified.checks);
        assert_eq!(
            ExecutionState::from_verified(&verified),
            ExecutionState::Verified
        );
        assert!(verified.checks.iter().all(|c| c.passed));
    }

    #[test]
    fn default_verifier_forbids_common_failure_markers() {
        let verification = CodeTestVerifier::new().to_verification();
        let action = Action::single("run", crate::Capability::ExecuteCommand, "g1");
        // A panicked run (common across languages) must not be verified.
        let result = ToolResult::success("thread 'main' panicked at src/main.rs:10");
        let verified = verification.verify(&action, &result);
        assert!(!verified.verified);
        assert!(verified
            .checks
            .iter()
            .any(|c| c.name == "no \"panicked\"" && !c.passed));
    }

    #[test]
    fn a_failed_tool_is_never_verified() {
        let verification = CodeTestVerifier::new().to_verification();
        let action = Action::single("run cargo test", crate::Capability::ExecuteCommand, "g1");
        let result = ToolResult::error("linker error");
        let verified = verification.verify(&action, &result);
        assert!(!verified.verified);
        assert!(!verified.tool_success);
        assert_eq!(
            ExecutionState::from_verified(&verified),
            ExecutionState::Failed
        );
    }

    // ── Stream scanning ─────────────────────────────────────────

    #[test]
    fn expect_marker_on_stderr_is_found() {
        let v = CodeTestVerifier::new().expecting("summary-line");
        let obs = CommandObservation {
            exit_code: 0,
            stdout: "compiling...".into(),
            stderr: "summary-line: all good".into(),
            duration_ms: 1,
        };
        let result = v.verify_observation(&obs);
        assert!(result.verified, "checks: {:?}", result.checks);
        assert!(
            result
                .checks
                .iter()
                .any(|c| c.name == "expect \"summary-line\"" && c.passed),
            "the expect check must scan stderr too: {:?}",
            result.checks
        );
    }

    #[test]
    fn forbid_marker_on_stderr_blocks_verification() {
        let v = CodeTestVerifier::new();
        let obs = CommandObservation {
            exit_code: 0,
            stdout: "done".into(),
            stderr: "thread panicked at src/main.rs:10".into(),
            duration_ms: 1,
        };
        let result = v.verify_observation(&obs);
        assert!(!result.verified);
        assert!(
            result
                .checks
                .iter()
                .any(|c| c.name == "no \"panicked\"" && !c.passed),
            "the forbid check must scan stderr too: {:?}",
            result.checks
        );
    }

    // ── Timeout ─────────────────────────────────────────────────

    #[test]
    fn a_hung_command_is_terminated_at_the_timeout() {
        let started = Instant::now();
        // Long enough to hang, short enough that the test stays fast.
        let hang = if cfg!(windows) {
            "ping -n 30 127.0.0.1 >nul"
        } else {
            "sleep 30"
        };
        let obs = run_command_in_with_timeout(
            std::env::temp_dir().as_path(),
            hang,
            Duration::from_millis(300),
        );
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "a hung command must return promptly, took {:?}",
            started.elapsed()
        );
        assert_eq!(obs.exit_code, -1);
        assert!(
            obs.stderr.contains("timeout"),
            "the timeout is reported: {}",
            obs.stderr
        );
    }

    // ── Helpers ─────────────────────────────────────────────────

    fn uuid_lite() -> u64 {
        use std::time::{SystemTime, UNIX_EPOCH};
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .subsec_nanos() as u64
    }
}
