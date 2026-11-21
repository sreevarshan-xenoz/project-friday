//! Workspaces: "save this as my FRIDAY workspace", then "continue building
//! FRIDAY" puts the work back (vNext N1). A saved workspace keeps each
//! window's app, workspace and (floating) place, each terminal's folder and
//! the editor running in it with its files, and the project folder they
//! share. Restoring moves windows that are already open to their workspace
//! and starts the missing ones there, then says what came back and what
//! didn't. Hyprland only for now.
//!
//! Files: `~/.local/share/friday/workspaces/<name>.json`, owner-only. Nothing
//! is sent anywhere. Browsers restore their own tabs (their session
//! restore); FRIDAY starts the browser on its workspace.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Terminals FRIDAY reopens in a folder: window class, program, and the
/// flag it takes for the folder.
const TERMINALS: &[(&str, &str, &str)] = &[
    ("kitty", "kitty", "--directory"),
    ("foot", "foot", "--working-directory"),
    ("alacritty", "alacritty", "--working-directory"),
    ("com.mitchellh.ghostty", "ghostty", "--working-directory"),
    ("org.wezfurlong.wezterm", "wezterm", "start --cwd"),
];
/// Chromium-based browsers (by program name): restarted with their last
/// session, so the tabs come back too.
const CHROMIUMS: &[&str] = &[
    "chromium",
    "chrome",
    "google-chrome",
    "google-chrome-stable",
    "brave",
    "brave-browser",
    "vivaldi",
    "vivaldi-stable",
    "microsoft-edge",
    "microsoft-edge-stable",
];
/// Editors run inside a terminal that are reopened with their files.
const EDITORS: &[&str] = &["nvim", "vim", "vi", "hx", "helix", "nano", "micro", "emacs"];

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Window {
    pub class: String,
    pub title: String,
    pub workspace: String,
    #[serde(default)]
    pub floating: bool,
    #[serde(default)]
    pub at: [i32; 2],
    #[serde(default)]
    pub size: [i32; 2],
    /// How it was started (`/proc/<pid>/cmdline`).
    #[serde(default)]
    pub command: Vec<String>,
    /// A terminal's folder.
    #[serde(default)]
    pub cwd: Option<String>,
    /// The editor running in a terminal, with its arguments.
    #[serde(default)]
    pub run: Vec<String>,
    /// Hyprland's address for it, while it is open (not saved).
    #[serde(skip)]
    pub address: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Saved {
    pub name: String,
    /// Unix seconds.
    pub saved: i64,
    /// The project folder the terminals share (a git repo's top), if any.
    pub project: Option<String>,
    pub windows: Vec<Window>,
}

/// What `hyprctl` gives and takes.
pub trait Hypr {
    /// `hyprctl -j clients`.
    fn clients(&self) -> Result<String, String>;
    /// Run a Lua dispatch (`hyprctl eval '…'`).
    fn eval(&self, lua: &str) -> Result<(), String>;
}

pub struct SystemHypr;

/// `hyprctl`, pointed at the newest Hyprland instance when the environment
/// doesn't name one (a systemd timer starts without it).
fn hyprctl() -> std::process::Command {
    let mut cmd = std::process::Command::new("hyprctl");
    if std::env::var_os("HYPRLAND_INSTANCE_SIGNATURE").is_none() {
        let runtime = std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from);
        let newest = runtime
            .and_then(|r| std::fs::read_dir(r.join("hypr")).ok())
            .and_then(|d| {
                d.flatten()
                    .filter(|e| e.path().is_dir())
                    .max_by_key(|e| e.metadata().and_then(|m| m.modified()).ok())
            });
        if let Some(e) = newest {
            cmd.env("HYPRLAND_INSTANCE_SIGNATURE", e.file_name());
        }
    }
    cmd
}

impl Hypr for SystemHypr {
    fn clients(&self) -> Result<String, String> {
        let out = hyprctl()
            .args(["-j", "clients"])
            .output()
            .map_err(|e| format!("hyprctl: {e}"))?;
        if !out.status.success() {
            return Err("Hyprland isn't running here".into());
        }
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    }

    fn eval(&self, lua: &str) -> Result<(), String> {
        let out = hyprctl()
            .args(["eval", lua])
            .output()
            .map_err(|e| format!("hyprctl: {e}"))?;
        let text = String::from_utf8_lossy(&out.stdout);
        if out.status.success() && !text.to_lowercase().contains("error") {
            Ok(())
        } else {
            Err(text.trim().to_string())
        }
    }
}

