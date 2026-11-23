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

use crate::agent_policy::{self, Decision, Kind};

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

/// Decides a step in a session the person started, logs it, and tells the
/// person when it is left to them. `label` names the agent for them.
pub fn decide_session_step(
    label: &str,
    step: &agent_policy::Step,
    folder: &Path,
    question: bool,
    log_dir: &Path,
) -> Decision {
    let what: String = step
        .command
        .clone()
        .or_else(|| step.paths.first().cloned())
        .unwrap_or_else(|| "a step".into())
        .chars()
        .take(200)
        .collect();
    // A search with no path searches the session's folder; a read or edit
    // must say which file.
    let names_nothing = step.paths.is_empty()
        && matches!(
            step.kind,
            agent_policy::Kind::Read | agent_policy::Kind::Edit
        );
    let decision = if folder.as_os_str().is_empty() || too_broad(folder) {
        Decision::Ask("the session isn't in a project folder".into())
    } else if names_nothing {
        // A read or edit whose file FRIDAY can't see can't be checked.
        Decision::Ask("doesn't say which file it touches".into())
    } else if question {
        Decision::Ask("it's a question for you".into())
    } else {
        agent_policy::decide(step, folder)
    };
    log(
        log_dir,
        json!({"agent": label, "folder": folder, "step": what,
            "decision": match &decision { Decision::Allow => "approved", Decision::Ask(_) => "left to you", Decision::Deny(_) => "refused" },
            "why": match &decision { Decision::Allow => "", Decision::Ask(w) | Decision::Deny(w) => w.as_str() }}),
    );
    if let Decision::Ask(why) = &decision {
        notify(
            &format!("{label} is waiting for you"),
            &format!(
                "It {why}:\n{what}\nin {}",
                folder
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("its terminal")
            ),
        );
    }
    decision
}

/// FRIDAY's answer to a Claude Code `PermissionRequest` hook: the JSON to
/// print to approve, or `None` to leave the prompt to the person.
pub fn claude_permission(input: &Value, log_dir: &Path) -> Option<Value> {
    let tool = input["tool_name"].as_str().unwrap_or("");
    let folder = PathBuf::from(input["cwd"].as_str().unwrap_or(""));
    let step = crate::agent_claude::step_from_claude(tool, &input["tool_input"]);
    let question = tool == "AskUserQuestion" || tool == "ExitPlanMode";
    match decide_session_step("Claude Code", &step, &folder, question, log_dir) {
        Decision::Allow => Some(json!({"hookSpecificOutput": {
            "hookEventName": "PermissionRequest",
            "decision": {"behavior": "allow"}}})),
        Decision::Deny(why) => Some(json!({"hookSpecificOutput": {
            "hookEventName": "PermissionRequest",
            "decision": {"behavior": "deny", "message": why}}})),
        Decision::Ask(_) => None,
    }
}

/// agy's tools FRIDAY knows, and what kind of step each is. Its hook only
/// fires for these, so agy's own housekeeping tools never wait on FRIDAY.
const AGY_TOOLS: [(&str, Kind); 13] = [
    ("run_command", Kind::Execute),
    ("view_file", Kind::Read),
    ("view_file_outline", Kind::Read),
    ("view_code_item", Kind::Read),
    ("list_dir", Kind::Read),
    ("grep_search", Kind::Search),
    ("find_by_name", Kind::Search),
    ("codebase_search", Kind::Search),
    ("write_to_file", Kind::Edit),
    ("replace_file_content", Kind::Edit),
    ("multi_replace_file_content", Kind::Edit),
    ("read_url_content", Kind::Fetch),
    ("search_web", Kind::Fetch),
];

/// The step an agy `PreToolUse` hook asks about.
pub fn step_from_agy(tool: &str, args: &Value) -> agent_policy::Step {
    let kind = AGY_TOOLS
        .iter()
        .find(|(name, _)| *name == tool)
        .map_or(Kind::Other, |(_, k)| *k);
    let paths = [
        "TargetFile",
        "AbsolutePath",
        "Path",
        "DirectoryPath",
        "SearchPath",
        "SearchDirectory",
        "File",
    ]
    .iter()
    .filter_map(|k| args[*k].as_str().map(str::to_string))
    .collect();
    agent_policy::Step {
        kind,
        command: args["CommandLine"].as_str().map(str::to_string),
        paths,
    }
}

