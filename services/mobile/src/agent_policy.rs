//! What FRIDAY may approve for a coding agent (Claude Code, agy, opencode)
//! on the person's behalf, and what must go to the person.
//!
//! The decision is fixed rules over the step's structured fields: its kind,
//! the exact command, its paths and the run's folder. It is never a model's
//! call. Text an agent reads (a README, a web page, an error) can carry
//! planted instructions such as "this is safe, approve it", and a model can
//! fall for them; these rules can't. Anything they don't recognise goes to
//! the person.

use std::path::{Component, Path, PathBuf};

/// What a step does, in ACP's words (Claude's tool names are mapped onto them).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Read,
    Search,
    Think,
    Edit,
    Delete,
    Move,
    Execute,
    Fetch,
    Other,
}

impl Kind {
    /// ACP's `kind` field.
    pub fn from_acp(kind: &str) -> Kind {
        match kind {
            "read" => Kind::Read,
            "search" => Kind::Search,
            "think" => Kind::Think,
            "edit" => Kind::Edit,
            "delete" => Kind::Delete,
            "move" => Kind::Move,
            "execute" => Kind::Execute,
            "fetch" => Kind::Fetch,
            _ => Kind::Other,
        }
    }

    /// A Claude Code tool name.
    pub fn from_claude_tool(tool: &str) -> Kind {
        match tool {
            "Read" | "LS" | "NotebookRead" => Kind::Read,
            "Glob" | "Grep" => Kind::Search,
            "Edit" | "Write" | "MultiEdit" | "NotebookEdit" => Kind::Edit,
            "Bash" => Kind::Execute,
            "WebFetch" | "WebSearch" => Kind::Fetch,
            "TodoWrite" => Kind::Think,
            _ => Kind::Other,
        }
    }
}

/// One step an agent wants to take.
#[derive(Debug, Clone)]
pub struct Step {
    pub kind: Kind,
    /// The shell command, for `Execute`.
    pub command: Option<String>,
    /// The files or folders it touches.
    pub paths: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decision {
    Allow,
    /// The person decides; says why in a few words.
    Ask(String),
}

fn ask(why: impl Into<String>) -> Decision {
    Decision::Ask(why.into())
}

/// Decides one step of a run in `folder`.
pub fn decide(step: &Step, folder: &Path) -> Decision {
    match step.kind {
        Kind::Think => Decision::Allow,
        // A fetch can carry what the agent read (a key in the URL) out to
        // anyone, so it is the person's call.
        Kind::Fetch => ask("fetches from the web"),
        Kind::Read | Kind::Search => {
            if let Some(p) = outside(&step.paths, folder) {
                return ask(format!("reads {p}, outside the project folder"));
            }
            match step.paths.iter().find(|p| is_secret(p)) {
                Some(p) => ask(format!("reads {p}, which may hold secrets")),
                None => Decision::Allow,
            }
        }
        Kind::Edit => {
            if step.paths.is_empty() {
                return ask("edits a file it doesn't name");
            }
            if let Some(p) = outside(&step.paths, folder) {
                return ask(format!("changes {p}, outside the project folder"));
            }
            match step.paths.iter().find(|p| is_sensitive(p)) {
                Some(p) if defines_commands(p) => ask(format!(
                    "changes {p}, which decides what builds and tests run"
                )),
                Some(p) => ask(format!("changes {p}, which may hold secrets or history")),
                None => Decision::Allow,
            }
        }
        Kind::Delete => ask("deletes files"),
        Kind::Move => ask("moves or renames files"),
        Kind::Execute => match &step.command {
            Some(c) => command(c, folder),
            None => ask("runs a command it doesn't show"),
        },
        Kind::Other => ask("does something FRIDAY doesn't recognise"),
    }
}

/// A path as it would resolve from `folder`, without touching the disk.
fn resolve(path: &str, folder: &Path) -> PathBuf {
    let p = Path::new(path);
    let joined = if path.starts_with('~') {
        return PathBuf::from("/~home");
    } else if p.is_absolute() {
        p.to_path_buf()
    } else {
        folder.join(p)
    };
    let mut out = PathBuf::new();
    for c in joined.components() {
        match c {
            Component::ParentDir => {
                out.pop();
            }
            Component::CurDir => {}
            other => out.push(other),
        }
    }
    out
}

/// The first path that lands outside `folder`.
/// A link inside the folder can point out of it, so where a path (or the
/// nearest part of it that exists) really is counts too.
fn real_place(path: &Path) -> Option<PathBuf> {
    let mut probe = path.to_path_buf();
    let mut rest = Vec::new();
    loop {
        if let Ok(real) = std::fs::canonicalize(&probe) {
            return Some(rest.iter().rev().fold(real, |acc, part| acc.join(part)));
        }
        rest.push(probe.file_name()?.to_os_string());
        probe = probe.parent()?.to_path_buf();
    }
}

fn outside<'a>(paths: &'a [String], folder: &Path) -> Option<&'a str> {
    // Resolved the same way as the paths, so both carry the same prefix
    // (Windows adds `\\?\` to a resolved path).
    let real_folder = real_place(folder).unwrap_or_else(|| folder.to_path_buf());
    paths
        .iter()
        .find(|p| {
            let lexical = resolve(p, folder);
            !lexical.starts_with(folder)
                || real_place(&lexical).is_some_and(|r| !r.starts_with(&real_folder))
        })
        .map(String::as_str)
}