pub fn dir() -> PathBuf {
    crate::routine::dir()
        .parent()
        .map(|p| p.join("workspaces"))
        .unwrap_or_else(|| PathBuf::from("/tmp/friday/workspaces"))
}

/// A name as a file name: lowercase words joined by `-`; "my" dropped.
pub fn key(name: &str) -> String {
    let lower = name.trim().to_lowercase();
    let lower = lower.strip_prefix("my ").unwrap_or(&lower);
    lower
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty() && *w != "workspace")
        .collect::<Vec<_>>()
        .join("-")
}

fn read_proc(proc_root: &Path, pid: i64, what: &str) -> Option<String> {
    std::fs::read_to_string(proc_root.join(pid.to_string()).join(what)).ok()
}

fn cmdline(proc_root: &Path, pid: i64) -> Vec<String> {
    let args: Vec<String> = read_proc(proc_root, pid, "cmdline")
        .map(|s| {
            s.split('\0')
                .filter(|a| !a.is_empty())
                .map(String::from)
                .collect()
        })
        .unwrap_or_default();
    // Chromium and Electron apps rewrite their process title: the whole
    // command line comes back as one string joined by spaces.
    match args.as_slice() {
        [one] if one.contains(' ') && !Path::new(one).exists() => {
            one.split_whitespace().map(String::from).collect()
        }
        _ => args,
    }
}

fn cwd(proc_root: &Path, pid: i64) -> Option<String> {
    std::fs::read_link(proc_root.join(pid.to_string()).join("cwd"))
        .ok()
        .map(|p| p.to_string_lossy().into_owned())
}

/// The processes whose parent is `pid`, oldest first.
fn children(proc_root: &Path, pid: i64) -> Vec<i64> {
    let mut out = Vec::new();
    let Ok(dir) = std::fs::read_dir(proc_root) else {
        return out;
    };
    for e in dir.flatten() {
        let Some(child) = e.file_name().to_str().and_then(|n| n.parse::<i64>().ok()) else {
            continue;
        };
        let parent = read_proc(proc_root, child, "stat").and_then(|s| {
            // "pid (comm) state ppid ...": comm may hold spaces and ")".
            let rest = s.rsplit_once(')')?.1.to_string();
            rest.split_whitespace().nth(1)?.parse::<i64>().ok()
        });
        if parent == Some(pid) {
            out.push(child);
        }
    }
    out.sort();
    out
}

fn program(arg0: &str) -> &str {
    Path::new(arg0)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(arg0)
}

/// A terminal's shell, its folder, and an editor running in it.
pub fn terminal_work(proc_root: &Path, pid: i64) -> (Option<String>, Vec<String>) {
    let Some(shell) = children(proc_root, pid).into_iter().next() else {
        return (cwd(proc_root, pid), Vec::new());
    };
    let folder = cwd(proc_root, shell);
    // The terminal may run the editor itself, or a shell that runs it.
    for child in std::iter::once(shell).chain(children(proc_root, shell)) {
        let cmd = cmdline(proc_root, child);
        if cmd.first().is_some_and(|c| EDITORS.contains(&program(c))) {
            return (cwd(proc_root, child).or(folder), cmd);
        }
    }
    (folder, Vec::new())
}

/// The top of the git repository holding `folder`, if any.
pub fn git_top(folder: &str) -> Option<String> {
    let mut p = Path::new(folder);
    loop {
        if p.join(".git").exists() {
            return Some(p.to_string_lossy().into_owned());
        }
        p = p.parent()?;
    }
}

/// Windows FRIDAY doesn't keep: its own, and special workspaces.
fn kept(class: &str, workspace: &str) -> bool {
    let c = class.to_lowercase();
    !(c.is_empty()
        || c.contains("quickshell")
        || c.contains("friday")
        || workspace.starts_with("special"))
}

