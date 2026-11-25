//! A coding agent run in a sandbox (`FRIDAY_AGENT_SANDBOX=1`, Linux, with
//! bubblewrap): the home folder is read-only, keys and passwords (`~/.ssh`,
//! `~/.gnupg`, keyrings, password stores, cloud credentials, FRIDAY's own
//! settings) are hidden, and only the run's folder and the agent's own state
//! folders can be written. The agent still reaches the network (its model)
//! and the rest of the system read-only.

use std::path::{Path, PathBuf};

use crate::agent_runs::Agent;

/// On when the person asked for it.
pub fn on() -> bool {
    std::env::var("FRIDAY_AGENT_SANDBOX").is_ok_and(|v| v.trim() == "1")
}

/// Folders and files in the home folder an agent never sees.
const HIDDEN: [&str; 12] = [
    ".ssh",
    ".gnupg",
    ".password-store",
    ".local/share/keyrings",
    ".config/friday",
    ".aws",
    ".azure",
    ".kube",
    ".docker",
    ".config/gcloud",
    ".netrc",
    ".git-credentials",
];

/// What each agent keeps for itself (its sign-in, sessions, caches).
fn own_state(agent: Agent) -> &'static [&'static str] {
    match agent {
        Agent::Opencode => &[
            ".config/opencode",
            ".local/share/opencode",
            ".local/state/opencode",
        ],
        Agent::Claude => &[".claude", ".claude.json", ".config/claude"],
        Agent::Agy => &[".gemini"],
        Agent::Codex => &[".codex"],
    }
}

/// The program and arguments that run `program args…` for `agent` in the
/// sandbox, working in `workdir` (and `git_dir`, a worktree's repository,
/// for its commits). Only paths that exist are mounted.
pub fn wrap(
    agent: Agent,
    program: &Path,
    args: &[String],
    workdir: &Path,
    git_dir: Option<&Path>,
    home: &Path,
) -> (String, Vec<String>) {
    let s = |p: &Path| p.display().to_string();
    // The system read-only, its own /dev, /proc and /tmp, and none of the
    // session's sockets (the SSH agent, D-Bus and the keyring live there).
    let mut a: Vec<String> = [
        "--die-with-parent",
        "--ro-bind",
        "/",
        "/",
        "--dev",
        "/dev",
        "--proc",
        "/proc",
        "--tmpfs",
        "/tmp",
        "--tmpfs",
        "/var/tmp",
        "--unsetenv",
        "SSH_AUTH_SOCK",
        "--unsetenv",
        "DBUS_SESSION_BUS_ADDRESS",
        "--unsetenv",
        "GPG_AGENT_INFO",
    ]
    .map(String::from)
    .to_vec();
    if let Some(run) = std::env::var_os("XDG_RUNTIME_DIR").filter(|r| Path::new(r).is_dir()) {
        a.extend(["--tmpfs".into(), s(Path::new(&run))]);
    }
    a.extend(["--ro-bind".into(), s(home), s(home)]);
    let rw = |p: PathBuf, a: &mut Vec<String>| {
        if p.exists() {
            a.extend(["--bind".into(), s(&p), s(&p)]);
        }
    };
    for p in own_state(agent).iter().chain(&[".cache", ".npm"]) {
        rw(home.join(p), &mut a);
    }
    rw(workdir.to_path_buf(), &mut a);
    if let Some(g) = git_dir {
        rw(g.to_path_buf(), &mut a);
    }
    // Nor the other agents' sign-ins and sessions.
    let others = Agent::ALL
        .into_iter()
        .filter(|a| *a != agent)
        .flat_map(|a| own_state(a).iter().copied());
    for p in HIDDEN.into_iter().chain(others) {
        let p = home.join(p);
        if p.is_dir() {
            a.extend(["--tmpfs".into(), s(&p)]);
        } else if p.exists() {
            a.extend(["--ro-bind".into(), "/dev/null".into(), s(&p)]);
        }
    }
    a.push("--".into());
    a.push(s(program));
    a.extend(args.iter().cloned());
    ("bwrap".into(), a)
}