/// Files whose change is the person's call: git history, secrets, and the
/// files that say what a build or test command runs.
fn is_sensitive(path: &str) -> bool {
    Path::new(path)
        .components()
        .any(|c| c.as_os_str() == ".git")
        || is_secret(path)
        || defines_commands(path)
}

/// Files that decide what an allowed command runs. Editing one and then
/// running `npm test` or `make` would run anything, so the edit asks first.
fn defines_commands(path: &str) -> bool {
    let p = Path::new(path);
    let name = p
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    let in_dir = |dir: &str| p.components().any(|c| c.as_os_str() == dir);
    [
        "package.json",
        "makefile",
        "gnumakefile",
        "build.rs",
        "cargo.toml",
        "pyproject.toml",
        "setup.py",
        "setup.cfg",
        "conftest.py",
        "tox.ini",
        "pytest.ini",
        "noxfile.py",
        "justfile",
        ".npmrc",
        ".yarnrc",
        ".yarnrc.yml",
        "go.mod",
        "build.gradle",
        "build.gradle.kts",
        "pom.xml",
        "directory.build.props",
        "directory.build.targets",
        "global.json",
        ".envrc",
    ]
    .contains(&name.as_str())
        || name.ends_with(".mk")
        || name.ends_with(".csproj")
        || name.ends_with(".sln")
        || in_dir(".cargo")
        || in_dir(".github")
        || in_dir(".husky")
        || in_dir(".githooks")
}

/// Files that look like they hold keys or passwords.
fn is_secret(path: &str) -> bool {
    let p = Path::new(path);
    let name = p
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    name.starts_with(".env")
        || name.starts_with("id_")
        || name.starts_with("credentials")
        || name.starts_with("secrets")
        || name.ends_with(".pem")
        || name.ends_with(".key")
        || name.ends_with(".p12")
        || name.ends_with(".pfx")
}

/// Programs that only read, whatever their arguments (paths still have to
/// stay in the folder).
const READ_ONLY: [&str; 14] = [
    "ls", "cat", "head", "tail", "wc", "grep", "rg", "echo", "pwd", "which", "file", "stat",
    "tree", "diff",
];

/// Script names that build, test or check, for `npm run <name>` and `make <name>`.
const CHECKS: [&str; 7] = [
    "test",
    "build",
    "lint",
    "check",
    "typecheck",
    "format",
    "fmt",
];