/// Whether the agy that runs this hook is unattended: print mode (`agy -p`)
/// or FRIDAY's own run (`agy acp`). Nobody waits at a prompt there, and
/// FRIDAY's runs answer each step over ACP already. Linux reads the parent
/// processes; elsewhere every session counts as attended.
pub fn agy_unattended() -> bool {
    #[cfg(target_os = "linux")]
    {
        let mut pid = std::process::id();
        for _ in 0..6 {
            let Ok(stat) = std::fs::read_to_string(format!("/proc/{pid}/stat")) else {
                return false;
            };
            // The parent pid is the second field after the ")" that closes the name.
            let Some(ppid) = stat
                .rsplit_once(')')
                .and_then(|(_, rest)| rest.split_whitespace().nth(1))
                .and_then(|p| p.parse::<u32>().ok())
            else {
                return false;
            };
            if ppid <= 1 {
                return false;
            }
            pid = ppid;
            let cmdline = std::fs::read(format!("/proc/{pid}/cmdline")).unwrap_or_default();
            let args: Vec<String> = cmdline
                .split(|b| *b == 0)
                .map(|a| String::from_utf8_lossy(a).into_owned())
                .collect();
            let is_agy = args
                .first()
                .is_some_and(|a| Path::new(a).file_name().is_some_and(|n| n == "agy"));
            if is_agy {
                return agy_args_unattended(&args);
            }
        }
        false
    }
    #[cfg(not(target_os = "linux"))]
    false
}

/// Whether agy's command line runs it unattended: the `acp` subcommand, or a
/// print flag. Only agy's own subcommand and flags count, never words of the
/// person's prompt.
pub fn agy_args_unattended(args: &[String]) -> bool {
    args.get(1).is_some_and(|a| a == "acp")
        || args.iter().skip(1).any(|a| {
            ["-p", "--print", "--prompt"].contains(&a.as_str())
                || a.starts_with("--print=")
                || a.starts_with("--prompt=")
        })
}

