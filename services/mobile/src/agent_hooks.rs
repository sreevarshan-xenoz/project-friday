//! FRIDAY in coding-agent sessions the person started themselves: Claude
//! Code calls `friday-ctl agent-hook claude` before it shows a permission
//! prompt, and FRIDAY approves the step when its rules allow it (the same
//! rules as [`crate::agent_policy`], with the session's folder as the
//! project). Anything else is left alone: Claude Code shows its own prompt
//! in the terminal as usual, and FRIDAY sends a notification so the person
//! knows it is waiting. Every decision is logged to `agents/hooks.jsonl`.
//!
//! `friday-ctl agent-hooks on|off` adds or removes FRIDAY's entry in Claude
//! Code's settings (`~/.claude/settings.json`), leaving everything else in
//! that file as it was. It is on by default: the daemon adds it once, unless
//! the person turned it off (`FRIDAY_AGENT_HOOKS=0`).

use std::path::{Path, PathBuf};

use serde_json::{json, Value};

use crate::agent_policy::{self, Decision};

/// What marks FRIDAY's own hook entry, so it can be found and removed.
const MARK: &str = "agent-hook claude";

/// Whether FRIDAY answers in the person's own sessions: on unless they
/// turned it off (`FRIDAY_AGENT_HOOKS=0`).
pub fn enabled() -> bool {
    std::env::var("FRIDAY_AGENT_HOOKS").map_or(true, |v| v != "0")
}

fn home() -> Option<PathBuf> {
    std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" }).map(PathBuf::from)
}

/// Claude Code's own settings file.
pub fn claude_settings() -> Option<PathBuf> {
    Some(home()?.join(".claude").join("settings.json"))
}

/// A folder too broad to treat as one project: the home folder, or one
/// near the top of the disk. Nothing is approved for a session there.
pub fn too_broad(folder: &Path) -> bool {
    let normal = |p: &Path| std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
    let folder = normal(folder);
    let home = home().map(|h| normal(&h));
    if home.as_ref().is_some_and(|h| *h == folder) || folder.components().count() < 3 {
        return true;
    }
    // A hidden folder in the home folder (~/.ssh, ~/.config) holds settings
    // and keys, not a project.
    if let Some(inside) = home.as_ref().and_then(|h| folder.strip_prefix(h).ok()) {
        if inside
            .components()
            .any(|c| c.as_os_str().to_string_lossy().starts_with('.'))
        {
            return true;
        }
    }
    // The system's own folders.
    let text = folder.to_string_lossy().to_lowercase().replace('\\', "/");
    let text = text.trim_start_matches("//?/");
    [
        "/etc",
        "/usr",
        "/var",
        "/boot",
        "/root",
        "/opt",
        "/srv/www",
        "/sys",
        "/proc",
        "/dev",
        "/run",
        "c:/windows",
        "c:/program files",
        "c:/programdata",
        "c:/users",
    ]
    .iter()
    .any(|sys| text == *sys || text.starts_with(&format!("{sys}/")))
        && !home.as_ref().is_some_and(|h| folder.starts_with(h))
}

/// FRIDAY's answer to a Claude Code `PermissionRequest` hook: the JSON to
/// print to approve, or `None` to leave the prompt to the person.
pub fn claude_permission(input: &Value, log_dir: &Path) -> Option<Value> {
    let tool = input["tool_name"].as_str().unwrap_or("");
    let tool_input = &input["tool_input"];
    let folder = PathBuf::from(input["cwd"].as_str().unwrap_or(""));
    let step = crate::agent_claude::step_from_claude(tool, tool_input);
    let what: String = step
        .command
        .clone()
        .or_else(|| step.paths.first().cloned())
        .unwrap_or_else(|| tool.to_string())
        .chars()
        .take(200)
        .collect();
    let decision = if folder.as_os_str().is_empty() || too_broad(&folder) {
        Decision::Ask("the session isn't in a project folder".into())
    } else if tool == "AskUserQuestion" || tool == "ExitPlanMode" {
        Decision::Ask("it's a question for you".into())
    } else {
        agent_policy::decide(&step, &folder)
    };
    log(
        log_dir,
        json!({"agent": "claude", "folder": folder, "step": what,
            "decision": match &decision { Decision::Allow => "approved", Decision::Ask(_) => "left to you" },
            "why": match &decision { Decision::Allow => "", Decision::Ask(w) => w.as_str() }}),
    );
    match decision {
        Decision::Allow => Some(json!({"hookSpecificOutput": {
            "hookEventName": "PermissionRequest",
            "decision": {"behavior": "allow"}}})),
        Decision::Ask(why) => {
            notify(
                "Claude Code is waiting for you",
                &format!(
                    "It {why}:\n{what}\nin {}",
                    folder
                        .file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or("its terminal")
                ),
            );
            None
        }
    }
}

fn log(dir: &Path, mut event: Value) {
    use std::io::Write;
    event["ts"] = json!(chrono::Utc::now().timestamp());
    let _ = std::fs::create_dir_all(dir);
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join("hooks.jsonl"))
    {
        let _ = writeln!(f, "{event}");
    }
}