/// The windows open now, as a workspace called `name`.
pub fn capture(hypr: &dyn Hypr, proc_root: &Path, name: &str, now: i64) -> Result<Saved, String> {
    if key(name).is_empty() {
        return Err(format!(
            "\"{}\" isn't a name FRIDAY can keep: use a word or two, like \"site\"",
            name.trim()
        ));
    }
    let clients: Vec<serde_json::Value> =
        serde_json::from_str(&hypr.clients()?).map_err(|e| format!("hyprctl clients: {e}"))?;
    let mut windows = Vec::new();
    for c in &clients {
        let class = c["class"].as_str().unwrap_or_default().to_string();
        let workspace = c["workspace"]["name"]
            .as_str()
            .map(String::from)
            .or_else(|| c["workspace"]["id"].as_i64().map(|i| i.to_string()))
            .unwrap_or_default();
        if !kept(&class, &workspace) {
            continue;
        }
        let pid = c["pid"].as_i64().unwrap_or(0);
        let pair = |k: &str| {
            [
                c[k][0].as_i64().unwrap_or(0) as i32,
                c[k][1].as_i64().unwrap_or(0) as i32,
            ]
        };
        let terminal = TERMINALS
            .iter()
            .any(|(t, _, _)| class.eq_ignore_ascii_case(t));
        let (cwd, run) = if terminal && pid > 0 {
            terminal_work(proc_root, pid)
        } else {
            (None, Vec::new())
        };
        windows.push(Window {
            class,
            title: c["title"].as_str().unwrap_or_default().to_string(),
            workspace,
            floating: c["floating"].as_bool().unwrap_or(false),
            at: pair("at"),
            size: pair("size"),
            command: if pid > 0 {
                cmdline(proc_root, pid)
            } else {
                Vec::new()
            },
            cwd,
            run,
            address: c["address"].as_str().map(String::from),
        });
    }
    if windows.is_empty() {
        return Err("no windows are open to save".into());
    }
    let mut tops: HashMap<String, usize> = HashMap::new();
    for w in &windows {
        if let Some(top) = w.cwd.as_deref().and_then(git_top) {
            *tops.entry(top).or_default() += 1;
        }
    }
    let project = tops.into_iter().max_by_key(|(_, n)| *n).map(|(t, _)| t);
    Ok(Saved {
        name: name.trim().to_string(),
        saved: now,
        project,
        windows,
    })
}

pub fn save(dir: &Path, saved: &Saved) -> std::io::Result<PathBuf> {
    use std::io::Write;
    std::fs::create_dir_all(dir)?;
    let path = dir.join(format!("{}.json", key(&saved.name)));
    let tmp = path.with_extension("tmp");
    let mut opts = std::fs::OpenOptions::new();
    opts.write(true).create(true).truncate(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut opts, 0o600);
    let mut f = opts.open(&tmp)?;
    f.write_all(
        serde_json::to_string_pretty(saved)
            .unwrap_or_default()
            .as_bytes(),
    )?;
    drop(f);
    std::fs::rename(&tmp, &path)?;
    Ok(path)
}

pub fn list(dir: &Path) -> Vec<Saved> {
    let mut out: Vec<Saved> = std::fs::read_dir(dir)
        .map(|d| {
            d.flatten()
                .filter(|e| e.path().extension().is_some_and(|x| x == "json"))
                .filter_map(|e| std::fs::read_to_string(e.path()).ok())
                .filter_map(|t| serde_json::from_str(&t).ok())
                .collect()
        })
        .unwrap_or_default();
    out.sort_by(|a: &Saved, b| a.name.cmp(&b.name));
    out
}

/// The workspace a phrase means: its name, or its project folder's name
/// ("FRIDAY" finds the workspace whose project is `…/project-friday`).
pub fn find(dir: &Path, phrase: &str) -> Option<Saved> {
    let want = key(phrase);
    if want.is_empty() {
        return None;
    }
    let all = list(dir);
    all.iter()
        .find(|s| key(&s.name) == want)
        .or_else(|| {
            all.iter().find(|s| {
                s.project.as_deref().is_some_and(|p| {
                    let folder = key(program(p));
                    folder == want || folder.split('-').any(|w| w == want)
                })
            })
        })
        .cloned()
}

