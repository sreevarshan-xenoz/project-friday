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
    let mut a: Vec<String> = vec![
        "--die-with-parent".into(),
        "--dev-bind".into(),
        "/".into(),
        "/".into(),
        "--ro-bind".into(),
        s(home),
        s(home),
    ];
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
    for p in HIDDEN {
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
        let script = format!(
            "cat {h}/.ssh/id_ed25519 {h}/.netrc 2>/dev/null; \
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
        assert_eq!(
            std::fs::read_to_string(home.join(".ssh/id_ed25519")).unwrap(),
            "SECRET KEY",
            "the key is untouched"
        );
        let _ = std::fs::remove_dir_all(&root);
    }
}
