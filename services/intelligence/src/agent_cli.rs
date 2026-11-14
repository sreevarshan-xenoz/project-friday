//! Answering with an AI account the person already has: Claude Code, Codex,
//! Antigravity (`agy`) or Cline, through that tool's own CLI and sign-in.
//!
//! FRIDAY holds no credential: the CLI owns its login (`friday connect`
//! runs it in the desktop session, since signing in needs a browser). Each
//! answer is one headless run with no tools, in a fresh empty directory with
//! stdin closed, so the tool can only reply — never read or change files.
//! These providers are text-only: a screenshot never goes to a CLI.
//!
//! Opt-in only: `FRIDAY_AGENT_CLIS=claude,cline` (written by `friday
//! connect`) lists the connected ones; one whose binary isn't on `PATH` is
//! skipped.

use async_trait::async_trait;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use crate::providers::{sanitize_candidate_error, Provider};

/// How long one headless run may take: the CLI starts a whole app before it
/// answers, far slower than an HTTP call.
pub const AGENT_CLI_TIMEOUT: Duration = Duration::from_secs(90);

/// Where a connected account ranks (the fabric tries lower latency first).
/// By default after the free models (OpenCode Zen 3000 ms, Kilo 3500 ms):
/// a connected tool starts a whole app per answer (seconds slower) and every
/// ask counts against the person's plan, so it is a fallback. With
/// `FRIDAY_PREFER_ACCOUNTS=1` it comes first, still after the person's own
/// API keys (800 ms).
pub fn account_latency_ms(prefer: bool) -> u64 {
    if prefer {
        1_500
    } else {
        4_000
    }
}

/// Whether the person asked for connected accounts ahead of the free models.
pub fn prefer_accounts() -> bool {
    std::env::var("FRIDAY_PREFER_ACCOUNTS").is_ok_and(|v| v == "1" || v == "true")
}

/// The account tools FRIDAY can answer through.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentCli {
    Claude,
    Codex,
    Antigravity,
    Cline,
}

impl AgentCli {
    pub const ALL: [AgentCli; 4] = [
        AgentCli::Claude,
        AgentCli::Codex,
        AgentCli::Antigravity,
        AgentCli::Cline,
    ];

    /// The id `friday connect` and `FRIDAY_AGENT_CLIS` use.
    pub fn id(self) -> &'static str {
        match self {
            AgentCli::Claude => "claude",
            AgentCli::Codex => "codex",
            AgentCli::Antigravity => "agy",
            AgentCli::Cline => "cline",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|c| c.id() == id.trim())
    }

    /// The provider name in routing decisions and errors.
    pub fn provider_name(self) -> &'static str {
        match self {
            AgentCli::Claude => "claude-code",
            AgentCli::Codex => "codex",
            AgentCli::Antigravity => "antigravity",
            AgentCli::Cline => "cline",
        }
    }

    pub fn binary(self) -> &'static str {
        self.id()
    }

    /// One headless answer with no tools (the same flags `friday connect`
    /// checks the sign-in with).
    pub fn args(self, prompt: &str) -> Vec<String> {
        let p = prompt.to_string();
        let v: Vec<&str> = match self {
            AgentCli::Claude => vec!["-p", &p, "--output-format", "text"],
            AgentCli::Codex => vec![
                "exec",
                "--skip-git-repo-check",
                "--sandbox",
                "read-only",
                &p,
            ],
            AgentCli::Antigravity => vec!["-p", &p, "--mode", "plan", "--effort", "low"],
            AgentCli::Cline => vec!["-p", "--auto-approve", "false", &p],
        };
        v.into_iter().map(String::from).collect()
    }
}

/// The connected tools listed in `FRIDAY_AGENT_CLIS` (comma-separated ids),
/// in order, without duplicates, keeping only those `installed` says exist.
pub fn agent_clis_from(value: &str, installed: impl Fn(&str) -> bool) -> Vec<AgentCli> {
    let mut out = Vec::new();
    for cli in value.split(',').filter_map(AgentCli::from_id) {
        if !out.contains(&cli) && installed(cli.binary()) {
            out.push(cli);
        }
    }
    out
}

/// Whether `binary` is an executable file on `PATH`.
pub fn on_path(binary: &str) -> bool {
    let Some(path) = std::env::var_os("PATH") else {
        return false;
    };
    std::env::split_paths(&path).any(|dir| {
        let candidate = dir.join(binary);
        if cfg!(windows) {
            // Only a native .exe: the question goes in as an argument, which
            // Windows can't hand safely to an npm .cmd/.bat shim.
            candidate.with_extension("exe").is_file()
        } else {
            is_executable(&candidate)
        }
    })
}