/// Makes the agent's own state folders (and `.cache`) before a run: the
/// home folder is read-only inside, so one missing then couldn't be made.
pub fn prepare(agent: Agent, home: &Path) {
    for p in own_state(agent).iter().chain(&[".cache"]) {
        let p = home.join(p);
        if p.exists() {
            continue;
        }
        if p.extension().is_some_and(|e| e == "json") {
            let _ = std::fs::write(&p, "{}");
        } else {
            let _ = std::fs::create_dir_all(&p);
        }
    }
}

/// The home folder.
pub fn home() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from)
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;

    /// The real thing: under bubblewrap (when this machine has it), with a
    /// made-up home, the agent reads no key, writes only its folder.
    #[test]
    fn a_sandboxed_agent_writes_only_its_folder_and_sees_no_keys() {
        if std::process::Command::new("bwrap")
            .args(["--dev-bind", "/", "/", "true"])
            .status()
            .map_or(true, |s| !s.success())
        {
            eprintln!("no working bwrap here: skipped");
            return;
        }
        let root = std::env::temp_dir().join(format!(
            "friday-sandbox-{}-{}",
            std::process::id(),
            rand::random::<u32>()
        ));
        let home = root.join("home");
        let work = home.join("code/site");
        for d in [
            home.join(".ssh"),
            work.clone(),
            home.join(".config/opencode"),
        ] {
            std::fs::create_dir_all(d).unwrap();
        }
        std::fs::write(home.join(".ssh/id_ed25519"), "SECRET KEY").unwrap();
        std::fs::write(home.join(".netrc"), "machine x password p").unwrap();
        // Another agent's sign-in: hidden from this one.
        std::fs::create_dir_all(home.join(".codex")).unwrap();
        std::fs::write(home.join(".codex/auth.json"), "CODEX TOKEN").unwrap();
        std::fs::write(home.join(".claude.json"), "CLAUDE TOKEN").unwrap();
        prepare(Agent::Opencode, &home);
        assert!(
            home.join(".local/share/opencode").is_dir(),
            "its state folder is made first"
        );
        let script = format!(
            "cat {h}/.ssh/id_ed25519 {h}/.netrc {h}/.codex/auth.json {h}/.claude.json 2>/dev/null; \
             echo sock=$SSH_AUTH_SOCK; \
             echo leak > /var/tmp/friday-sandbox-leak 2>/dev/null; \
             echo state > {h}/.local/share/opencode/s 2>/dev/null; \
             echo ok > {w}/made.txt; \
             echo bad > {h}/outside.txt 2>/dev/null; \
             echo cfg > {h}/.config/opencode/state 2>/dev/null; true",
            h = home.display(),
            w = work.display()
        );
        let (prog, args) = wrap(
            Agent::Opencode,
            Path::new("/bin/sh"),
            &["-c".into(), script],
            &work,
            None,
            &home,
        );
        let out = std::process::Command::new(prog)
            .args(args)
            .output()
            .unwrap();
        let said = String::from_utf8_lossy(&out.stdout);
        assert!(
            !said.contains("SECRET KEY") && !said.contains("password"),
            "{said}"
        );
        assert!(
            !said.contains("CODEX TOKEN") && !said.contains("CLAUDE TOKEN"),
            "saw another agent's sign-in: {said}"
        );
        assert_eq!(
            std::fs::read_to_string(work.join("made.txt")).unwrap(),
            "ok\n"
        );
        assert!(
            !home.join("outside.txt").exists(),
            "wrote outside its folder"
        );
        assert!(
            home.join(".config/opencode/state").exists(),
            "its own state is writable"
        );
        assert!(home.join(".local/share/opencode/s").exists());
        assert!(said.contains("sock=\n"), "no SSH agent: {said}");
        assert!(
            !Path::new("/var/tmp/friday-sandbox-leak").exists(),
            "wrote outside, in /var/tmp"
        );
        assert_eq!(
            std::fs::read_to_string(home.join(".ssh/id_ed25519")).unwrap(),
            "SECRET KEY",
            "the key is untouched"
        );
        let _ = std::fs::remove_dir_all(&root);
    }
}
