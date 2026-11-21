//! What a project's own repository says (vNext N2): when an ask names a
//! project ("continue building FRIDAY", "what's left on the site"), its
//! folder is found from what FRIDAY already knows (saved workspaces, the
//! projects answers were asked in, coding-agent runs) and git is asked,
//! read-only, for the branch, the files not yet committed and the last
//! commits. Every line is a fact from git; nothing is guessed.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

/// Folders FRIDAY has seen projects in, newest knowledge last.
fn known_folders(state_dir: &Path, workspaces: &Path) -> Vec<String> {
    let mut out: Vec<String> = crate::workspace::list(workspaces)
        .into_iter()
        .filter_map(|s| s.project)
        .collect();
    let answers = std::fs::read_to_string(state_dir.join("answers.md")).unwrap_or_default();
    out.extend(answers.lines().filter_map(|l| {
        l.trim()
            .strip_prefix("_project: ")
            .and_then(|p| p.strip_suffix('_'))
            .map(String::from)
    }));
    if let Ok(dir) = std::fs::read_dir(state_dir.join("agents")) {
        for f in dir.flatten() {
            let v: serde_json::Value = std::fs::read_to_string(f.path())
                .ok()
                .and_then(|t| serde_json::from_str(&t).ok())
                .unwrap_or_default();
            if let Some(folder) = v["folder"].as_str() {
                out.push(folder.to_string());
            }
        }
    }
    out
}

/// The folder of the project an ask names, if FRIDAY knows one.
pub fn find(state_dir: &Path, workspaces: &Path, goal: &str) -> Option<PathBuf> {
    known_folders(state_dir, workspaces)
        .into_iter()
        .rev()
        .find(|f| crate::thoughts::named(goal, f))
        .map(PathBuf::from)
        .filter(|p| p.join(".git").exists())
}

fn git(dir: &Path, args: &[&str]) -> Option<String> {
    let mut child = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .env("GIT_OPTIONAL_LOCKS", "0")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .ok()?;
    // Read while it runs (a full pipe would stall it), and give up after
    // three seconds: a huge repository mustn't hold an answer up.
    let mut stdout = child.stdout.take()?;
    let reader = std::thread::spawn(move || {
        let mut out = String::new();
        std::io::Read::read_to_string(&mut stdout, &mut out).ok();
        out
    });
    let deadline = std::time::Instant::now() + Duration::from_secs(3);
    let ok = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status.success(),
            Ok(None) if std::time::Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(20))
            }
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                break false;
            }
        }
    };
    let out = reader.join().ok()?;
    ok.then_some(out)
}

/// Branch, uncommitted files and the last commits, as `[memory]` lines.
pub fn facts(dir: &Path) -> Option<String> {
    // All three at once: a slow repository costs three seconds, not nine.
    let (branch, status, log) = std::thread::scope(|s| {
        let branch = s.spawn(|| git(dir, &["rev-parse", "--abbrev-ref", "HEAD"]));
        let status = s.spawn(|| git(dir, &["status", "--porcelain"]));
        let log = s.spawn(|| {
            git(
                dir,
                &[
                    "log",
                    "-5",
                    "--date=format:%Y-%m-%d %H:%M",
                    "--format=%ad %s",
                ],
            )
        });
        (
            branch.join().ok().flatten(),
            status.join().ok().flatten(),
            log.join().ok().flatten().unwrap_or_default(),
        )
    });
    let branch = branch?;
    let changed: Vec<&str> = status
        .as_deref()
        .unwrap_or_default()
        .lines()
        .filter(|l| !l.trim().is_empty())
        .collect();
    let mut out = format!(
        "{} (git): on branch {}; {}",
        crate::thoughts::project_name(&dir.to_string_lossy()),
        branch.trim(),
        match status {
            Some(_) => format!("{} file(s) not committed", changed.len()),
            None => "uncommitted files unknown (git status didn't answer)".to_string(),
        }
    );
    if !changed.is_empty() {
        let names: Vec<&str> = changed
            .iter()
            .take(10)
            .map(|l| l.get(3..).unwrap_or(l).trim())
            .collect();
        out.push_str(&format!(" ({})", names.join(", ")));
    }
    out.push('\n');
    if !log.trim().is_empty() {
        out.push_str("last commits:\n");
        out.push_str(log.trim_end());
        out.push('\n');
    }
    Some(out)
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    fn run(dir: &Path, args: &[&str]) {
        let ok = Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(args)
            .env("GIT_AUTHOR_NAME", "t")
            .env("GIT_AUTHOR_EMAIL", "t@example.com")
            .env("GIT_COMMITTER_NAME", "t")
            .env("GIT_COMMITTER_EMAIL", "t@example.com")
            .status()
            .unwrap()
            .success();
        assert!(ok, "git {args:?}");
    }

    #[test]
    fn a_named_project_is_found_and_git_says_where_it_stands() {
        let root = std::env::temp_dir().join(format!("friday-project-{}", uuid::Uuid::new_v4()));
        let repo = root.join("project-friday");
        std::fs::create_dir_all(&repo).unwrap();
        run(&repo, &["init", "-q", "-b", "main"]);
        std::fs::write(repo.join("a.rs"), "fn main() {}\n").unwrap();
        run(&repo, &["add", "a.rs"]);
        run(&repo, &["commit", "-q", "-m", "add the workspace module"]);
        std::fs::write(repo.join("b.rs"), "// wip\n").unwrap();

        let state = root.join("state");
        std::fs::create_dir_all(&state).unwrap();
        std::fs::write(
            state.join("answers.md"),
            format!(
                "<!-- friday answer -->\n# q\n\n_2026-11-20 10:05_\n\n_project: {}_\n\nA\n",
                repo.display()
            ),
        )
        .unwrap();
        let ws = root.join("workspaces");
        let found = find(&state, &ws, "let's continue building FRIDAY").unwrap();
        assert_eq!(found, repo);
        assert!(find(&state, &ws, "what's the weather").is_none());

        let text = facts(&found).unwrap();
        assert!(
            text.starts_with(
                "project-friday (git): on branch main; 1 file(s) not committed (b.rs)"
            ),
            "{text}"
        );
        assert!(text.contains("add the workspace module"), "{text}");
        assert!(facts(&root).is_none(), "not a repository");
    }
}