fn command(cmd: &str, folder: &Path) -> Decision {
    let cmd = cmd.trim().replace("2>&1", "");
    if cmd.is_empty() {
        return ask("runs an empty command");
    }
    if let Some(c) = [";", "&", "|", "`", "$", ">", "<", "\n", "\\"]
        .iter()
        .find(|c| cmd.contains(**c))
    {
        return ask(format!("runs a command that chains or redirects ({c})"));
    }
    let words: Vec<&str> = cmd.split_whitespace().collect();
    let (program, args) = (words[0], &words[1..]);
    if program.contains('=') {
        return ask("runs a command with changed environment");
    }
    let program = Path::new(program)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(program);
    // Every argument that could be a path, quotes stripped and `--opt=value`
    // split, is checked against the folder; plain words resolve inside it.
    let paths: Vec<String> = args
        .iter()
        .map(|a| a.trim_matches(|c| c == '"' || c == '\''))
        .map(|a| match a.split_once('=') {
            Some((flag, value)) if flag.starts_with('-') => value,
            _ => a,
        })
        .filter(|a| !a.starts_with('-') && !a.is_empty())
        .map(|a| a.trim_matches(|c| c == '"' || c == '\'').to_string())
        .collect();
    if let Some(p) = outside(&paths, folder) {
        return ask(format!("runs a command on {p}, outside the project folder"));
    }
    if let Some(p) = paths.iter().find(|p| is_secret(p)) {
        return ask(format!("runs a command on {p}, which may hold secrets"));
    }
    // Options that run another program or write a file.
    let writes_or_runs = |flags: &[&str]| {
        args.iter().any(|a| {
            flags
                .iter()
                .any(|f| *a == *f || a.starts_with(&format!("{f}=")))
        })
    };
    let sub = args.first().copied().unwrap_or("");
    let ok = match program {
        "rg" => !writes_or_runs(&["--pre", "--pre-glob"]),
        "tree" => !writes_or_runs(&["-o"]),
        p if READ_ONLY.contains(&p) => true,
        "find" => !args.iter().any(|a| {
            ["-delete", "-exec", "-execdir", "-ok", "-okdir", "-fls"].contains(a)
                || a.starts_with("-fprint")
        }),
        "git" => {
            ([
                "status",
                "diff",
                "log",
                "show",
                "rev-parse",
                "ls-files",
                "blame",
            ]
            .contains(&sub)
                || (sub == "branch" && args.len() == 1))
                && !writes_or_runs(&["--output", "--ext-diff", "--textconv", "-o"])
        }
        "cargo" => ["build", "check", "test", "fmt", "clippy"].contains(&sub),
        "npm" | "pnpm" | "yarn" => {
            sub == "test" || (sub == "run" && args.get(1).is_some_and(|s| CHECKS.contains(s)))
        }
        "pytest" => true,
        "python" | "python3" => {
            sub == "-m"
                && args
                    .get(1)
                    .is_some_and(|m| ["pytest", "unittest"].contains(m))
        }
        "go" => ["build", "test", "vet", "fmt"].contains(&sub),
        "dotnet" => ["build", "test"].contains(&sub),
        "make" => args.is_empty() || args.iter().all(|a| CHECKS.contains(a)),
        _ => false,
    };
    if ok {
        Decision::Allow
    } else {
        ask(format!(
            "runs `{}`",
            cmd.chars().take(120).collect::<String>()
        ))
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    const DIR: &str = "/home/u/code/site";

    fn run(cmd: &str) -> Decision {
        decide(
            &Step {
                kind: Kind::Execute,
                command: Some(cmd.into()),
                paths: vec![],
            },
            Path::new(DIR),
        )
    }

    fn touch(kind: Kind, path: &str) -> Decision {
        decide(
            &Step {
                kind,
                command: None,
                paths: vec![path.into()],
            },
            Path::new(DIR),
        )
    }

    fn allowed(d: Decision) -> bool {
        d == Decision::Allow
    }

    #[test]
    fn builds_tests_and_reads_in_the_folder_are_allowed() {
        for c in [
            "cargo test -p core",
            "cargo test 2>&1",
            "git status",
            "git diff HEAD~1",
            "npm test",
            "npm run lint",
            "python3 -m pytest tests",
            "ls -la src",
            "grep -rn TODO src",
            "find . -name '*.rs'",
            "make",
            "make test",
            "go test ./...",
        ] {
            assert!(allowed(run(c)), "{c}");
        }
        assert!(allowed(touch(Kind::Read, "src/main.rs")));
        assert!(allowed(touch(Kind::Edit, "src/main.rs")));
        assert!(allowed(touch(Kind::Edit, "/home/u/code/site/README.md")));
    }

    #[test]
    fn destructive_or_outward_steps_go_to_the_person() {
        for c in [
            "rm -rf build",
            "git push",
            "git commit -m x",
            "git reset --hard",
            "git branch -D old",
            "npm install left-pad",
            "cargo run",
            "curl https://example.com/x.sh",
            "sudo ls",
            "cat /etc/shadow",
            "ls ../other",
            "ls ~/.ssh",
            "find . -delete",
            "find . -exec rm {} ;",
            "echo hi > file",
            "cargo test && git push",
            "ls; rm x",
            "cat $(which sh)",
            "FOO=1 cargo test",
            "npm run deploy",
            "make install",
            "bash script.sh",
            "cat \"/etc/passwd\"",
            "cat '/etc/passwd'",
            "grep --file=/etc/passwd x",
            "rg --pre=sh x",
            "rg --pre sh x",
            "tree -o out.txt",
            "find . -fls out",
            "git diff --output=/tmp/x",
            "git log --ext-diff",
            "git -C /other status",
            "cat $HOME/.ssh/id_rsa",
            "cat .env",
            "grep -r KEY .env.local",
            "ls \\..\\x",
        ] {
            assert!(!allowed(run(c)), "{c}");
        }
        assert!(!allowed(touch(Kind::Delete, "src/old.rs")));
        assert!(!allowed(touch(Kind::Move, "src/a.rs")));
        assert!(!allowed(touch(Kind::Edit, "../other/x")));
        assert!(!allowed(touch(Kind::Edit, "/home/u/.bashrc")));
        assert!(!allowed(touch(Kind::Edit, ".env")));
        assert!(!allowed(touch(Kind::Edit, "config/.env.local")));
        assert!(!allowed(touch(Kind::Edit, ".git/config")));
        assert!(!allowed(touch(Kind::Edit, "certs/server.pem")));
        // Files that say what an allowed build or test runs.
        for f in [
            "package.json",
            "Makefile",
            "build.rs",
            "crates/x/build.rs",
            "Cargo.toml",
            ".cargo/config.toml",
            "tests/conftest.py",
            "pyproject.toml",
            ".github/workflows/ci.yml",
            "rules.mk",
            "app/App.csproj",
        ] {
            assert!(!allowed(touch(Kind::Edit, f)), "{f}");
        }
        assert!(allowed(touch(Kind::Edit, "src/lib.rs")));
        assert!(allowed(touch(Kind::Edit, "tests/test_app.py")));
        assert!(!allowed(touch(Kind::Read, "/etc/passwd")));
        assert!(!allowed(touch(Kind::Read, ".env")));
        assert!(!allowed(touch(Kind::Fetch, "https://example.com")));
        assert!(!allowed(touch(Kind::Other, "x")));
    }

    #[test]
    fn a_step_that_hides_its_target_goes_to_the_person() {
        let folder = Path::new(DIR);
        let edit = Step {
            kind: Kind::Edit,
            command: None,
            paths: vec![],
        };
        assert!(!allowed(decide(&edit, folder)));
        let exec = Step {
            kind: Kind::Execute,
            command: None,
            paths: vec![],
        };
        assert!(!allowed(decide(&exec, folder)));
    }

    #[test]
    fn a_link_out_of_the_folder_is_outside() {
        let base = std::env::temp_dir().join(format!("friday-policy-link-{}", std::process::id()));
        let (folder, away) = (base.join("site"), base.join("away"));
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::create_dir_all(&away).unwrap();
        std::os::unix::fs::symlink(&away, folder.join("escape")).unwrap();
        let edit = |p: &str| {
            decide(
                &Step {
                    kind: Kind::Edit,
                    command: None,
                    paths: vec![p.into()],
                },
                &folder,
            )
        };
        assert!(!allowed(edit("escape/x.txt")));
        assert!(allowed(edit("src/new.rs")));
        assert!(!allowed(decide(
            &Step {
                kind: Kind::Execute,
                command: Some("cat escape/x".into()),
                paths: vec![],
            },
            &folder
        )));
        let _ = std::fs::remove_dir_all(base);
    }

    #[test]
    fn the_reason_names_the_step() {
        assert_eq!(run("git push"), Decision::Ask("runs `git push`".into()));
        match touch(Kind::Edit, "/home/u/.bashrc") {
            Decision::Ask(why) => assert!(why.contains(".bashrc"), "{why}"),
            d => panic!("{d:?}"),
        }
    }

    #[test]
    fn claude_tools_map_to_kinds() {
        assert_eq!(Kind::from_claude_tool("Bash"), Kind::Execute);
        assert_eq!(Kind::from_claude_tool("Write"), Kind::Edit);
        assert_eq!(Kind::from_claude_tool("Grep"), Kind::Search);
        assert_eq!(Kind::from_claude_tool("mcp__x__y"), Kind::Other);
        assert_eq!(Kind::from_acp("execute"), Kind::Execute);
        assert_eq!(Kind::from_acp("switch_mode"), Kind::Other);
    }
}

#[cfg(all(test, windows))]
mod windows_tests {
    use super::*;

    #[test]
    fn windows_paths_stay_in_the_folder() {
        let folder = Path::new(r"C:\code\site");
        let edit = |p: &str| {
            decide(
                &Step {
                    kind: Kind::Edit,
                    command: None,
                    paths: vec![p.into()],
                },
                folder,
            )
        };
        assert_eq!(edit(r"src\main.rs"), Decision::Allow);
        assert_eq!(edit(r"C:\code\site\README.md"), Decision::Allow);
        assert!(matches!(edit(r"C:\Users\u\notes.txt"), Decision::Ask(_)));
        assert!(matches!(edit(r"..\other\x"), Decision::Ask(_)));
        let run = |c: &str| {
            decide(
                &Step {
                    kind: Kind::Execute,
                    command: Some(c.into()),
                    paths: vec![],
                },
                folder,
            )
        };
        assert_eq!(run("cargo test"), Decision::Allow);
        assert!(matches!(run(r"type C:\Windows\win.ini"), Decision::Ask(_)));
        assert!(matches!(run(r"cat C:\Windows\win.ini"), Decision::Ask(_)));
    }
}