/// One argument quoted for a shell, then the whole line for a Lua string.
fn shell_line(args: &[String]) -> String {
    args.iter()
        .map(|a| {
            if !a.is_empty()
                && a.chars()
                    .all(|c| c.is_ascii_alphanumeric() || "-_./=:@+,".contains(c))
            {
                a.clone()
            } else {
                format!("'{}'", a.replace('\'', r"'\''"))
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn lua_string(s: &str) -> String {
    format!(
        "\"{}\"",
        s.replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace('\n', " ")
    )
}

/// How to start a saved window again; `None` when FRIDAY can't tell.
fn launch_line(w: &Window) -> Option<String> {
    let terminal = TERMINALS
        .iter()
        .find(|(t, _, _)| w.class.eq_ignore_ascii_case(t));
    match (terminal, &w.cwd) {
        (Some((_, program_name, flag)), Some(dir)) => {
            // The terminal's own program as started (it may live elsewhere
            // than PATH), unless something else started it (`sh -c kitty`).
            let term = w
                .command
                .first()
                .filter(|c| program(c) == *program_name)
                .cloned()
                .unwrap_or_else(|| program_name.to_string());
            let mut line = format!(
                "{} {flag} {}",
                shell_line(&[term]),
                shell_line(std::slice::from_ref(dir))
            );
            if !w.run.is_empty() {
                let sep = if w.class.eq_ignore_ascii_case("kitty")
                    || w.class.eq_ignore_ascii_case("foot")
                {
                    ""
                } else {
                    " -e"
                };
                line.push_str(&format!("{sep} {}", shell_line(&w.run)));
            }
            Some(line)
        }
        _ if !w.command.is_empty() => {
            let mut command = w.command.clone();
            if CHROMIUMS.contains(&program(&command[0])) {
                // Its last session brings the tabs back; the pages it was
                // started with would open a second time.
                let program = command.remove(0);
                command.retain(|a| a.starts_with('-') && a != "--restore-last-session");
                command.insert(0, program);
                command.push("--restore-last-session".into());
            }
            Some(shell_line(&command))
        }
        _ => None,
    }
}

/// What a restore did.
#[derive(Debug, Default, PartialEq)]
pub struct Report {
    pub moved: Vec<String>,
    pub started: Vec<String>,
    pub already: Vec<String>,
    pub failed: Vec<(String, String)>,
}

impl Report {
    pub fn text(&self, saved: &Saved) -> String {
        let mut out = format!("Workspace \"{}\"", saved.name);
        if let Some(p) = &saved.project {
            out.push_str(&format!(" ({p})"));
        }
        out.push_str(": ");
        let mut parts = Vec::new();
        // Launched, not yet seen: a window can take a few seconds (or fail
        // to come up), so this says "opening" rather than "started".
        if !self.started.is_empty() {
            parts.push(format!("opening {}", self.started.join(", ")));
        }
        if !self.moved.is_empty() {
            parts.push(format!("moved {}", self.moved.join(", ")));
        }
        if !self.already.is_empty() {
            parts.push(format!("{} already in place", self.already.len()));
        }
        if parts.is_empty() {
            parts.push("nothing to do".into());
        }
        out.push_str(&parts.join("; "));
        out.push('.');
        for (what, why) in &self.failed {
            out.push_str(&format!("\nCouldn't bring back {what}: {why}."));
        }
        out
    }
}

fn label(w: &Window) -> String {
    match (&w.cwd, w.run.first()) {
        (Some(dir), Some(ed)) => format!("{} in {} ({})", w.class, program(dir), program(ed)),
        (Some(dir), None) => format!("{} in {}", w.class, program(dir)),
        _ => w.class.clone(),
    }
}

/// Puts a saved workspace back: open windows go to their workspace, missing
/// ones are started there.
pub fn restore(hypr: &dyn Hypr, proc_root: &Path, saved: &Saved) -> Result<Report, String> {
    let now = match capture(hypr, proc_root, "now", 0) {
        Ok(s) => s.windows,
        Err(e) if e.contains("no windows") => Vec::new(),
        Err(e) => return Err(e),
    };
    let mut used = vec![false; now.len()];
    let mut report = Report::default();
    for w in &saved.windows {
        // The same app (and for a terminal, the same folder) already open.
        let open = now.iter().enumerate().position(|(i, n)| {
            !used[i] && n.class == w.class && (w.cwd.is_none() || n.cwd == w.cwd)
        });
        if let Some(i) = open {
            used[i] = true;
            if now[i].workspace == w.workspace {
                report.already.push(label(w));
                continue;
            }
            let Some(address) = now[i].address.clone() else {
                report.already.push(label(w));
                continue;
            };
            let lua = format!(
                "hl.dsp.window.move({{ workspace = {}, follow = false, window = {} }})",
                lua_string(&w.workspace),
                lua_string(&format!("address:{address}"))
            );
            match hypr.eval(&lua) {
                Ok(()) => report.moved.push(label(w)),
                Err(e) => report.failed.push((label(w), e)),
            }
            continue;
        }
        let Some(line) = launch_line(w) else {
            report
                .failed
                .push((label(w), "FRIDAY doesn't know how it was started".into()));
            continue;
        };
        let mut rules = format!(
            "workspace = {}",
            lua_string(&format!("{} silent", w.workspace))
        );
        if w.floating {
            rules.push_str(&format!(
                ", float = true, move = {}, size = {}",
                lua_string(&format!("{} {}", w.at[0], w.at[1])),
                lua_string(&format!("{} {}", w.size[0], w.size[1]))
            ));
        }
        match hypr.eval(&format!(
            "hl.exec_cmd({}, {{ {rules} }})",
            lua_string(&line)
        )) {
            Ok(()) => report.started.push(label(w)),
            Err(e) => report.failed.push((label(w), e)),
        }
    }
    Ok(report)
}

// Workspaces are Hyprland-only, and the tests build a fake /proc.
#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::sync::Mutex;

    struct FakeHypr {
        clients: String,
        calls: Mutex<Vec<String>>,
    }

    impl Hypr for FakeHypr {
        fn clients(&self) -> Result<String, String> {
            Ok(self.clients.clone())
        }
        fn eval(&self, lua: &str) -> Result<(), String> {
            self.calls.lock().unwrap().push(lua.to_string());
            Ok(())
        }
    }

    fn tmp() -> PathBuf {
        let d = std::env::temp_dir().join(format!("friday-ws-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    /// A fake /proc: kitty (100) → zsh (101, in the repo) → nvim (102).
    fn fake_proc(repo: &Path) -> PathBuf {
        let p = tmp();
        let proc_entry = |pid: i64, ppid: i64, cmd: &[&str], cwd: &Path| {
            let d = p.join(pid.to_string());
            std::fs::create_dir_all(&d).unwrap();
            std::fs::write(d.join("cmdline"), cmd.join("\0") + "\0").unwrap();
            std::fs::write(d.join("stat"), format!("{pid} (x y) S {ppid} 0 0")).unwrap();
            std::os::unix::fs::symlink(cwd, d.join("cwd")).unwrap();
        };
        proc_entry(100, 1, &["kitty"], Path::new("/"));
        proc_entry(101, 100, &["-zsh"], repo);
        proc_entry(102, 101, &["nvim", "src/main.rs"], repo);
        proc_entry(200, 1, &["/usr/lib/firefox/firefox"], Path::new("/"));
        p
    }

    fn clients() -> String {
        r#"[
          {"address":"0xa1","class":"kitty","title":"nvim","workspace":{"id":2,"name":"2"},"pid":100,"floating":false,"at":[0,0],"size":[800,600]},
          {"address":"0xa2","class":"firefox","title":"Docs","workspace":{"id":3,"name":"3"},"pid":200,"floating":true,"at":[10,20],"size":[640,480]},
          {"address":"0xa3","class":"quickshell","title":"bar","workspace":{"id":1,"name":"1"},"pid":0}
        ]"#
        .to_string()
    }

    #[test]
    fn a_workspace_keeps_the_work_not_just_the_windows() {
        let repo = tmp().join("project-friday");
        std::fs::create_dir_all(repo.join(".git")).unwrap();
        let proc_root = fake_proc(&repo);
        let hypr = FakeHypr {
            clients: clients(),
            calls: Mutex::default(),
        };
        let saved = capture(&hypr, &proc_root, "My FRIDAY workspace", 7).unwrap();
        assert_eq!(saved.windows.len(), 2, "FRIDAY's own bar isn't kept");
        let term = &saved.windows[0];
        assert_eq!(term.cwd.as_deref(), Some(repo.to_str().unwrap()));
        assert_eq!(term.run, vec!["nvim", "src/main.rs"]);
        assert_eq!(saved.project.as_deref(), Some(repo.to_str().unwrap()));

        let dir = tmp();
        let path = save(&dir, &saved).unwrap();
        assert!(path.ends_with("friday.json"), "{path:?}");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
        assert_eq!(find(&dir, "friday").unwrap().name, "My FRIDAY workspace");
        assert_eq!(find(&dir, "FRIDAY").unwrap().windows.len(), 2);
        assert!(find(&dir, "garden").is_none());
    }

    #[test]
    fn restore_starts_what_is_missing_where_it_was_and_moves_the_rest() {
        let repo = tmp().join("site");
        std::fs::create_dir_all(&repo).unwrap();
        let proc_root = fake_proc(&repo);
        let saved = capture(
            &FakeHypr {
                clients: clients(),
                calls: Mutex::default(),
            },
            &proc_root,
            "site",
            1,
        )
        .unwrap();

        // Nothing open: both are started on their workspaces.
        let empty = FakeHypr {
            clients: "[]".into(),
            calls: Mutex::default(),
        };
        let report = restore(&empty, &proc_root, &saved).unwrap();
        let calls = empty.calls.lock().unwrap().clone();
        assert_eq!(calls.len(), 2, "{calls:?}");
        assert!(
            calls[0].starts_with("hl.exec_cmd(\"kitty --directory "),
            "{calls:?}"
        );
        assert!(
            calls[0].contains(" nvim src/main.rs\", { workspace = \"2 silent\" })"),
            "{calls:?}"
        );
        assert!(calls[1].contains("/usr/lib/firefox/firefox"), "{calls:?}");
        assert!(
            calls[1].contains("float = true, move = \"10 20\", size = \"640 480\""),
            "{calls:?}"
        );
        assert_eq!(report.started.len(), 2);
        assert!(
            report
                .text(&saved)
                .contains("opening kitty in site (nvim), firefox"),
            "{}",
            report.text(&saved)
        );

        // Firefox open on another workspace: moved, not started twice.
        let moved_ff = clients().replace(r#""id":3,"name":"3""#, r#""id":5,"name":"5""#);
        let open = FakeHypr {
            clients: moved_ff,
            calls: Mutex::default(),
        };
        let report = restore(&open, &proc_root, &saved).unwrap();
        let calls = open.calls.lock().unwrap().clone();
        assert_eq!(
            calls,
            vec![
                r#"hl.dsp.window.move({ workspace = "3", follow = false, window = "address:0xa2" })"#
            ]
        );
        assert_eq!(report.already.len(), 1);
    }

    #[test]
    fn an_editor_the_terminal_runs_itself_is_kept() {
        let p = tmp();
        let repo = tmp();
        for (pid, ppid, cmd) in [(300, 1, "foot"), (301, 300, "hx\0notes.md")] {
            let d = p.join(pid.to_string());
            std::fs::create_dir_all(&d).unwrap();
            std::fs::write(d.join("cmdline"), format!("{cmd}\0")).unwrap();
            std::fs::write(d.join("stat"), format!("{pid} (x) S {ppid} 0")).unwrap();
            std::os::unix::fs::symlink(&repo, d.join("cwd")).unwrap();
        }
        let (dir, run) = terminal_work(&p, 300);
        assert_eq!(dir.as_deref(), Some(repo.to_str().unwrap()));
        assert_eq!(run, vec!["hx", "notes.md"]);
    }

    #[test]
    fn two_alike_windows_each_go_to_their_own_workspace() {
        let repo = tmp();
        let proc_root = tmp();
        for (pid, dir) in [(400, "site"), (402, "docs")] {
            for (p, ppid, cmd, cwd) in [
                (pid, 1, "kitty".to_string(), "/".to_string()),
                (
                    pid + 1,
                    pid,
                    "-zsh".to_string(),
                    repo.join(dir).to_string_lossy().into_owned(),
                ),
            ] {
                let d = proc_root.join(p.to_string());
                std::fs::create_dir_all(&d).unwrap();
                std::fs::create_dir_all(&cwd).unwrap();
                std::fs::write(d.join("cmdline"), format!("{cmd}\0")).unwrap();
                std::fs::write(d.join("stat"), format!("{p} (x) S {ppid} 0")).unwrap();
                std::os::unix::fs::symlink(&cwd, d.join("cwd")).unwrap();
            }
        }
        let win = |addr: &str, pid: i64, ws: &str| {
            format!(
                r#"{{"address":"{addr}","class":"kitty","title":"zsh","workspace":{{"id":{ws},"name":"{ws}"}},"pid":{pid}}}"#
            )
        };
        let saved_clients = format!("[{},{}]", win("0x1", 400, "2"), win("0x2", 402, "3"));
        let saved = capture(
            &FakeHypr {
                clients: saved_clients,
                calls: Mutex::default(),
            },
            &proc_root,
            "two",
            1,
        )
        .unwrap();
        // Both now sit on workspace 9, listed in the other order.
        let now = format!("[{},{}]", win("0x2", 402, "9"), win("0x1", 400, "9"));
        let hypr = FakeHypr {
            clients: now,
            calls: Mutex::default(),
        };
        restore(&hypr, &proc_root, &saved).unwrap();
        let calls = hypr.calls.lock().unwrap().clone();
        assert_eq!(
            calls,
            vec![
                r#"hl.dsp.window.move({ workspace = "2", follow = false, window = "address:0x1" })"#,
                r#"hl.dsp.window.move({ workspace = "3", follow = false, window = "address:0x2" })"#,
            ]
        );
    }

    #[test]
    fn a_terminal_started_by_something_else_reopens_as_itself() {
        let w = Window {
            class: "Alacritty".into(),
            title: "t".into(),
            workspace: "1".into(),
            floating: false,
            at: [0, 0],
            size: [0, 0],
            command: vec!["sh".into(), "-c".into(), "alacritty".into()],
            cwd: Some("/w".into()),
            run: vec![],
            address: None,
        };
        assert_eq!(launch_line(&w).unwrap(), "alacritty --working-directory /w");
        let unread = Window {
            command: vec![],
            ..w.clone()
        };
        assert_eq!(
            launch_line(&unread).unwrap(),
            "alacritty --working-directory /w"
        );
        let own = Window {
            command: vec!["/opt/alacritty/bin/alacritty".into()],
            ..w
        };
        assert_eq!(
            launch_line(&own).unwrap(),
            "/opt/alacritty/bin/alacritty --working-directory /w"
        );
    }

    #[test]
    fn a_chromium_comes_back_with_its_tabs() {
        let w = Window {
            class: "chromium".into(),
            title: "t".into(),
            workspace: "3".into(),
            floating: false,
            at: [0, 0],
            size: [0, 0],
            command: vec![
                "/usr/lib/chromium/chromium".into(),
                "--ozone-platform=wayland".into(),
            ],
            cwd: None,
            run: vec![],
            address: None,
        };
        assert_eq!(
            launch_line(&w).unwrap(),
            "/usr/lib/chromium/chromium --ozone-platform=wayland --restore-last-session"
        );
        let with_pages = Window {
            command: vec![
                "chromium".into(),
                "--incognito".into(),
                "file:///tmp/a.html".into(),
            ],
            ..w.clone()
        };
        assert_eq!(
            launch_line(&with_pages).unwrap(),
            "chromium --incognito --restore-last-session"
        );
        let firefox = Window {
            command: vec!["firefox".into()],
            ..w
        };
        assert_eq!(launch_line(&firefox).unwrap(), "firefox");
    }

    #[test]
    fn a_rewritten_process_title_is_split_back_into_arguments() {
        let p = tmp();
        let d = p.join("500");
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(
            d.join("cmdline"),
            "/usr/lib/chromium/chromium --ozone-platform=wayland --user-data-dir=/tmp/x\0",
        )
        .unwrap();
        assert_eq!(
            cmdline(&p, 500),
            vec![
                "/usr/lib/chromium/chromium",
                "--ozone-platform=wayland",
                "--user-data-dir=/tmp/x"
            ]
        );
    }

    #[test]
    fn a_name_with_no_words_is_refused() {
        let hypr = FakeHypr {
            clients: clients(),
            calls: Mutex::default(),
        };
        for name in ["workspace", "my workspace", "!!"] {
            assert!(
                capture(&hypr, Path::new("/nonexistent"), name, 1).is_err(),
                "{name}"
            );
        }
    }

    #[test]
    fn odd_names_and_arguments_stay_quoted() {
        assert_eq!(key("My Rust Learning workspace"), "rust-learning");
        let w = Window {
            class: "org.gnome.TextEditor".into(),
            title: "t".into(),
            workspace: "1".into(),
            floating: false,
            at: [0, 0],
            size: [0, 0],
            command: vec![
                "gnome-text-editor".into(),
                "my \"notes\"; rm -rf ~.txt".into(),
            ],
            cwd: None,
            run: vec![],
            address: None,
        };
        let line = launch_line(&w).unwrap();
        assert_eq!(line, r#"gnome-text-editor 'my "notes"; rm -rf ~.txt'"#);
        assert_eq!(
            lua_string(&line),
            r#""gnome-text-editor 'my \"notes\"; rm -rf ~.txt'""#
        );
        let unknown = Window {
            command: vec![],
            ..w
        };
        assert!(launch_line(&unknown).is_none());
    }
}