fn notify(title: &str, body: &str) {
    if cfg!(target_os = "linux") {
        let _ = std::process::Command::new("notify-send")
            .args(["--app-name=FRIDAY", title, body])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();
    }
}

/// The hook command line for `ctl` (friday-ctl's full path).
pub fn hook_command(ctl: &Path) -> String {
    format!("\"{}\" {MARK}", ctl.display())
}

fn is_ours(entry: &Value) -> bool {
    entry["hooks"]
        .as_array()
        .into_iter()
        .flatten()
        .any(|h| h["command"].as_str().is_some_and(|c| c.contains(MARK)))
}

/// Adds FRIDAY's `PermissionRequest` hook to Claude Code's settings, keeping
/// the rest of the file. Returns whether anything changed.
pub fn install_claude(settings: &Path, ctl: &Path) -> Result<bool, String> {
    let mut doc: Value = match std::fs::read(settings) {
        Ok(b) => serde_json::from_slice(&b).map_err(|e| {
            format!(
                "{} isn't valid JSON, so FRIDAY left it alone: {e}",
                settings.display()
            )
        })?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => json!({}),
        Err(e) => return Err(e.to_string()),
    };
    if !doc.is_object() {
        return Err(format!("{} isn't a settings object", settings.display()));
    }
    let hooks = doc
        .as_object_mut()
        .unwrap()
        .entry("hooks")
        .or_insert_with(|| json!({}));
    let Some(hooks) = hooks.as_object_mut() else {
        return Err("its \"hooks\" isn't an object".into());
    };
    let list = hooks
        .entry("PermissionRequest")
        .or_insert_with(|| json!([]));
    let Some(list) = list.as_array_mut() else {
        return Err("its PermissionRequest hooks aren't a list".into());
    };
    if list.iter().any(is_ours) {
        return Ok(false);
    }
    list.push(json!({"matcher": "*", "hooks": [
        {"type": "command", "command": hook_command(ctl), "timeout": 15}]}));
    write(settings, &doc)?;
    Ok(true)
}

/// Removes FRIDAY's hook from Claude Code's settings, and nothing else.
pub fn uninstall_claude(settings: &Path) -> Result<bool, String> {
    let Ok(bytes) = std::fs::read(settings) else {
        return Ok(false);
    };
    let mut doc: Value = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
    let Some(list) = doc
        .pointer_mut("/hooks/PermissionRequest")
        .and_then(Value::as_array_mut)
    else {
        return Ok(false);
    };
    // Only FRIDAY's own hook goes: an entry that also holds someone else's
    // keeps theirs.
    let mut changed = false;
    for entry in list.iter_mut() {
        if let Some(hooks) = entry["hooks"].as_array_mut() {
            let before = hooks.len();
            hooks.retain(|h| !h["command"].as_str().is_some_and(|c| c.contains(MARK)));
            changed |= hooks.len() != before;
        }
    }
    if !changed {
        return Ok(false);
    }
    list.retain(|e| e["hooks"].as_array().is_none_or(|h| !h.is_empty()));
    if list.is_empty() {
        doc["hooks"]
            .as_object_mut()
            .unwrap()
            .remove("PermissionRequest");
        if doc["hooks"].as_object().is_some_and(|h| h.is_empty()) {
            doc.as_object_mut().unwrap().remove("hooks");
        }
    }
    write(settings, &doc)?;
    Ok(true)
}

/// Whether FRIDAY's hook is in Claude Code's settings.
pub fn installed_claude(settings: &Path) -> bool {
    std::fs::read(settings)
        .ok()
        .and_then(|b| serde_json::from_slice::<Value>(&b).ok())
        .and_then(|d| d.pointer("/hooks/PermissionRequest").cloned())
        .and_then(|l| l.as_array().cloned())
        .is_some_and(|l| l.iter().any(is_ours))
}

fn write(path: &Path, doc: &Value) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let tmp = path.with_extension("json.friday-tmp");
    let mut text = serde_json::to_string_pretty(doc).map_err(|e| e.to_string())?;
    text.push('\n');
    std::fs::write(&tmp, text).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, path).map_err(|e| e.to_string())
}