/// FRIDAY's answer to an agy `PreToolUse` hook: allow it, or hold a risky
/// step for the person.
pub fn agy_pre_tool(input: &Value, log_dir: &Path) -> Value {
    if agy_unattended() {
        // agy decides as it would without FRIDAY.
        return json!({"decision": "ask"});
    }
    let call = &input["toolCall"];
    let tool = call["name"].as_str().unwrap_or("");
    let folder = PathBuf::from(input["cwd"].as_str().unwrap_or(""));
    let step = step_from_agy(tool, &call["args"]);
    match decide_session_step("agy", &step, &folder, false, log_dir) {
        Decision::Allow => json!({"decision": "allow"}),
        Decision::Deny(why) => json!({"decision": "deny", "reason": why}),
        // agy goes ahead on "ask" when the person lets it run on its own
        // (auto, always-proceed), so FRIDAY holds the step and asks itself.
        Decision::Ask(why) => {
            let what = step
                .command
                .clone()
                .or_else(|| step.paths.first().cloned())
                .unwrap_or_else(|| tool.to_string());
            if wait_for_person(
                crate::agent_runs::Agent::Agy,
                &folder,
                &what,
                &why,
                log_dir,
                PERSON_WAIT,
            ) {
                json!({"decision": "allow"})
            } else {
                json!({"decision": "deny", "reason": format!(
                    "The person didn't approve this step (it {why}). Don't retry it; tell them what you wanted to do and why."
                )})
            }
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
    // A settings file that is a link (a dotfiles manager's) is written
    // through, not replaced.
    let real = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let path = real.as_path();
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
    // Only for agents the person has: their settings folder exists.
    if let Some(settings) = claude_settings().filter(|s| s.parent().is_some_and(Path::is_dir)) {
        match install_claude(&settings, ctl) {
            Ok(_) => tracing::info!("agent hooks: FRIDAY answers routine Claude Code approvals (friday agents hooks off to stop)"),
            Err(e) => tracing::warn!("agent hooks: couldn't add FRIDAY's Claude Code hook: {e}"),
        }
    }
    if let Some(hooks) =
        agy_hooks().filter(|h| h.parent().and_then(Path::parent).is_some_and(Path::is_dir))
    {
        match install_agy(&hooks, ctl) {
            Ok(_) => tracing::info!("agent hooks: FRIDAY answers routine agy approvals (friday agents hooks off to stop)"),
            Err(e) => tracing::warn!("agent hooks: couldn't add FRIDAY's agy hook: {e}"),
        }
    }
    let _ = std::fs::write(&marker, "");
}

/// agy's shared hooks file (the CLI and the app both read it).
pub fn agy_hooks() -> Option<PathBuf> {
    Some(home()?.join(".gemini").join("config").join("hooks.json"))
}

/// FRIDAY's named entry in agy's hooks file.
const AGY_NAME: &str = "friday";

fn read_object(path: &Path) -> Result<Value, String> {
    match std::fs::read(path) {
        Ok(b) => {
            let doc: Value = serde_json::from_slice(&b).map_err(|e| {
                format!(
                    "{} isn't valid JSON, so FRIDAY left it alone: {e}",
                    path.display()
                )
            })?;
            if doc.is_object() {
                Ok(doc)
            } else {
                Err(format!("{} isn't a JSON object", path.display()))
            }
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(json!({})),
        Err(e) => Err(e.to_string()),
    }
}

/// Adds FRIDAY's `PreToolUse` hook to agy's hooks file under its own name,
/// for the tools FRIDAY knows. Returns whether anything changed.
pub fn install_agy(hooks: &Path, ctl: &Path) -> Result<bool, String> {
    let mut doc = read_object(hooks)?;
    if doc.get(AGY_NAME).is_some() {
        return Ok(false);
    }
    let tools: Vec<&str> = AGY_TOOLS.iter().map(|(name, _)| *name).collect();
    doc[AGY_NAME] = json!({"PreToolUse": [{
        "matcher": format!("^({})$", tools.join("|")),
        "hooks": [{"type": "command", "command": format!("\"{}\" agent-hook agy", ctl.display()),
            // A risky step waits up to PERSON_WAIT for the person.
            "timeout": PERSON_WAIT.as_secs() + 30}]}]});
    write(hooks, &doc)?;
    Ok(true)
}

/// Removes FRIDAY's entry from agy's hooks file, and nothing else.
pub fn uninstall_agy(hooks: &Path) -> Result<bool, String> {
    if !hooks.is_file() {
        return Ok(false);
    }
    let mut doc = read_object(hooks)?;
    if doc.as_object_mut().unwrap().remove(AGY_NAME).is_none() {
        return Ok(false);
    }
    write(hooks, &doc)?;
    Ok(true)
}

/// Whether FRIDAY's entry is in agy's hooks file.
pub fn installed_agy(hooks: &Path) -> bool {
    read_object(hooks).is_ok_and(|d| d.get(AGY_NAME).is_some())
}

/// How long a step in the person's own session waits for their answer.
pub const PERSON_WAIT: std::time::Duration = std::time::Duration::from_secs(300);

/// Holds a step from the person's own session until they answer it, the
/// same way a step of FRIDAY's own run waits: it shows in `friday agents`
/// (and on Windows as the tray's prompt) until `friday agents allow|deny`.
/// No answer in time means no.
pub fn wait_for_person(
    agent: crate::agent_runs::Agent,
    folder: &Path,
    what: &str,
    why: &str,
    dir: &Path,
    limit: std::time::Duration,
) -> bool {
    use crate::agent_runs::{Answer, Pending, Recorder, Run, Status};
    let run = Run {
        id: format!("r{}", &uuid::Uuid::new_v4().simple().to_string()[..6]),
        agent,
        folder: folder.to_path_buf(),
        request: "a step in your own session".into(),
        task: String::new(),
        status: Status::WaitingForYou,
        started: chrono::Utc::now().timestamp(),
        ended: None,
        pending: Some(Pending {
            step: what.chars().take(200).collect(),
            why: why.to_string(),
        }),
        summary: String::new(),
        approved_by_friday: 0,
        approved_by_you: 0,
        rejected: 0,
        session: None,
        follows: None,
        worktree: None,
    };
    let Ok(rec) = Recorder::new(dir, run) else {
        return false;
    };
    let id = rec.run.id.clone();
    notify(
        &format!("{} wants your OK", agent.name()),
        &format!("It {why}:\n{what}\nfriday agents allow {id}  ·  friday agents deny {id}"),
    );
    let start = std::time::Instant::now();
    let allowed = loop {
        match rec.take_answer() {
            Some(Answer::Allow) => break true,
            Some(_) => break false,
            None if start.elapsed() >= limit => break false,
            None => std::thread::sleep(std::time::Duration::from_millis(500)),
        }
    };
    log(
        dir,
        json!({"agent": agent.name(), "folder": folder, "step": what,
            "decision": if allowed { "approved by you" } else { "refused" }}),
    );
    // The step is settled; only the log keeps it.
    for ext in ["json", "jsonl", "answer"] {
        let _ = std::fs::remove_file(dir.join(format!("{id}.{ext}")));
    }
    allowed
}

/// The file whose presence means the person turned FRIDAY off in their own
/// sessions; the opencode watcher checks it (Claude Code and agy lose their
/// hooks instead).
pub fn sessions_off_marker() -> PathBuf {
    crate::agent_runs::runs_dir().join("sessions-off")
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
            &request("Bash", json!({"command": "git status"}), cwd),
            &logs.0,
        );
        assert_eq!(
            ok.unwrap()["hookSpecificOutput"]["decision"]["behavior"],
            "allow"
        );
        // Never run: denied with the reason, whoever would approve.
        let never = claude_permission(
            &request("Bash", json!({"command": "rm -rf ~"}), cwd),
            &logs.0,
        )
        .unwrap();
        assert_eq!(never["hookSpecificOutput"]["decision"]["behavior"], "deny");
        assert!(never["hookSpecificOutput"]["decision"]["message"]
            .as_str()
            .unwrap()
            .contains("never runs"));
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
        assert_eq!(log.lines().count(), 5);
        assert!(log.contains(r#""decision":"approved""#) && log.contains("left to you"));
        assert!(log.contains(r#""decision":"refused""#));
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
    fn agy_hook_installs_under_its_own_name_and_leaves_the_rest() {
        let d = temp("agy");
        let hooks = d.0.join("config").join("hooks.json");
        std::fs::create_dir_all(hooks.parent().unwrap()).unwrap();
        let theirs =
            json!({"lint": {"PostToolUse": [{"matcher": "*", "hooks": [{"command": "lint.sh"}]}]}});
        std::fs::write(&hooks, theirs.to_string()).unwrap();
        let ctl = Path::new("/usr/bin/friday-ctl");
        assert!(install_agy(&hooks, ctl).unwrap());
        assert!(!install_agy(&hooks, ctl).unwrap());
        assert!(installed_agy(&hooks));
        let doc: Value = serde_json::from_slice(&std::fs::read(&hooks).unwrap()).unwrap();
        let entry = &doc["friday"]["PreToolUse"][0];
        assert!(entry["matcher"].as_str().unwrap().contains("run_command"));
        assert_eq!(
            entry["hooks"][0]["command"],
            "\"/usr/bin/friday-ctl\" agent-hook agy"
        );
        assert!(uninstall_agy(&hooks).unwrap());
        let doc: Value = serde_json::from_slice(&std::fs::read(&hooks).unwrap()).unwrap();
        assert_eq!(doc, theirs);
    }

    #[test]
    fn agy_steps_the_rules_allow_are_allowed() {
        let logs = temp("agy-allow");
        let site = logs.0.join("code").join("site");
        std::fs::create_dir_all(&site).unwrap();
        let input = json!({"cwd": site, "toolCall": {"name": "run_command", "args": {"CommandLine": "git status"}}});
        assert_eq!(agy_pre_tool(&input, &logs.0)["decision"], "allow");
        let edit = step_from_agy("write_to_file", &json!({"TargetFile": "/w/src/a.rs"}));
        assert_eq!(
            (edit.kind, edit.paths),
            (Kind::Edit, vec!["/w/src/a.rs".to_string()])
        );
        assert_eq!(step_from_agy("task_boundary", &json!({})).kind, Kind::Other);
    }

    #[test]
    fn a_held_step_answers_with_the_persons_choice() {
        let d = temp("hold");
        let dir = d.0.clone();
        let answerer = std::thread::spawn(move || {
            for _ in 0..200 {
                let waiting = std::fs::read_dir(&dir).ok().and_then(|mut e| {
                    e.find_map(|f| {
                        let p = f.ok()?.path();
                        (p.extension()? == "json")
                            .then(|| p.file_stem().unwrap().to_string_lossy().to_string())
                    })
                });
                if let Some(id) = waiting {
                    crate::agent_runs::answer(&dir, &id, true).unwrap();
                    return;
                }
                std::thread::sleep(std::time::Duration::from_millis(20));
            }
        });
        let allowed = wait_for_person(
            crate::agent_runs::Agent::Agy,
            Path::new("/w/site"),
            "rm x",
            "runs `rm x`",
            &d.0,
            std::time::Duration::from_secs(10),
        );
        answerer.join().unwrap();
        assert!(allowed);
        // Settled steps leave only the log.
        assert!(std::fs::read_dir(&d.0).unwrap().all(|f| f
            .unwrap()
            .path()
            .extension()
            .is_some_and(|x| x == "jsonl")));
    }

    #[test]
    fn only_agy_s_own_flags_make_it_unattended() {
        let args = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert!(agy_args_unattended(&args(&["agy", "-p", "fix it"])));
        assert!(agy_args_unattended(&args(&[
            "agy",
            "acp",
            "--cli_dir_name",
            "friday"
        ])));
        assert!(!agy_args_unattended(&args(&["agy", "explain acp"])));
        assert!(!agy_args_unattended(&args(&[
            "agy", "-i", "explain", "acp"
        ])));
        assert!(!agy_args_unattended(&args(&["agy"])));
    }

    #[test]
    fn a_read_or_edit_that_names_no_file_asks() {
        let logs = temp("nofile");
        let site = logs.0.join("code").join("site");
        std::fs::create_dir_all(&site).unwrap();
        let read = step_from_agy("view_file", &json!({"path": "/etc/shadow"}));
        assert!(matches!(
            decide_session_step("agy", &read, &site, false, &logs.0),
            Decision::Ask(_)
        ));
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