#[cfg(unix)]
fn is_executable(p: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    p.metadata()
        .map(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

#[cfg(not(unix))]
fn is_executable(p: &Path) -> bool {
    p.is_file()
}

/// A connected account tool as a fabric provider.
pub struct AgentCliProvider {
    pub cli: AgentCli,
    /// The binary to run (the tool's own on `PATH`; a test shim in tests).
    pub binary: String,
}

impl AgentCliProvider {
    pub fn new(cli: AgentCli) -> Self {
        Self {
            cli,
            binary: cli.binary().to_string(),
        }
    }

    async fn run(&self, prompt: &str) -> Result<String, String> {
        static RUN: AtomicU64 = AtomicU64::new(0);
        let dir = std::env::temp_dir().join(format!(
            "friday-{}-{}-{}",
            self.cli.id(),
            std::process::id(),
            RUN.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", self.cli.provider_name()))?;
        let out = tokio::process::Command::new(&self.binary)
            .args(self.cli.args(prompt))
            .current_dir(&dir)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true)
            .output()
            .await;
        let _ = std::fs::remove_dir_all(&dir);
        let name = self.cli.provider_name();
        let out = out.map_err(|e| format!("{name} isn't available: {e}"))?;
        let answer = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if out.status.success() && !answer.is_empty() {
            return Ok(answer);
        }
        let stderr: String = String::from_utf8_lossy(&out.stderr)
            .trim()
            .chars()
            .take(300)
            .collect();
        Err(sanitize_candidate_error(&if out.status.success() {
            format!("{name} gave no answer")
        } else if stderr.is_empty() {
            format!("{name} failed (exit {:?})", out.status.code())
        } else {
            format!("{name} failed: {stderr}")
        }))
    }
}

#[async_trait]
impl Provider for AgentCliProvider {
    fn name(&self) -> &str {
        self.cli.provider_name()
    }

    async fn complete(
        &self,
        _model_id: &str,
        prompt: &str,
        _require_json: bool,
    ) -> Result<String, String> {
        self.run(prompt).await
    }

    fn min_request_timeout(&self) -> Option<Duration> {
        Some(AGENT_CLI_TIMEOUT)
    }

    async fn check_health(&self) -> bool {
        on_path(&self.binary)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_round_trip_and_unknown_ids_are_ignored() {
        for cli in AgentCli::ALL {
            assert_eq!(AgentCli::from_id(cli.id()), Some(cli));
        }
        assert_eq!(AgentCli::from_id("bard"), None);
    }

    #[test]
    fn the_list_keeps_order_drops_duplicates_unknowns_and_missing_tools() {
        let installed = |b: &str| b != "codex";
        assert_eq!(
            agent_clis_from("cline, claude,cline,bard,codex,agy", installed),
            vec![AgentCli::Cline, AgentCli::Claude, AgentCli::Antigravity]
        );
        assert!(agent_clis_from("", |_| true).is_empty());
    }

    #[test]
    fn every_tool_is_asked_headless_with_the_prompt_as_one_argument() {
        let prompt = "why does \"$(rm -rf ~)\" fail?";
        for cli in AgentCli::ALL {
            let args = cli.args(prompt);
            assert_eq!(
                args.iter().filter(|a| a.as_str() == prompt).count(),
                1,
                "{cli:?}: {args:?}"
            );
        }
        assert_eq!(
            AgentCli::Claude.args("q"),
            ["-p", "q", "--output-format", "text"]
        );
        assert_eq!(
            AgentCli::Antigravity.args("q"),
            ["-p", "q", "--mode", "plan", "--effort", "low"]
        );
        assert_eq!(
            AgentCli::Cline.args("q"),
            ["-p", "--auto-approve", "false", "q"]
        );
        assert_eq!(
            AgentCli::Codex.args("q"),
            [
                "exec",
                "--skip-git-repo-check",
                "--sandbox",
                "read-only",
                "q"
            ]
        );
    }

    #[cfg(unix)]
    fn shim(name: &str, body: &str) -> String {
        use std::os::unix::fs::PermissionsExt;
        let dir =
            std::env::temp_dir().join(format!("friday-agent-shim-{}-{name}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(name);
        std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        path.display().to_string()
    }

    /// The answer comes from stdout; the run happens in an empty directory
    /// that is gone afterwards, with stdin closed.
    #[cfg(unix)]
    #[tokio::test]
    async fn answers_from_an_empty_directory_with_no_input() {
        let binary = shim(
            "answer",
            r#"[ -z "$(ls -A .)" ] || { echo "cwd not empty" >&2; exit 2; }
pwd > "$TMPDIR_MARK"
read -r line && { echo "stdin was open" >&2; exit 3; }
echo "answer to: $2""#,
        );
        let mark = std::env::temp_dir().join(format!("friday-agent-mark-{}", std::process::id()));
        std::env::set_var("TMPDIR_MARK", &mark);
        let p = AgentCliProvider {
            cli: AgentCli::Claude,
            binary,
        };
        assert_eq!(
            p.complete("m", "hello", false).await.unwrap(),
            "answer to: hello"
        );
        let ran_in = std::fs::read_to_string(&mark).unwrap();
        assert!(
            !Path::new(ran_in.trim()).exists(),
            "the run's directory is removed"
        );
        let _ = std::fs::remove_file(mark);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn a_failing_or_silent_tool_is_an_error() {
        let failing = AgentCliProvider {
            cli: AgentCli::Cline,
            binary: shim("fails", "echo 'not signed in' >&2; exit 1"),
        };
        let e = failing.complete("m", "q", false).await.unwrap_err();
        assert!(e.contains("cline failed: not signed in"), "{e}");
        let silent = AgentCliProvider {
            cli: AgentCli::Codex,
            binary: shim("silent", "exit 0"),
        };
        assert!(silent
            .complete("m", "q", false)
            .await
            .unwrap_err()
            .contains("gave no answer"));
        let missing = AgentCliProvider {
            cli: AgentCli::Antigravity,
            binary: "/nonexistent/agy".into(),
        };
        assert!(missing
            .complete("m", "q", false)
            .await
            .unwrap_err()
            .contains("isn't available"));
    }

    #[test]
    fn agent_tools_get_more_time_than_an_http_call() {
        assert_eq!(
            AgentCliProvider::new(AgentCli::Claude).min_request_timeout(),
            Some(AGENT_CLI_TIMEOUT)
        );
    }
}