/// Adds the hook once, the first time the daemon runs with this on, so the
/// default is on; after that the person's `friday agents hooks off` stands.
pub fn install_once(state_dir: &Path, ctl: &Path) {
    let marker = state_dir.join("agent-hooks-offered");
    if !enabled() || marker.exists() || !ctl.is_file() {
        return;
    }
    let Some(settings) = claude_settings() else {
        return;
    };
    // Only for people who use Claude Code.
    if !settings.parent().is_some_and(Path::is_dir) {
        return;
    }
    match install_claude(&settings, ctl) {
        Ok(_) => {
            let _ = std::fs::write(&marker, "");
            tracing::info!("agent hooks: FRIDAY answers routine Claude Code approvals (friday agents hooks off to stop)");
        }
        Err(e) => tracing::warn!("agent hooks: couldn't add FRIDAY's Claude Code hook: {e}"),
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    struct TempDir(PathBuf);

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn temp(name: &str) -> TempDir {
        let d = std::env::temp_dir().join(format!("friday-hooks-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        TempDir(d)
    }

    fn request(tool: &str, input: Value, cwd: &str) -> Value {
        json!({"session_id": "s", "cwd": cwd, "hook_event_name": "PermissionRequest",
            "tool_name": tool, "tool_input": input})
    }

    #[test]
    fn safe_steps_are_approved_and_the_rest_left_to_the_person() {
        let logs = temp("decide");
        let site = logs.0.join("code").join("site");
        std::fs::create_dir_all(&site).unwrap();
        let cwd = site.to_str().unwrap();
        let ok = claude_permission(
            &request("Bash", json!({"command": "cargo test"}), cwd),
            &logs.0,
        );
        assert_eq!(
            ok.unwrap()["hookSpecificOutput"]["decision"]["behavior"],
            "allow"
        );
        assert!(claude_permission(
            &request("Bash", json!({"command": "git push"}), cwd),
            &logs.0
        )
        .is_none());
        assert!(claude_permission(&request("AskUserQuestion", json!({}), cwd), &logs.0).is_none());
        // A session in the home folder (or near the top of the disk) gets nothing.
        assert!(
            claude_permission(&request("Bash", json!({"command": "ls"}), "/"), &logs.0).is_none()
        );
        let log = std::fs::read_to_string(logs.0.join("hooks.jsonl")).unwrap();
        assert_eq!(log.lines().count(), 4);
        assert!(log.contains(r#""decision":"approved""#) && log.contains("left to you"));
    }

    #[test]
    fn install_keeps_the_persons_settings_and_uninstall_removes_only_ours() {
        let d = temp("install");
        let settings = d.0.join("settings.json");
        let theirs = json!({"model": "opus", "hooks": {"PermissionRequest": [
            {"matcher": "Bash", "hooks": [{"type": "command", "command": "their-hook"}]}],
            "Stop": [{"hooks": [{"type": "command", "command": "say done"}]}]}});
        std::fs::write(&settings, theirs.to_string()).unwrap();
        let ctl = Path::new("/usr/bin/friday-ctl");
        assert!(install_claude(&settings, ctl).unwrap());
        assert!(!install_claude(&settings, ctl).unwrap(), "installed twice");
        assert!(installed_claude(&settings));
        let doc: Value = serde_json::from_slice(&std::fs::read(&settings).unwrap()).unwrap();
        assert_eq!(doc["model"], "opus");
        assert_eq!(
            doc["hooks"]["PermissionRequest"].as_array().unwrap().len(),
            2
        );
        assert_eq!(
            doc["hooks"]["PermissionRequest"][1]["hooks"][0]["command"],
            "\"/usr/bin/friday-ctl\" agent-hook claude"
        );
        assert!(uninstall_claude(&settings).unwrap());
        let doc: Value = serde_json::from_slice(&std::fs::read(&settings).unwrap()).unwrap();
        assert_eq!(doc, theirs);
        assert!(!installed_claude(&settings));
    }

    #[test]
    fn a_settings_file_friday_cant_read_is_left_alone() {
        let d = temp("broken");
        let settings = d.0.join("settings.json");
        std::fs::write(&settings, "{ not json").unwrap();
        assert!(install_claude(&settings, Path::new("/usr/bin/friday-ctl")).is_err());
        assert_eq!(std::fs::read_to_string(&settings).unwrap(), "{ not json");
        // A missing file is made with just the hook.
        let fresh = d.0.join("new").join("settings.json");
        assert!(install_claude(&fresh, Path::new("/usr/bin/friday-ctl")).unwrap());
        assert!(uninstall_claude(&fresh).unwrap());
        assert_eq!(std::fs::read_to_string(&fresh).unwrap().trim(), "{}");
    }

    #[test]
    fn broad_folders_are_never_a_project() {
        assert!(too_broad(Path::new("/")));
        assert!(too_broad(Path::new("/home")));
        assert!(too_broad(Path::new("/etc/nginx")));
        assert!(too_broad(Path::new("/usr/local/bin")));
        assert!(!too_broad(Path::new("/srv/code/site")));
        let home = home().unwrap();
        assert!(too_broad(&home.join(".ssh")));
        assert!(too_broad(&home.join(".config").join("app")));
        assert!(!too_broad(&home.join("code").join("site")));
    }

    #[test]
    fn uninstall_keeps_a_hook_that_shares_friday_s_entry() {
        let d = temp("shared");
        let settings = d.0.join("settings.json");
        std::fs::write(
            &settings,
            json!({"hooks": {"PermissionRequest": [{"matcher": "*", "hooks": [
                {"type": "command", "command": "\"/usr/bin/friday-ctl\" agent-hook claude"},
                {"type": "command", "command": "their-audit-hook"}]}]}})
            .to_string(),
        )
        .unwrap();
        assert!(uninstall_claude(&settings).unwrap());
        let doc: Value = serde_json::from_slice(&std::fs::read(&settings).unwrap()).unwrap();
        assert_eq!(
            doc["hooks"]["PermissionRequest"][0]["hooks"],
            json!([{"type": "command", "command": "their-audit-hook"}])
        );
    }
}
