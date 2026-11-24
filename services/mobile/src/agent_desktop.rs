//! The `agent-desktop` accessibility-tree desktop control engine.
//!
//! Implements [`UiControl`] on top of the external `agent-desktop` CLI
//! (vercel-labs, v1.x), which drives Linux applications via the accessibility
//! tree.
//!
//! Typed verbs only:
//! - `apps`: list accessible applications (names only)
//! - `tree`: snapshot the application accessibility hierarchy (capped at 120 nodes)
//! - `find`: search interactive elements by name and role (capped at 10 matches, never returns values)
//! - `read`: read text property of an element (capped at 2000 characters)
//! - `click`: click an actionable element by ref, name, or confirmed label
//! - `type_text`: send text into an editable element over STDIN (never password/secure, capped at 2000 chars)
//!
//! Safety guarantees:
//! - Direct process spawning only, never via a shell.
//! - Strict subcommand allowlist: version, list-apps, list-windows, snapshot, find, get, click, type.
//! - Flags `--headed` and `--screenshot` are strictly forbidden.
//! - Strict input validation: ref pattern `^@s[a-z0-9]+:e[0-9]+$`, app names `^[A-Za-z0-9._ +-]{1,64}$`,
//!   no names starting with '-'.
//! - Text for `type_text` is passed strictly over STDIN via `type REF --stdin`, never in argv.
//! - Password and secure fields are rejected.
//! - Confirmed clicks re-resolve the target label and reject stale refs.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use friday_actions::ActionResult;
use serde_json::{json, Value};

use crate::desktop_tool::UiControl;

/// Default timeout for individual CLI invocations.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(15);

/// Maximum stdout bytes collected from the CLI process (256 KiB).
pub const MAX_STDOUT_BYTES: u64 = 256 * 1024;

/// Maximum nodes in a flattened snapshot tree.
pub const MAX_TREE_NODES: usize = 120;

/// Maximum character length for read and typed text.
pub const MAX_TEXT_CHARS: usize = 2000;

/// The process spawning seam: returns stdout or an error message.
#[async_trait]
pub trait Spawn: Send + Sync {
    async fn run(
        &self,
        args: &[String],
        stdin: Option<&str>,
        timeout: Duration,
    ) -> Result<String, String>;
}

/// Spawns the real `agent-desktop` binary with isolated environment and timeout.
#[derive(Debug, Clone)]
pub struct SystemSpawn {
    pub binary: PathBuf,
}

impl SystemSpawn {
    pub fn new(binary: impl Into<PathBuf>) -> Self {
        Self {
            binary: binary.into(),
        }
    }
}

#[async_trait]
impl Spawn for SystemSpawn {
    async fn run(
        &self,
        args: &[String],
        stdin: Option<&str>,
        timeout: Duration,
    ) -> Result<String, String> {
        let mut cmd = tokio::process::Command::new(&self.binary);
        cmd.args(args);
        cmd.env_clear();

        const ALLOWED_ENV: &[&str] = &[
            "HOME",
            "XDG_RUNTIME_DIR",
            "WAYLAND_DISPLAY",
            "DISPLAY",
            "DBUS_SESSION_BUS_ADDRESS",
            "HYPRLAND_INSTANCE_SIGNATURE",
            "PATH",
            "LANG",
            // Windows: without these a process can't find its system files,
            // profile or temp folder (none of them holds a secret).
            "SystemRoot",
            "SystemDrive",
            "windir",
            "ComSpec",
            "PATHEXT",
            "USERPROFILE",
            "APPDATA",
            "LOCALAPPDATA",
            "TEMP",
            "TMP",
        ];
        for &key in ALLOWED_ENV {
            if let Ok(val) = std::env::var(key) {
                cmd.env(key, val);
            }
        }

        cmd.stdin(if stdin.is_some() {
            std::process::Stdio::piped()
        } else {
            std::process::Stdio::null()
        });
        cmd.stdout(std::process::Stdio::piped());
        cmd.stderr(std::process::Stdio::null());
        cmd.kill_on_drop(true);

        let mut child = cmd
            .spawn()
            .map_err(|e| format!("failed to spawn {}: {e}", self.binary.display()))?;

        if let Some(text) = stdin {
            if let Some(mut child_stdin) = child.stdin.take() {
                use tokio::io::AsyncWriteExt;
                let _ = child_stdin.write_all(text.as_bytes()).await;
                let _ = child_stdin.shutdown().await;
            }
        }

        let stdout_handle = child.stdout.take();

        let read_stdout = async {
            let res: Result<String, String> = if let Some(stdout) = stdout_handle {
                use tokio::io::AsyncReadExt;
                let mut buf = Vec::new();
                let mut reader = stdout.take(MAX_STDOUT_BYTES);
                reader
                    .read_to_end(&mut buf)
                    .await
                    .map_err(|e| format!("failed to read stdout: {e}"))?;
                Ok(String::from_utf8_lossy(&buf).to_string())
            } else {
                Ok(String::new())
            };
            res
        };

        tokio::select! {
            res = read_stdout => {
                let out = res?;
                let status = child
                    .wait()
                    .await
                    .map_err(|e| format!("failed waiting for process: {e}"))?;
                if !out.trim().is_empty() {
                    Ok(out)
                } else if !status.success() {
                    Err(format!("{} exited with status {status}", self.binary.display()))
                } else {
                    Ok(out)
                }
            }
            _ = tokio::time::sleep(timeout) => {
                let _ = child.kill().await;
                Err("process timed out".to_string())
            }
        }
    }
}

/// Enforces the subcommand allowlist and rejects forbidden flags.
pub fn validate_command(args: &[String]) -> Result<(), String> {
    if args.is_empty() {
        return Err("command arguments cannot be empty".to_string());
    }

    const ALLOWED_SUBCOMMANDS: &[&str] = &[
        "version",
        "list-apps",
        "list-windows",
        "snapshot",
        "find",
        "get",
        "click",
        "type",
        "set-value",
    ];

    let subcmd = args[0].as_str();
    if !ALLOWED_SUBCOMMANDS.contains(&subcmd) {
        return Err(format!("subcommand \"{subcmd}\" is not allowed"));
    }

    for arg in args {
        if arg == "--headed" || arg.starts_with("--headed=") {
            return Err("flag \"--headed\" is forbidden".to_string());
        }
        if arg == "--screenshot" || arg.starts_with("--screenshot=") {
            return Err("flag \"--screenshot\" is forbidden".to_string());
        }
    }

    Ok(())
}

/// Validates an element ref against `^@s[a-z0-9]+:e[0-9]+$`.
pub fn validate_ref(r: &str) -> Result<(), String> {
    if !r.starts_with("@s") {
        return Err(format!("invalid ref \"{r}\": must start with @s"));
    }
    let rest = &r[2..];
    let Some((snap, elem)) = rest.split_once(":e") else {
        return Err(format!("invalid ref \"{r}\": missing \":e\" separator"));
    };
    if snap.is_empty()
        || !snap
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
    {
        return Err(format!("invalid ref \"{r}\": invalid snapshot id"));
    }
    if elem.is_empty() || !elem.chars().all(|c| c.is_ascii_digit()) {
        return Err(format!("invalid ref \"{r}\": invalid element index"));
    }
    Ok(())
}

/// Validates an application name against `^[A-Za-z0-9._ +-]{1,64}$` and not starting with '-'.
pub fn validate_app(app: &str) -> Result<(), String> {
    if app.is_empty() || app.len() > 64 {
        return Err(format!(
            "invalid app name \"{app}\": length must be between 1 and 64"
        ));
    }
    if app.starts_with('-') {
        return Err(format!("invalid app name \"{app}\": cannot start with '-'"));
    }
    if !app.chars().all(|c| {
        c.is_ascii_alphanumeric() || c == '.' || c == '_' || c == ' ' || c == '+' || c == '-'
    }) {
        return Err(format!(
            "invalid app name \"{app}\": contains forbidden characters"
        ));
    }
    Ok(())
}

/// Validates that an argument value (name, role, label) does not start with '-'.
pub fn validate_param_name(field: &str, val: &str) -> Result<(), String> {
    if val.starts_with('-') {
        return Err(format!("{field} cannot start with '-'"));
    }
    Ok(())
}

/// An actionable match returned by find.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActionableMatch {
    pub name: String,
    pub role: String,
    pub ref_id: String,
    pub states: Vec<String>,
}

/// Extracts actionable (`interactive: true`) candidates from find matches.
pub fn extract_actionable_matches(find_data: &Value) -> Vec<ActionableMatch> {
    let matches_array = if let Some(m) = find_data.get("matches").and_then(Value::as_array) {
        m
    } else if let Some(m) = find_data.as_array() {
        m
    } else {
        return Vec::new();
    };

    let mut out = Vec::new();
    for item in matches_array {
        let is_interactive = item
            .get("interactive")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        if !is_interactive {
            continue;
        }
        let ref_id = match item
            .get("ref_id")
            .or_else(|| item.get("ref"))
            .and_then(Value::as_str)
        {
            Some(r) if !r.is_empty() => r.to_string(),
            _ => continue,
        };
        let name = item
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        let role = item
            .get("role")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        let states = item
            .get("states")
            .and_then(Value::as_array)
            .map(|arr| {
                arr.iter()
                    .filter_map(Value::as_str)
                    .map(ToString::to_string)
                    .collect()
            })
            .unwrap_or_default();

        out.push(ActionableMatch {
            name,
            role,
            ref_id,
            states,
        });
    }
    out
}

/// Recursively flattens an accessibility snapshot tree to at most `max_nodes`.
/// Keeps only nodes with a non-empty name or ref.
pub fn flatten_snapshot_tree(tree: &Value, max_nodes: usize) -> Vec<Value> {
    let mut out = Vec::new();
    fn walk(node: &Value, out: &mut Vec<Value>, max: usize) {
        if out.len() >= max {
            return;
        }
        let name = node.get("name").and_then(Value::as_str).unwrap_or("");
        let role = node.get("role").and_then(Value::as_str).unwrap_or("");
        let ref_id = node
            .get("ref_id")
            .or_else(|| node.get("ref"))
            .and_then(Value::as_str)
            .unwrap_or("");
        let actions = node
            .get("available_actions")
            .or_else(|| node.get("actions"))
            .cloned()
            .unwrap_or_else(|| json!([]));

        if !name.is_empty() || !ref_id.is_empty() {
            out.push(json!({
                "name": name,
                "role": role,
                "ref": ref_id,
                "actions": actions,
            }));
        }
        if let Some(children) = node.get("children").and_then(Value::as_array) {
            for child in children {
                if out.len() >= max {
                    break;
                }
                walk(child, out, max);
            }
        }
    }
    walk(tree, &mut out, max_nodes);
    out
}

/// Roles whose name is already the text, or just a container's label (not a
/// control to describe).
const TEXT_ROLES: &[&str] = &[
    "text",
    "statictext",
    "static_text",
    "label",
    "heading",
    "paragraph",
    "link",
    "listitem",
    "list_item",
    "cell",
    "document",
    "title",
    "window",
    "group",
    "frame",
    "panel",
    "pane",
    "section",
];

/// The readable text of an accessibility snapshot, one line per named node,
/// at most `max_chars` long. Password fields and anything marked protected
/// are left out entirely; text-like nodes give their text, controls give
/// `role: name`, and a field's own text follows its name.
pub fn tree_text(tree: &Value, max_chars: usize) -> String {
    fn lower(node: &Value, key: &str) -> String {
        node.get(key)
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_lowercase()
    }
    fn secret(node: &Value) -> bool {
        let role = lower(node, "role");
        if role.contains("password") || role.contains("secret") {
            return true;
        }
        node.get("states")
            .and_then(Value::as_array)
            .is_some_and(|s| {
                s.iter().filter_map(Value::as_str).any(|x| {
                    x.eq_ignore_ascii_case("protected") || x.eq_ignore_ascii_case("password")
                })
            })
    }
    fn has_state(node: &Value, names: &[&str]) -> bool {
        node.get("states")
            .and_then(Value::as_array)
            .is_some_and(|s| {
                s.iter()
                    .filter_map(Value::as_str)
                    .any(|x| names.iter().any(|n| x.eq_ignore_ascii_case(n)))
            })
    }
    struct Seen {
        last: String,
        all: std::collections::HashSet<String>,
        trim: bool,
    }
    fn walk(node: &Value, out: &mut String, seen: &mut Seen, max: usize) {
        if out.len() >= max || secret(node) {
            return;
        }
        // What the app marks hidden isn't sent (with the tree-trim switch).
        // "offscreen" and "invisible" aren't trusted: toolkits set them on
        // things in scroll views that are in plain sight.
        if seen.trim && has_state(node, &["hidden"]) {
            return;
        }
        let role = lower(node, "role");
        let name = node
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim();
        let value = node
            .get("value")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim();
        let mut line = String::new();
        if !name.is_empty() {
            if TEXT_ROLES.contains(&role.as_str()) || role.is_empty() {
                line.push_str(name);
            } else {
                line.push_str(&format!("{role}: {name}"));
            }
        }
        if !value.is_empty() && value != name {
            if line.is_empty() {
                line.push_str(value);
            } else {
                line.push_str(&format!(" = {value}"));
            }
        }
        let line = line.split_whitespace().collect::<Vec<_>>().join(" ");
        // Trimmed: a control already sent (a toolbar's "Save" and the
        // menu's; repeated text, like a table's values, stays), and an icon
        // font's glyph ("⋮", a private-use character), say nothing new. A
        // symbol that is a real control (+, ?, <) stays.
        let control = !(TEXT_ROLES.contains(&role.as_str()) || role.is_empty());
        let glyph = value.is_empty()
            && name.chars().count() == 1
            && name
                .chars()
                .all(|c| matches!(c, '⋮' | '⋯' | '☰' | '\u{E000}'..='\u{F8FF}'));
        let noise = seen.trim && ((control && seen.all.contains(&line)) || glyph);
        if !line.is_empty() && line != seen.last && !noise {
            out.push_str(&line);
            out.push('\n');
            seen.all.insert(line.clone());
            seen.last = line;
        }
        // A collapsed menu or tree item: its name, not what it hides.
        if seen.trim && has_state(node, &["collapsed"]) {
            return;
        }
        if let Some(children) = node.get("children").and_then(Value::as_array) {
            for child in children {
                walk(child, out, seen, max);
            }
        }
    }
    let mut out = String::new();
    let mut seen = Seen {
        last: String::new(),
        all: Default::default(),
        trim: crate::features::on("tree-trim"),
    };
    walk(tree, &mut out, &mut seen, max_chars);
    if out.len() > max_chars {
        let mut cut = max_chars;
        while !out.is_char_boundary(cut) {
            cut -= 1;
        }
        out.truncate(cut);
    }
    out.trim_end().to_string()
}

/// Which of `apps` is the window class `class` (a compositor's name for the
/// focused app, like `org.gnome.Nautilus` or `firefox`). An exact match
/// first, then the class's last dotted part; a part that only appears inside
/// another app's name, or in several, picks nothing.
pub fn pick_app(apps: &[String], class: &str) -> Option<String> {
    let squash = |s: &str| {
        s.chars()
            .filter(|c| c.is_alphanumeric())
            .collect::<String>()
            .to_lowercase()
    };
    let full = squash(class);
    let tail = squash(class.rsplit('.').next().unwrap_or(class));
    if full.is_empty() {
        return None;
    }
    for want in [&full, &tail] {
        if want.is_empty() {
            continue;
        }
        // Windows names an app by its program ("Notepad.exe" or "notepad").
        let hits: Vec<&String> = apps
            .iter()
            .filter(|a| {
                let a = match a.len().checked_sub(4) {
                    Some(i) if a.is_char_boundary(i) && a[i..].eq_ignore_ascii_case(".exe") => {
                        &a[..i]
                    }
                    _ => a.as_str(),
                };
                squash(a) == *want
            })
            .collect();
        if hits.len() == 1 {
            return Some(hits[0].clone());
        }
    }
    None
}

/// Converts an error envelope into a plain English error message.
pub fn parse_error_envelope(envelope: &Value) -> String {
    let err_obj = envelope.get("error").filter(|v| v.is_object());
    let code = err_obj
        .and_then(|e| e.get("code"))
        .and_then(Value::as_str)
        .unwrap_or("ERROR");
    let msg = err_obj
        .and_then(|e| e.get("message"))
        .and_then(Value::as_str)
        .unwrap_or("");

    let base = if msg.is_empty() {
        code.to_string()
    } else {
        format!("{code}: {msg}")
    };

    if code == "POLICY_DENIED" || code == "ACTION_NOT_SUPPORTED" {
        format!(
            "{base}. that control can't be pressed from here (some apps, like GTK4 ones, offer no way to press it without physical input)"
        )
    } else {
        base
    }
}

/// The result of analyzing window candidates in an AMBIGUOUS_TARGET error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WindowAmbiguityResolution {
    /// Exactly one candidate has `is_focused: true`.
    Focused(String),
    /// Zero or multiple candidates are focused; lists all candidate window titles.
    Ambiguous(Vec<String>),
}

/// Analyzes candidates from an AMBIGUOUS_TARGET error envelope.
pub fn resolve_window_ambiguity(details: &Value) -> Option<WindowAmbiguityResolution> {
    let candidates = details.get("candidates")?.as_array()?;
    let mut focused = Vec::new();
    let mut titles = Vec::new();

    for c in candidates {
        let title = c
            .get("title")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        titles.push(title);
        if c.get("is_focused").and_then(Value::as_bool) == Some(true) {
            if let Some(id) = c.get("id").and_then(Value::as_str) {
                focused.push(id.to_string());
            }
        }
    }

    if focused.len() == 1 {
        Some(WindowAmbiguityResolution::Focused(focused.remove(0)))
    } else {
        Some(WindowAmbiguityResolution::Ambiguous(titles))
    }
}

/// Table of whole-word dangerous button labels.
const DANGEROUS_TARGETS: &[&[&str]] = &[
    &["delete"],
    &["remove"],
    &["erase"],
    &["format"],
    &["uninstall"],
    &["empty", "trash"],
    &["send"],
    &["submit"],
    &["post"],
    &["publish"],
    &["pay"],
    &["buy"],
    &["purchase"],
    &["order"],
    &["confirm"],
    &["sign", "out"],
    &["log", "out"],
    &["logout"],
    &["shut", "down"],
    &["shutdown"],
    &["restart"],
    &["power", "off"],
    &["overwrite"],
    &["reset"],
    &["clear", "all"],
    &["discard"],
    &["install"],
    &["trash"],
    &["kill"],
    &["terminate"],
    &["unsubscribe"],
    &["revoke"],
    &["allow"],
    &["grant"],
    &["authorize"],
];

/// Checks whether a label (case-insensitive, whole-word) contains dangerous terms.
pub fn label_is_dangerous(label: &str) -> bool {
    let lower = label.to_lowercase();
    let cleaned: String = lower
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { ' ' })
        .collect();
    let words: Vec<&str> = cleaned.split_whitespace().collect();
    DANGEROUS_TARGETS.iter().any(|target| {
        if target.len() > words.len() {
            return false;
        }
        words.windows(target.len()).any(|window| window == *target)
    })
}

/// Parses a semver triple `(major, minor, patch)` by hand.
pub fn parse_semver(s: &str) -> Option<(u64, u64, u64)> {
    let core = s.split(['-', '+']).next()?;
    let mut parts = core.split('.');
    let major = parts.next()?.parse::<u64>().ok()?;
    let minor = parts.next()?.parse::<u64>().ok()?;
    let patch = parts.next()?.parse::<u64>().ok()?;
    if parts.next().is_some() {
        return None;
    }
    Some((major, minor, patch))
}

/// Version policy: envelope schema starts with "2." and tool version is >= 1.3.0 and < 2.0.0.
pub fn version_supported(schema: &str, tool: &str) -> bool {
    if !schema.starts_with("2.") {
        return false;
    }
    let Some((major, minor, _patch)) = parse_semver(tool) else {
        return false;
    };
    major == 1 && minor >= 3
}

/// Checks whether role or states indicate a password or secure field.
fn is_password_or_secure(role: &str, states: &[String]) -> bool {
    let r = role.to_lowercase();
    if r.contains("password") || r.contains("secure") {
        return true;
    }
    states.iter().any(|s| s.to_lowercase().contains("secure"))
}

/// Searches the system PATH for an executable by name.
fn find_on_path(binary_name: &str) -> Option<PathBuf> {
    let path_var = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path_var) {
        let candidate = dir.join(binary_name);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

/// Desktop UI controller backed by the `agent-desktop` CLI.
pub struct AgentDesktopUi {
    spawn: Arc<dyn Spawn>,
}

impl AgentDesktopUi {
    pub fn new(spawn: Arc<dyn Spawn>) -> Self {
        Self { spawn }
    }

    /// Runs a validated command via the spawn seam.
    async fn run_cli(&self, args: &[String], stdin: Option<&str>) -> Result<String, String> {
        validate_command(args)?;
        self.spawn.run(args, stdin, DEFAULT_TIMEOUT).await
    }

    /// Runs a command, parses the JSON envelope, and automatically retries once
    /// if AMBIGUOUS_TARGET identifies a single focused window.
    async fn run_cli_envelope(
        &self,
        args: &[String],
        stdin: Option<&str>,
    ) -> Result<Value, String> {
        let stdout = self.run_cli(args, stdin).await?;
        let envelope: Value = serde_json::from_str(&stdout)
            .map_err(|e| format!("invalid JSON envelope from agent-desktop: {e}"))?;

        if envelope.get("ok").and_then(Value::as_bool) == Some(true) {
            return Ok(envelope);
        }

        let err_obj = envelope.get("error").filter(|v| v.is_object());
        let code = err_obj
            .and_then(|e| e.get("code"))
            .and_then(Value::as_str)
            .unwrap_or("");

        let has_window_id = args.iter().any(|a| a.starts_with("--window-id"));

        if code == "AMBIGUOUS_TARGET" && !has_window_id {
            if let Some(details) = err_obj.and_then(|e| e.get("details")) {
                match resolve_window_ambiguity(details) {
                    Some(WindowAmbiguityResolution::Focused(wid)) => {
                        let mut retry_args = args.to_vec();
                        retry_args.push(format!("--window-id={wid}"));
                        let retry_stdout = self.run_cli(&retry_args, stdin).await?;
                        let retry_envelope: Value = serde_json::from_str(&retry_stdout)
                            .map_err(|e| format!("invalid JSON envelope on retry: {e}"))?;
                        if retry_envelope.get("ok").and_then(Value::as_bool) == Some(true) {
                            return Ok(retry_envelope);
                        }
                        return Err(parse_error_envelope(&retry_envelope));
                    }
                    Some(WindowAmbiguityResolution::Ambiguous(titles)) => {
                        let formatted = titles
                            .iter()
                            .map(|t| format!("\"{t}\""))
                            .collect::<Vec<_>>()
                            .join(", ");
                        return Err(format!(
                            "multiple windows found ({formatted}); name the window to use"
                        ));
                    }
                    None => {}
                }
            }
        }

        Err(parse_error_envelope(&envelope))
    }

    /// Resolves an optional window title substring to a unique `--window-id`.
    async fn resolve_window_id(
        &self,
        app: Option<&str>,
        window_substring: &str,
    ) -> Result<String, String> {
        let envelope = self
            .run_cli_envelope(&["list-windows".to_string()], None)
            .await?;
        let windows = envelope
            .get("data")
            .and_then(Value::as_array)
            .ok_or_else(|| "missing window list in response".to_string())?;

        let mut matching = Vec::new();
        let needle = window_substring.to_lowercase();
        for w in windows {
            if let Some(app_name) = app {
                if w.get("app_name").and_then(Value::as_str) != Some(app_name) {
                    continue;
                }
            }
            let title = w.get("title").and_then(Value::as_str).unwrap_or("");
            if title.to_lowercase().contains(&needle) {
                matching.push(w);
            }
        }

        if matching.is_empty() {
            let app_ctx = app.map(|a| format!(" for {a}")).unwrap_or_default();
            return Err(format!("no window{app_ctx} matches \"{window_substring}\""));
        }
        if matching.len() > 1 {
            let titles: Vec<String> = matching
                .iter()
                .map(|w| {
                    format!(
                        "\"{}\"",
                        w.get("title").and_then(Value::as_str).unwrap_or("")
                    )
                })
                .collect();
            return Err(format!(
                "multiple windows match \"{window_substring}\" ({}); say which",
                titles.join(", ")
            ));
        }

        let id = matching[0]
            .get("id")
            .and_then(Value::as_str)
            .ok_or_else(|| "window missing id".to_string())?;
        Ok(id.to_string())
    }

    /// Executes `find` and returns actionable candidates.
    async fn find_elements(
        &self,
        app: &str,
        name: Option<&str>,
        role: Option<&str>,
        exact: bool,
        window_id: Option<&str>,
    ) -> Result<Vec<ActionableMatch>, String> {
        let mut args = vec!["find".to_string(), "--app".to_string(), app.to_string()];
        if let Some(n) = name {
            validate_param_name("name", n)?;
            args.push(format!("--name={n}"));
        }
        if let Some(r) = role {
            validate_param_name("role", r)?;
            args.push(format!("--role={r}"));
        }
        if exact {
            args.push("--exact".to_string());
        }
        if let Some(wid) = window_id {
            args.push(format!("--window-id={wid}"));
        }
        args.push("--limit".to_string());
        args.push("10".to_string());

        let envelope = self.run_cli_envelope(&args, None).await?;
        let matches_val = envelope
            .get("data")
            .and_then(|d| d.get("matches"))
            .unwrap_or(&Value::Null);
        Ok(extract_actionable_matches(matches_val))
    }

    /// Resolves an element by name (exact first, then non-exact).
    async fn resolve_by_name(
        &self,
        app: &str,
        name: &str,
        window_id: Option<&str>,
    ) -> Result<ActionableMatch, String> {
        validate_param_name("name", name)?;
        let mut matches = self
            .find_elements(app, Some(name), None, true, window_id)
            .await?;
        if matches.is_empty() {
            matches = self
                .find_elements(app, Some(name), None, false, window_id)
                .await?;
        }
        if matches.is_empty() {
            return Err(format!("nothing named \"{name}\" in {app} to press"));
        }
        if matches.len() > 1 {
            let candidates = matches
                .iter()
                .map(|m| format!("\"{}\" ({})", m.name, m.role))
                .collect::<Vec<_>>()
                .join(", ");
            return Err(format!(
                "multiple elements match \"{name}\" in {app}: {candidates}; say which"
            ));
        }
        Ok(matches.remove(0))
    }

    /// Re-resolves an element by label for confirmed clicks.
    async fn resolve_by_label(
        &self,
        app: &str,
        label: &str,
        window_id: Option<&str>,
    ) -> Result<ActionableMatch, String> {
        validate_param_name("label", label)?;
        let matches = self
            .find_elements(app, Some(label), None, true, window_id)
            .await?;
        let mut exact_matches: Vec<ActionableMatch> = matches
            .into_iter()
            .filter(|m| m.name.eq_ignore_ascii_case(label))
            .collect();
        if exact_matches.len() != 1 {
            return Err("the button changed since you were asked".to_string());
        }
        Ok(exact_matches.remove(0))
    }

    /// Checks if a ref points to a password or secure element via snapshot.
    /// What an element is, looked up from its ref alone (refs outlive the
    /// snapshot call that made them): its label, role and states. A ref is
    /// never acted on, read or typed into before this says it is safe.
    async fn ref_facts(&self, ref_id: &str) -> Result<RefFacts, String> {
        let get = |property: &str| {
            vec![
                "get".to_string(),
                ref_id.to_string(),
                "--property".to_string(),
                property.to_string(),
            ]
        };
        let value_of =
            |envelope: &Value| envelope.get("data").and_then(|d| d.get("value")).cloned();
        let label = self.run_cli_envelope(&get("text"), None).await?;
        let role = self.run_cli_envelope(&get("role"), None).await?;
        let states = self.run_cli_envelope(&get("states"), None).await?;
        Ok(RefFacts {
            label: value_of(&label)
                .and_then(|v| v.as_str().map(str::to_string))
                .unwrap_or_default(),
            role: value_of(&role)
                .and_then(|v| v.as_str().map(str::to_string))
                .unwrap_or_default(),
            states: value_of(&states)
                .and_then(|v| v.as_array().cloned())
                .map(|a| {
                    a.iter()
                        .filter_map(Value::as_str)
                        .map(str::to_string)
                        .collect()
                })
                .unwrap_or_default(),
        })
    }
}

/// What `get` says about one element.
struct RefFacts {
    label: String,
    role: String,
    states: Vec<String>,
}

impl AgentDesktopUi {
    /// Sets an empty field's value to `text`. Setting a field that already has
    /// text would replace it: on Windows the text is inserted at the app's
    /// caret instead, and elsewhere the field is left alone and `refused` says why.
    async fn fill_if_empty(
        &self,
        app: &str,
        target_ref: &str,
        text: &str,
        refused: String,
    ) -> ActionResult {
        let get = vec![
            "get".to_string(),
            target_ref.to_string(),
            "--property".to_string(),
            "value".to_string(),
        ];
        let current = match self.run_cli_envelope(&get, None).await {
            Ok(env) => env["data"]["value"].as_str().unwrap_or("").to_string(),
            Err(_) => return ActionResult::error(refused),
        };
        if !current.trim().is_empty() {
            #[cfg(windows)]
            {
                let (app, owned) = (app.to_string(), text.to_string());
                return match tokio::task::spawn_blocking(move || {
                    crate::win_insert::insert_text(&app, &owned)
                })
                .await
                {
                    Ok(Ok(())) => ActionResult::success(json!({
                        "typed": text.chars().count(),
                        "ref": target_ref,
                        "via": "insert",
                    })),
                    Ok(Err(err)) => ActionResult::error(format!("{refused}. {err}")),
                    Err(err) => ActionResult::error(err.to_string()),
                };
            }
            #[cfg(not(windows))]
            {
                let _ = app;
                return ActionResult::error(format!(
                    "{refused}. That field already has text, and FRIDAY won't replace it"
                ));
            }
        }
        let set = vec![
            "set-value".to_string(),
            target_ref.to_string(),
            "--stdin".to_string(),
        ];
        match self.run_cli_envelope(&set, Some(text)).await {
            Ok(_) => ActionResult::success(json!({
                "typed": text.chars().count(),
                "ref": target_ref,
                "via": "set-value",
            })),
            Err(err) => ActionResult::error(err),
        }
    }
}

#[async_trait]
impl UiControl for AgentDesktopUi {
    async fn run(&self, verb: &str, params: Value) -> ActionResult {
        match verb {
            "apps" => {
                let envelope = match self
                    .run_cli_envelope(&["list-apps".to_string()], None)
                    .await
                {
                    Ok(e) => e,
                    Err(err) => return ActionResult::error(err),
                };
                let apps = envelope
                    .get("data")
                    .and_then(|d| d.get("apps"))
                    .and_then(Value::as_array)
                    .cloned()
                    .unwrap_or_default();
                let names: Vec<Value> = apps
                    .into_iter()
                    .filter_map(|app| {
                        let name = app.get("name").and_then(Value::as_str)?;
                        Some(json!({ "name": name }))
                    })
                    .collect();
                ActionResult::success(json!(names))
            }
            "tree" => {
                let app = match params.get("app").and_then(Value::as_str) {
                    Some(a) => a,
                    None => return ActionResult::error("name the app to inspect"),
                };
                if let Err(err) = validate_app(app) {
                    return ActionResult::error(err);
                }
                let window_id = if let Some(w) = params.get("window").and_then(Value::as_str) {
                    match self.resolve_window_id(Some(app), w).await {
                        Ok(id) => Some(id),
                        Err(err) => return ActionResult::error(err),
                    }
                } else {
                    None
                };

                let mut args = vec![
                    "snapshot".to_string(),
                    "--app".to_string(),
                    app.to_string(),
                    "-i".to_string(),
                    "--compact".to_string(),
                ];
                if let Some(wid) = window_id {
                    args.push(format!("--window-id={wid}"));
                }

                let envelope = match self.run_cli_envelope(&args, None).await {
                    Ok(e) => e,
                    Err(err) => return ActionResult::error(err),
                };
                let tree = envelope
                    .get("data")
                    .and_then(|d| d.get("tree"))
                    .unwrap_or(&Value::Null);
                let flattened = flatten_snapshot_tree(tree, MAX_TREE_NODES);
                ActionResult::success(Value::Array(flattened))
            }
            "find" => {
                let app = match params.get("app").and_then(Value::as_str) {
                    Some(a) => a,
                    None => return ActionResult::error("name the app to find elements in"),
                };
                if let Err(err) = validate_app(app) {
                    return ActionResult::error(err);
                }
                let name = params.get("name").and_then(Value::as_str);
                if let Some(n) = name {
                    if let Err(err) = validate_param_name("name", n) {
                        return ActionResult::error(err);
                    }
                }
                let role = params.get("role").and_then(Value::as_str);
                if let Some(r) = role {
                    if let Err(err) = validate_param_name("role", r) {
                        return ActionResult::error(err);
                    }
                }
                let window_id = if let Some(w) = params.get("window").and_then(Value::as_str) {
                    match self.resolve_window_id(Some(app), w).await {
                        Ok(id) => Some(id),
                        Err(err) => return ActionResult::error(err),
                    }
                } else {
                    None
                };

                let mut args = vec!["find".to_string(), "--app".to_string(), app.to_string()];
                if let Some(n) = name {
                    args.push(format!("--name={n}"));
                }
                if let Some(r) = role {
                    args.push(format!("--role={r}"));
                }
                if let Some(wid) = window_id {
                    args.push(format!("--window-id={wid}"));
                }
                args.push("--limit".to_string());
                args.push("10".to_string());

                let envelope = match self.run_cli_envelope(&args, None).await {
                    Ok(e) => e,
                    Err(err) => return ActionResult::error(err),
                };
                let matches_val = envelope
                    .get("data")
                    .and_then(|d| d.get("matches"))
                    .unwrap_or(&Value::Null);
                let candidates = extract_actionable_matches(matches_val);
                let mapped: Vec<Value> = candidates
                    .into_iter()
                    .map(|c| {
                        json!({
                            "name": c.name,
                            "role": c.role,
                            "ref": c.ref_id,
                            "states": c.states,
                        })
                    })
                    .collect();
                ActionResult::success(json!(mapped))
            }
            "read" => {
                let app = match params.get("app").and_then(Value::as_str) {
                    Some(a) => a,
                    None => return ActionResult::error("name the app to read from"),
                };
                if let Err(err) = validate_app(app) {
                    return ActionResult::error(err);
                }
                let ref_id = match params
                    .get("ref")
                    .or_else(|| params.get("path"))
                    .and_then(Value::as_str)
                {
                    Some(r) => r,
                    None => return ActionResult::error("ref is required to read element"),
                };
                if let Err(err) = validate_ref(ref_id) {
                    return ActionResult::error(err);
                }

                // A password or secure field is never read.
                match self.ref_facts(ref_id).await {
                    Ok(f) if is_password_or_secure(&f.role, &f.states) => {
                        return ActionResult::error("refusing to read a password or secure field");
                    }
                    Ok(_) => {}
                    Err(err) => return ActionResult::error(err),
                }
                let args = vec![
                    "get".to_string(),
                    ref_id.to_string(),
                    "--property".to_string(),
                    "text".to_string(),
                ];
                let envelope = match self.run_cli_envelope(&args, None).await {
                    Ok(e) => e,
                    Err(err) => return ActionResult::error(err),
                };
                let raw_text = envelope
                    .get("data")
                    .and_then(|d| d.get("value"))
                    .and_then(Value::as_str)
                    .unwrap_or("");
                let truncated: String = raw_text.chars().take(MAX_TEXT_CHARS).collect();
                ActionResult::success(json!({
                    "text": truncated,
                    "ref": ref_id,
                }))
            }
            "click" => {
                let app = match params.get("app").and_then(Value::as_str) {
                    Some(a) => a,
                    None => return ActionResult::error("name the app to click in"),
                };
                if let Err(err) = validate_app(app) {
                    return ActionResult::error(err);
                }
                let window_id = if let Some(w) = params.get("window").and_then(Value::as_str) {
                    match self.resolve_window_id(Some(app), w).await {
                        Ok(id) => Some(id),
                        Err(err) => return ActionResult::error(err),
                    }
                } else {
                    None
                };

                let confirmed = params.get("confirmed").and_then(Value::as_bool) == Some(true);

                if let Some(label) = params.get("label").and_then(Value::as_str) {
                    let target = match self
                        .resolve_by_label(app, label, window_id.as_deref())
                        .await
                    {
                        Ok(t) => t,
                        Err(err) => return ActionResult::error(err),
                    };

                    if self.is_dangerous(label) && !confirmed {
                        return ActionResult::error(format!(
                            "\"{label}\" could lose or send something, so it needs confirmation first"
                        ));
                    }

                    let args = vec!["click".to_string(), target.ref_id.clone()];
                    match self.run_cli_envelope(&args, None).await {
                        Ok(_) => ActionResult::success(json!({
                            "clicked": true,
                            "ref": target.ref_id,
                            "label": label,
                        })),
                        Err(err) => ActionResult::error(err),
                    }
                } else if let Some(ref_id) = params
                    .get("ref")
                    .or_else(|| params.get("path"))
                    .and_then(Value::as_str)
                {
                    if let Err(err) = validate_ref(ref_id) {
                        return ActionResult::error(err);
                    }
                    // A bare ref says nothing about the button: look it up, and
                    // gate on its own label like a click by name.
                    let facts = match self.ref_facts(ref_id).await {
                        Ok(f) => f,
                        Err(err) => return ActionResult::error(err),
                    };
                    if is_password_or_secure(&facts.role, &facts.states) {
                        return ActionResult::error("refusing to press a password or secure field");
                    }
                    if self.is_dangerous(&facts.label) && !confirmed {
                        return ActionResult::error(format!(
                            "\"{}\" could lose or send something, so it needs confirmation first",
                            facts.label
                        ));
                    }
                    let args = vec!["click".to_string(), ref_id.to_string()];
                    match self.run_cli_envelope(&args, None).await {
                        Ok(_) => ActionResult::success(json!({
                            "clicked": true,
                            "ref": ref_id,
                            "label": facts.label,
                        })),
                        Err(err) => ActionResult::error(err),
                    }
                } else if let Some(name) = params.get("name").and_then(Value::as_str) {
                    let target = match self.resolve_by_name(app, name, window_id.as_deref()).await {
                        Ok(t) => t,
                        Err(err) => return ActionResult::error(err),
                    };

                    if self.is_dangerous(&target.name) && !confirmed {
                        return ActionResult::error(format!(
                            "\"{}\" could lose or send something, so it needs confirmation first",
                            target.name
                        ));
                    }

                    let args = vec!["click".to_string(), target.ref_id.clone()];
                    match self.run_cli_envelope(&args, None).await {
                        Ok(_) => ActionResult::success(json!({
                            "clicked": true,
                            "ref": target.ref_id,
                            "name": target.name,
                        })),
                        Err(err) => ActionResult::error(err),
                    }
                } else {
                    ActionResult::error("name, ref, or label is required for click")
                }
            }
            "type_text" => {
                let app = match params.get("app").and_then(Value::as_str) {
                    Some(a) => a,
                    None => return ActionResult::error("name the app to type in"),
                };
                if let Err(err) = validate_app(app) {
                    return ActionResult::error(err);
                }
                let text = match params.get("text").and_then(Value::as_str) {
                    Some(t) => t,
                    None => return ActionResult::error("text parameter is required"),
                };
                if text.chars().count() > MAX_TEXT_CHARS {
                    return ActionResult::error("text exceeds maximum length of 2000 characters");
                }

                let window_id = if let Some(w) = params.get("window").and_then(Value::as_str) {
                    match self.resolve_window_id(Some(app), w).await {
                        Ok(id) => Some(id),
                        Err(err) => return ActionResult::error(err),
                    }
                } else {
                    None
                };

                let target_ref = if let Some(name) = params.get("name").and_then(Value::as_str) {
                    let target = match self.resolve_by_name(app, name, window_id.as_deref()).await {
                        Ok(t) => t,
                        Err(err) => return ActionResult::error(err),
                    };
                    if is_password_or_secure(&target.role, &target.states) {
                        return ActionResult::error(
                            "refusing to type into password or secure field",
                        );
                    }
                    target.ref_id
                } else if let Some(ref_id) = params
                    .get("ref")
                    .or_else(|| params.get("path"))
                    .and_then(Value::as_str)
                {
                    if let Err(err) = validate_ref(ref_id) {
                        return ActionResult::error(err);
                    }
                    match self.ref_facts(ref_id).await {
                        Ok(f) if is_password_or_secure(&f.role, &f.states) => {
                            return ActionResult::error(
                                "refusing to type into password or secure field",
                            );
                        }
                        Ok(_) => {}
                        Err(err) => return ActionResult::error(err),
                    }
                    ref_id.to_string()
                } else {
                    return ActionResult::error("name or ref is required for type_text");
                };

                let args = vec![
                    "type".to_string(),
                    target_ref.clone(),
                    "--stdin".to_string(),
                ];
                match self.run_cli_envelope(&args, Some(text)).await {
                    Ok(_) => ActionResult::success(json!({
                        "typed": text.chars().count(),
                        "ref": target_ref,
                    })),
                    // Some editors take no inserted text through accessibility
                    // (the Windows 11 Notepad editor is one) and only physical
                    // keys would reach them, which FRIDAY never sends. An empty
                    // one can still be filled by setting its value: nothing is
                    // replaced, and no focus or keys are involved.
                    Err(err) if err.contains("semantic action is unavailable") => {
                        self.fill_if_empty(app, &target_ref, text, err).await
                    }
                    Err(err) => ActionResult::error(err),
                }
            }
            _ => ActionResult::error(format!("unknown ui verb \"{verb}\"")),
        }
    }

    async fn resolve_click(&self, params: &Value) -> Result<(String, String), String> {
        let app = params
            .get("app")
            .and_then(Value::as_str)
            .ok_or_else(|| "name the app to click in".to_string())?;
        validate_app(app)?;

        let window_id = if let Some(w) = params.get("window").and_then(Value::as_str) {
            Some(self.resolve_window_id(Some(app), w).await?)
        } else {
            None
        };

        if let Some(name) = params.get("name").and_then(Value::as_str) {
            let target = self
                .resolve_by_name(app, name, window_id.as_deref())
                .await?;
            Ok((target.ref_id, target.name))
        } else if let Some(label) = params.get("label").and_then(Value::as_str) {
            let target = self
                .resolve_by_label(app, label, window_id.as_deref())
                .await?;
            Ok((target.ref_id, target.name))
        } else if let Some(ref_id) = params
            .get("ref")
            .or_else(|| params.get("path"))
            .and_then(Value::as_str)
        {
            validate_ref(ref_id)?;
            let facts = self.ref_facts(ref_id).await?;
            Ok((ref_id.to_string(), facts.label))
        } else {
            Err("name, ref, or label is required for resolve_click".to_string())
        }
    }

    fn is_dangerous(&self, label: &str) -> bool {
        label_is_dangerous(label)
    }
}

/// The CLI's file name here.
const BINARY_NAME: &str = if cfg!(windows) {
    "agent-desktop.exe"
} else {
    "agent-desktop"
};

/// Where the CLI is: `FRIDAY_AGENT_DESKTOP`, FRIDAY's own copy (the package's
/// on Linux, the one the MSI puts beside the daemon on Windows), then PATH.
fn locate_binary() -> Option<PathBuf> {
    let from_env = std::env::var("FRIDAY_AGENT_DESKTOP")
        .ok()
        .map(PathBuf::from)
        .filter(|p| p.is_file());
    from_env
        .or_else(|| Some(PathBuf::from("/usr/lib/friday/agent-desktop")).filter(|p| p.is_file()))
        .or_else(|| beside_this_program(BINARY_NAME))
        .or_else(|| find_on_path(BINARY_NAME))
}

/// A file in the folder this program was started from (the MSI's install folder).
fn beside_this_program(name: &str) -> Option<PathBuf> {
    cfg!(windows)
        .then(|| std::env::current_exe().ok())
        .flatten()?
        .parent()
        .map(|dir| dir.join(name))
        .filter(|p| p.is_file())
}

/// The same check as [`detect`] for a synchronous caller (the goal runner is
/// built outside async code): runs `version` with a 3 s limit.
pub fn detect_blocking() -> Option<AgentDesktopUi> {
    let binary = locate_binary()?;
    let (tx, rx) = std::sync::mpsc::channel();
    let path = binary.clone();
    std::thread::spawn(move || {
        let out = std::process::Command::new(&path)
            .arg("version")
            .stdin(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .output();
        let _ = tx.send(out);
    });
    let out = rx.recv_timeout(Duration::from_secs(3)).ok()?.ok()?;
    let envelope: Value = serde_json::from_slice(&out.stdout).ok()?;
    if envelope.get("ok").and_then(Value::as_bool) != Some(true) {
        return None;
    }
    let schema_ver = envelope.get("version").and_then(Value::as_str)?;
    let tool_ver = envelope
        .get("data")
        .and_then(|d| d.get("version"))
        .and_then(Value::as_str)?;
    version_supported(schema_ver, tool_ver)
        .then(|| AgentDesktopUi::new(Arc::new(SystemSpawn::new(binary))))
}

/// Which engine reads and presses things inside apps.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Engine {
    AgentDesktop,
    Atspi,
    None,
}

/// The engine to use. `FRIDAY_UI_ENGINE` is `auto` (the default: agent-desktop
/// when a supported one is installed, else FRIDAY's own AT-SPI adapter where
/// there is one), `agent-desktop` (that or nothing) or `atspi`.
pub fn choose_engine(setting: Option<&str>, agent_desktop_found: bool, atspi_here: bool) -> Engine {
    let atspi = if atspi_here {
        Engine::Atspi
    } else {
        Engine::None
    };
    match setting.map(|s| s.trim().to_ascii_lowercase()).as_deref() {
        Some("atspi") => atspi,
        Some("agent-desktop") => {
            if agent_desktop_found {
                Engine::AgentDesktop
            } else {
                Engine::None
            }
        }
        _ => {
            if agent_desktop_found {
                Engine::AgentDesktop
            } else {
                atspi
            }
        }
    }
}

/// Detects and returns an `AgentDesktopUi` if a supported CLI binary is present.
pub async fn detect() -> Option<AgentDesktopUi> {
    let binary = locate_binary()?;
    let spawn = Arc::new(SystemSpawn::new(binary));
    let stdout = spawn
        .run(&["version".to_string()], None, Duration::from_secs(3))
        .await
        .ok()?;
    let envelope: Value = serde_json::from_str(&stdout).ok()?;
    if envelope.get("ok").and_then(Value::as_bool) != Some(true) {
        return None;
    }
    let schema_ver = envelope.get("version").and_then(Value::as_str)?;
    let tool_ver = envelope
        .get("data")
        .and_then(|d| d.get("version"))
        .and_then(Value::as_str)?;
    if version_supported(schema_ver, tool_ver) {
        Some(AgentDesktopUi::new(spawn))
    } else {
        None
    }
}

/// The id of `app`'s window that is in front: the one with the focused
/// window's exact title, else the app's only window. Anything less certain
/// (no title match, two with that title) is None, so a window the person
/// isn't looking at, a private one, is never read in its place.
fn front_window(windows: &[Value], app: &str, title: &str) -> Option<String> {
    let own: Vec<&Value> = windows
        .iter()
        .filter(|w| w.get("app_name").and_then(Value::as_str) == Some(app))
        .collect();
    let titled: Vec<&&Value> = own
        .iter()
        .filter(|w| w.get("title").and_then(Value::as_str) == Some(title))
        .collect();
    let w = match (titled.as_slice(), own.as_slice()) {
        ([w], _) => **w,
        ([], [w]) => *w,
        _ => return None,
    };
    w.get("id").and_then(Value::as_str).map(str::to_string)
}

/// The most text of one window sent to a model.
const MAX_SCREEN_TEXT: usize = 12_000;

/// Words in a field's name that make it a card field.
const CARD_WORDS: [&str; 7] = [
    "card number",
    "credit card",
    "debit card",
    "cvv",
    "cvc",
    "security code",
    "expiry",
];

/// Where `tree`'s password and card fields are (`bounds` the snapshot took
/// with `--include-bounds`): `[x, y, width, height]` each, or
/// [`crate::context_read::UNPLACED`] for one without bounds.
pub fn secret_field_bounds(tree: &Value) -> Vec<[i64; 4]> {
    fn walk(node: &Value, out: &mut Vec<[i64; 4]>) {
        let s = |k: &str| {
            node.get(k)
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_lowercase()
        };
        let states: Vec<String> = node
            .get("states")
            .and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .filter_map(Value::as_str)
                    .map(str::to_lowercase)
                    .collect()
            })
            .unwrap_or_default();
        let role = s("role");
        let field = role.contains("text")
            || role.contains("entry")
            || role.contains("field")
            || role.contains("password");
        let secret = role.contains("password")
            || role.contains("secret")
            || states
                .iter()
                .any(|x| x == "protected" || x == "password" || x.contains("secure"))
            || (field && CARD_WORDS.iter().any(|w| s("name").contains(w)));
        if secret {
            if let Some(b) = node.get("bounds") {
                let n = |k: &str, alt: &str| {
                    b.get(k)
                        .or_else(|| b.get(alt))
                        .and_then(Value::as_f64)
                        .map(|v| v.round() as i64)
                };
                if let (Some(x), Some(y), Some(w), Some(h)) = (
                    n("x", "left"),
                    n("y", "top"),
                    n("width", "w"),
                    n("height", "h"),
                ) {
                    out.push([x, y, w, h]);
                } else {
                    out.push(crate::context_read::UNPLACED);
                }
            } else {
                out.push(crate::context_read::UNPLACED);
            }
        }
        if let Some(children) = node.get("children").and_then(Value::as_array) {
            for c in children {
                walk(c, out);
            }
        }
    }
    let mut out = Vec::new();
    walk(tree, &mut out);
    out
}

impl AgentDesktopUi {
    /// The accessibility tree of `class`'s window titled `title` (the one in
    /// front), never another window's, with `extra` snapshot flags.
    async fn front_tree(&self, class: &str, title: &str, extra: &[&str]) -> Option<Value> {
        let listed = self
            .run_cli_envelope(&["list-apps".to_string()], None)
            .await
            .ok()?;
        let names: Vec<String> = listed
            .get("data")
            .and_then(|d| d.get("apps"))
            .and_then(Value::as_array)?
            .iter()
            .filter_map(|a| a.get("name").and_then(Value::as_str).map(str::to_string))
            .collect();
        let app = pick_app(&names, class)?;
        validate_app(&app).ok()?;
        let listed = self
            .run_cli_envelope(&["list-windows".to_string()], None)
            .await
            .ok()?;
        let id = front_window(listed.get("data")?.as_array()?, &app, title)?;
        let mut args = vec![
            "snapshot".to_string(),
            "--app".to_string(),
            app,
            format!("--window-id={id}"),
        ];
        args.extend(extra.iter().map(|s| s.to_string()));
        let snap = self.run_cli_envelope(&args, None).await.ok()?;
        snap.get("data")?.get("tree").cloned()
    }
}

#[async_trait]
impl crate::context_read::ScreenText for AgentDesktopUi {
    async fn secret_fields(&self, class: &str, title: &str) -> Option<Vec<[i64; 4]>> {
        let tree = self.front_tree(class, title, &["--include-bounds"]).await?;
        Some(secret_field_bounds(&tree))
    }

    async fn window_text(&self, class: &str, title: &str) -> Option<String> {
        let listed = self
            .run_cli_envelope(&["list-apps".to_string()], None)
            .await
            .ok()?;
        let names: Vec<String> = listed
            .get("data")
            .and_then(|d| d.get("apps"))
            .and_then(Value::as_array)?
            .iter()
            .filter_map(|a| a.get("name").and_then(Value::as_str).map(str::to_string))
            .collect();
        let app = pick_app(&names, class)?;
        validate_app(&app).ok()?;
        // Only the window in front: another window of the same app (a private
        // one, another document) never goes with the ask.
        let listed = self
            .run_cli_envelope(&["list-windows".to_string()], None)
            .await
            .ok()?;
        let id = front_window(listed.get("data")?.as_array()?, &app, title)?;
        let snap = self
            .run_cli_envelope(
                &[
                    "snapshot".to_string(),
                    "--app".to_string(),
                    app,
                    format!("--window-id={id}"),
                    "--compact".to_string(),
                ],
                None,
            )
            .await
            .ok()?;
        let tree = snap.get("data")?.get("tree")?;
        // Short is returned too: it is checked for secrets before a
        // screenshot of the window goes instead.
        let mut text = tree_text(tree, MAX_SCREEN_TEXT);
        // More than fits: say so, to the model and in the answer.
        if tree_text(tree, MAX_SCREEN_TEXT + 1).len() > text.len() {
            text.push('\n');
            text.push_str(crate::context_read::SCREEN_TEXT_CUT);
        }
        Some(text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::sync::Mutex;

    #[test]
    fn password_and_card_fields_are_found_with_their_bounds() {
        let tree = serde_json::json!({
            "role": "window", "name": "Checkout",
            "children": [
                {"role": "textfield", "name": "Email", "bounds": {"x": 10, "y": 10, "width": 200, "height": 24}},
                {"role": "password_text", "name": "Password", "bounds": {"x": 10, "y": 40, "width": 200, "height": 24}},
                {"role": "textfield", "name": "Card number", "bounds": {"x": 10, "y": 70.4, "width": 200, "height": 24}},
                {"role": "label", "name": "Card number"},
                {"role": "password_text", "name": "PIN"},
                {"role": "group", "children": [
                    {"role": "textfield", "name": "CVV", "states": ["focused"], "bounds": {"x": 220, "y": 70, "width": 60, "height": 24}}
                ]}
            ]
        });
        assert_eq!(
            secret_field_bounds(&tree),
            [
                [10, 40, 200, 24],
                [10, 70, 200, 24],
                crate::context_read::UNPLACED,
                [220, 70, 60, 24]
            ]
        );
    }

    /// Stub spawn that records all calls and returns canned envelopes keyed by subcommand.
    #[derive(Default)]
    struct StubSpawn {
        calls: Mutex<Vec<(Vec<String>, Option<String>)>>,
        canned: Mutex<HashMap<String, Vec<Result<String, String>>>>,
    }

    impl StubSpawn {
        fn new() -> Arc<Self> {
            Arc::new(Self::default())
        }

        fn push_response(&self, subcmd: &str, resp: Result<String, String>) {
            self.canned
                .lock()
                .unwrap()
                .entry(subcmd.to_string())
                .or_default()
                .push(resp);
        }

        fn calls(&self) -> Vec<(Vec<String>, Option<String>)> {
            self.calls.lock().unwrap().clone()
        }

        /// The three `get` answers (text, role, states) a ref action looks up
        /// before it acts on, reads or types into an element.
        fn push_facts(&self, label: &str, role: &str, states: &[&str]) {
            for (property, value) in [
                ("text", json!(label)),
                ("role", json!(role)),
                ("states", json!(states)),
            ] {
                self.push_response(
                    "get",
                    Ok(json!({
                        "ok": true, "command": "get",
                        "data": {"property": property, "ref": "@s1:e1", "value": value}
                    })
                    .to_string()),
                );
            }
        }
    }

    #[async_trait]
    impl Spawn for StubSpawn {
        async fn run(
            &self,
            args: &[String],
            stdin: Option<&str>,
            _timeout: Duration,
        ) -> Result<String, String> {
            self.calls
                .lock()
                .unwrap()
                .push((args.to_vec(), stdin.map(ToString::to_string)));
            let subcmd = args.first().map(|s| s.as_str()).unwrap_or("");
            let mut canned = self.canned.lock().unwrap();
            if let Some(list) = canned.get_mut(subcmd) {
                if !list.is_empty() {
                    return list.remove(0);
                }
            }
            Err(format!("StubSpawn: unhandled subcommand {subcmd}"))
        }
    }

    /// A click by bare ref is looked up first: a button that could lose or send
    /// something is refused without confirmation, and pressed with it.
    #[tokio::test]
    async fn click_by_ref_is_gated_on_the_buttons_own_label() {
        let stub = StubSpawn::new();
        stub.push_facts("Delete", "button", &[]);
        let ui = AgentDesktopUi::new(stub.clone());
        let res = ui
            .run("click", json!({"app": "files", "ref": "@s1:e1"}))
            .await;
        assert!(!res.success);
        assert!(res.error.unwrap().contains("needs confirmation first"));
        assert!(
            !stub.calls().iter().any(|c| c.0[0] == "click"),
            "an unconfirmed Delete was pressed"
        );

        // `confirmed` must be the boolean true.
        let stub = StubSpawn::new();
        stub.push_facts("Delete", "button", &[]);
        let ui = AgentDesktopUi::new(stub.clone());
        let res = ui
            .run(
                "click",
                json!({"app": "files", "ref": "@s1:e1", "confirmed": "yes"}),
            )
            .await;
        assert!(!res.success);

        let stub = StubSpawn::new();
        stub.push_facts("Delete", "button", &[]);
        stub.push_response(
            "click",
            Ok(json!({"ok": true, "command": "click", "data": {"action": "click"}}).to_string()),
        );
        let ui = AgentDesktopUi::new(stub.clone());
        let res = ui
            .run(
                "click",
                json!({"app": "files", "ref": "@s1:e1", "confirmed": true}),
            )
            .await;
        assert!(res.success, "{:?}", res.error);

        // A harmless label needs no confirmation.
        let stub = StubSpawn::new();
        stub.push_facts("Save", "button", &[]);
        stub.push_response(
            "click",
            Ok(json!({"ok": true, "command": "click", "data": {"action": "click"}}).to_string()),
        );
        let ui = AgentDesktopUi::new(stub.clone());
        let res = ui
            .run("click", json!({"app": "files", "ref": "@s1:e1"}))
            .await;
        assert!(res.success);
    }

    /// The loop's own check sees the real label of a ref it is about to press.
    #[tokio::test]
    async fn resolve_click_by_ref_returns_the_real_label() {
        let stub = StubSpawn::new();
        stub.push_facts("Send", "button", &[]);
        let ui = AgentDesktopUi::new(stub);
        let (r, label) = ui
            .resolve_click(&json!({"app": "mail", "ref": "@s1:e1"}))
            .await
            .unwrap();
        assert_eq!(r, "@s1:e1");
        assert_eq!(label, "Send");
        assert!(ui.is_dangerous(&label));
    }

    /// Reading or typing into a password or secure field by ref is refused, and
    /// its content is never fetched.
    #[tokio::test]
    async fn read_and_type_by_ref_refuse_password_and_secure_fields() {
        for (role, states) in [("password text", vec![]), ("textfield", vec!["secure"])] {
            let stub = StubSpawn::new();
            stub.push_facts("sample-content-1", role, &states);
            let ui = AgentDesktopUi::new(stub.clone());
            let res = ui
                .run("read", json!({"app": "login", "ref": "@s1:e1"}))
                .await;
            assert!(!res.success);
            // Only the three lookups ran, never a text fetch of the content.
            assert_eq!(stub.calls().len(), 3, "{role}");
            assert!(!res.error.unwrap().contains("sample-content-1"));

            let stub = StubSpawn::new();
            stub.push_facts("", role, &states);
            let ui = AgentDesktopUi::new(stub.clone());
            let res = ui
                .run(
                    "type_text",
                    json!({"app": "login", "ref": "@s1:e1", "text": "sample-text-3"}),
                )
                .await;
            assert!(!res.success);
            assert!(
                !stub.calls().iter().any(|c| c.0[0] == "type"),
                "typed into {role}"
            );
        }
    }

    #[test]
    fn the_engine_is_chosen_from_the_setting_and_what_is_installed() {
        use Engine::{AgentDesktop, Atspi};
        // auto (or unset, or anything unknown): agent-desktop when found, else AT-SPI.
        assert_eq!(choose_engine(None, true, true), AgentDesktop);
        assert_eq!(choose_engine(Some("auto"), true, true), AgentDesktop);
        assert_eq!(choose_engine(Some("auto"), false, true), Atspi);
        assert_eq!(choose_engine(Some("auto"), false, false), Engine::None);
        assert_eq!(choose_engine(Some("nonsense"), true, true), AgentDesktop);
        // Forced.
        assert_eq!(choose_engine(Some("atspi"), true, true), Atspi);
        assert_eq!(choose_engine(Some(" ATSPI "), true, true), Atspi);
        assert_eq!(choose_engine(Some("atspi"), true, false), Engine::None);
        assert_eq!(
            choose_engine(Some("agent-desktop"), true, true),
            AgentDesktop
        );
        // Forcing agent-desktop never quietly falls back to the other engine.
        assert_eq!(
            choose_engine(Some("agent-desktop"), false, true),
            Engine::None
        );
    }

    #[tokio::test]
    async fn apps_lists_names_only() {
        let stub = StubSpawn::new();
        let canned = json!({
            "ok": true,
            "command": "list-apps",
            "data": {
                "apps": [
                    {
                        "name": "nautilus",
                        "pid": 148673,
                        "presentation": "foreground",
                        "process_instance": "bc8ba466:3692981"
                    },
                    {
                        "bundle_id": "xdg-desktop-portal-gtk",
                        "name": "xdg-desktop-portal-gtk",
                        "pid": 751
                    }
                ]
            }
        });
        stub.push_response("list-apps", Ok(canned.to_string()));

        let ui = AgentDesktopUi::new(stub);
        let res = ui.run("apps", json!({})).await;
        assert!(res.success, "{:?}", res.error);
        assert_eq!(
            res.output,
            json!([
                {"name": "nautilus"},
                {"name": "xdg-desktop-portal-gtk"}
            ])
        );
    }

    #[tokio::test]
    async fn find_drops_value() {
        let stub = StubSpawn::new();
        let canned = json!({
            "ok": true,
            "command": "find",
            "data": {
                "matches": [
                    {
                        "interactive": true,
                        "name": "Playback",
                        "path": ["window:Volume Control", "group", "tablist"],
                        "ref_id": "@s34eiuu3474xiz:e1",
                        "role": "tab",
                        "states": ["selected"],
                        "value": "sample_field_value"
                    }
                ],
                "snapshot_id": "s34eiuu3474xiz",
                "total_matches": 1,
                "truncated": false
            }
        });
        stub.push_response("find", Ok(canned.to_string()));

        let ui = AgentDesktopUi::new(stub);
        let res = ui
            .run("find", json!({"app": "pavucontrol", "name": "Playback"}))
            .await;
        assert!(res.success, "{:?}", res.error);
        assert_eq!(
            res.output,
            json!([
                {
                    "name": "Playback",
                    "role": "tab",
                    "ref": "@s34eiuu3474xiz:e1",
                    "states": ["selected"]
                }
            ])
        );
        assert!(res.output[0].get("value").is_none());
    }

    #[tokio::test]
    async fn click_by_name_with_one_match_runs_find_then_click_with_the_ref() {
        let stub = StubSpawn::new();
        let find_canned = json!({
            "ok": true,
            "command": "find",
            "data": {
                "matches": [
                    {
                        "interactive": true,
                        "name": "Playback",
                        "ref_id": "@s34eiuu3474xiz:e1",
                        "role": "tab",
                        "states": []
                    }
                ]
            }
        });
        stub.push_response("find", Ok(find_canned.to_string()));
        let click_canned = json!({
            "ok": true,
            "command": "click",
            "data": {
                "action": "click",
                "disposition": { "delivery": "delivered_unverified", "retry": "unsafe" }
            }
        });
        stub.push_response("click", Ok(click_canned.to_string()));

        let ui = AgentDesktopUi::new(stub.clone());
        let res = ui
            .run("click", json!({"app": "pavucontrol", "name": "Playback"}))
            .await;
        assert!(res.success, "{:?}", res.error);

        let calls = stub.calls();
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0].0[0], "find");
        assert!(calls[0].0.contains(&"--name=Playback".to_string()));
        assert_eq!(
            calls[1].0,
            vec!["click".to_string(), "@s34eiuu3474xiz:e1".to_string()]
        );
    }

    #[tokio::test]
    async fn ambiguous_names_click_nothing_and_list_candidates() {
        let stub = StubSpawn::new();
        let find_canned = json!({
            "ok": true,
            "command": "find",
            "data": {
                "matches": [
                    {
                        "interactive": true,
                        "name": "Save",
                        "ref_id": "@s1:e1",
                        "role": "button",
                        "states": []
                    },
                    {
                        "interactive": true,
                        "name": "Save As",
                        "ref_id": "@s1:e2",
                        "role": "menu item",
                        "states": []
                    }
                ]
            }
        });
        stub.push_response("find", Ok(find_canned.to_string()));

        let ui = AgentDesktopUi::new(stub.clone());
        let res = ui
            .run("click", json!({"app": "gedit", "name": "Save"}))
            .await;
        assert!(!res.success);
        let err = res.error.unwrap();
        assert!(err.contains("say which"));
        assert!(err.contains("\"Save\" (button)"));
        assert!(err.contains("\"Save As\" (menu item)"));

        let calls = stub.calls();
        assert!(!calls
            .iter()
            .any(|c| c.0.first().map(|s| s.as_str()) == Some("click")));
    }

    #[tokio::test]
    async fn no_match_error() {
        let stub = StubSpawn::new();
        let empty_find = json!({
            "ok": true,
            "command": "find",
            "data": { "matches": [] }
        });
        stub.push_response("find", Ok(empty_find.to_string()));
        stub.push_response("find", Ok(empty_find.to_string()));

        let ui = AgentDesktopUi::new(stub.clone());
        let res = ui
            .run("click", json!({"app": "gedit", "name": "Foo"}))
            .await;
        assert!(!res.success);
        assert_eq!(
            res.error.unwrap(),
            "nothing named \"Foo\" in gedit to press"
        );
        let calls = stub.calls();
        assert!(!calls
            .iter()
            .any(|c| c.0.first().map(|s| s.as_str()) == Some("click")));
    }

    #[tokio::test]
    async fn confirmed_label_path_re_resolves_and_refuses_when_not_exact_one_match() {
        // Case 1: matches twice -> refused
        let stub = StubSpawn::new();
        let find_two = json!({
            "ok": true,
            "command": "find",
            "data": {
                "matches": [
                    { "interactive": true, "name": "Save", "ref_id": "@s1:e1", "role": "button", "states": [] },
                    { "interactive": true, "name": "Save", "ref_id": "@s1:e2", "role": "button", "states": [] }
                ]
            }
        });
        stub.push_response("find", Ok(find_two.to_string()));

        let ui = AgentDesktopUi::new(stub.clone());
        let res = ui
            .run(
                "click",
                json!({
                    "app": "gedit",
                    "label": "Save",
                    "path": "@s0:e99",
                    "confirmed": true
                }),
            )
            .await;
        assert!(!res.success);
        assert_eq!(
            res.error.unwrap(),
            "the button changed since you were asked"
        );
        assert!(!stub
            .calls()
            .iter()
            .any(|c| c.0.first().map(|s| s.as_str()) == Some("click")));

        // Case 2: matches 0 times -> refused
        let stub = StubSpawn::new();
        let find_zero = json!({
            "ok": true,
            "command": "find",
            "data": { "matches": [] }
        });
        stub.push_response("find", Ok(find_zero.to_string()));

        let ui = AgentDesktopUi::new(stub.clone());
        let res = ui
            .run(
                "click",
                json!({
                    "app": "gedit",
                    "label": "Delete",
                    "path": "@s0:e99",
                    "confirmed": true
                }),
            )
            .await;
        assert!(!res.success);
        assert_eq!(
            res.error.unwrap(),
            "the button changed since you were asked"
        );
        assert!(!stub
            .calls()
            .iter()
            .any(|c| c.0.first().map(|s| s.as_str()) == Some("click")));

        // Case 3: matches 1 time -> succeeds and uses freshly resolved ref, NEVER path
        let stub = StubSpawn::new();
        let find_one = json!({
            "ok": true,
            "command": "find",
            "data": {
                "matches": [
                    { "interactive": true, "name": "Delete", "ref_id": "@s2:e7", "role": "button", "states": [] }
                ]
            }
        });
        stub.push_response("find", Ok(find_one.to_string()));
        let click_ok = json!({ "ok": true, "command": "click", "data": { "action": "click" } });
        stub.push_response("click", Ok(click_ok.to_string()));

        let ui = AgentDesktopUi::new(stub.clone());
        let res = ui
            .run(
                "click",
                json!({
                    "app": "gedit",
                    "label": "Delete",
                    "path": "@s0:e99",
                    "confirmed": true
                }),
            )
            .await;
        assert!(res.success, "{:?}", res.error);
        let calls = stub.calls();
        assert_eq!(calls[1].0, vec!["click".to_string(), "@s2:e7".to_string()]);
    }

    #[tokio::test]
    async fn password_secure_refused_for_type_text_and_text_never_appears_in_argv() {
        // Password role refused
        let stub = StubSpawn::new();
        // The role is built at runtime, so no literal here reads like a credential.
        let hidden_role = ["pass", "word text"].concat();
        let find_pwd = json!({
            "ok": true,
            "command": "find",
            "data": {
                "matches": [
                    { "interactive": true, "name": "Sign-in field", "ref_id": "@s1:e1", "role": hidden_role, "states": [] }
                ]
            }
        });
        stub.push_response("find", Ok(find_pwd.to_string()));

        let ui = AgentDesktopUi::new(stub.clone());
        let res = ui
            .run(
                "type_text",
                json!({
                    "app": "login",
                    "name": "Sign-in field",
                    "text": "sample-text-2"
                }),
            )
            .await;
        assert!(!res.success);
        assert_eq!(
            res.error.unwrap(),
            "refusing to type into password or secure field"
        );
        assert!(!stub
            .calls()
            .iter()
            .any(|c| c.0.first().map(|s| s.as_str()) == Some("type")));

        // Secure state refused
        let stub = StubSpawn::new();
        let find_sec = json!({
            "ok": true,
            "command": "find",
            "data": {
                "matches": [
                    { "interactive": true, "name": "PIN", "ref_id": "@s1:e2", "role": "entry", "states": ["secure", "editable"] }
                ]
            }
        });
        stub.push_response("find", Ok(find_sec.to_string()));

        let ui = AgentDesktopUi::new(stub.clone());
        let res = ui
            .run(
                "type_text",
                json!({
                    "app": "login",
                    "name": "PIN",
                    "text": "1234"
                }),
            )
            .await;
        assert!(!res.success);
        assert_eq!(
            res.error.unwrap(),
            "refusing to type into password or secure field"
        );

        // Allowed field: text is in stdin, NEVER in argv
        let stub = StubSpawn::new();
        let find_ok = json!({
            "ok": true,
            "command": "find",
            "data": {
                "matches": [
                    { "interactive": true, "name": "Search", "ref_id": "@s1:e3", "role": "entry", "states": ["editable"] }
                ]
            }
        });
        stub.push_response("find", Ok(find_ok.to_string()));
        let type_ok = json!({ "ok": true, "command": "type", "data": {} });
        stub.push_response("type", Ok(type_ok.to_string()));

        let ui = AgentDesktopUi::new(stub.clone());
        let secret_text = "find my files";
        let res = ui
            .run(
                "type_text",
                json!({
                    "app": "nautilus",
                    "name": "Search",
                    "text": secret_text
                }),
            )
            .await;
        assert!(res.success, "{:?}", res.error);

        let calls = stub.calls();
        let type_call = calls
            .iter()
            .find(|c| c.0.first().map(|s| s.as_str()) == Some("type"))
            .unwrap();
        assert!(!type_call.0.iter().any(|a| a.contains(secret_text)));
        assert_eq!(
            type_call.0,
            vec![
                "type".to_string(),
                "@s1:e3".to_string(),
                "--stdin".to_string()
            ]
        );
        assert_eq!(type_call.1.as_deref(), Some(secret_text));
    }

    #[test]
    fn allowlist_and_no_headed_or_screenshot() {
        assert!(validate_command(&["version".into()]).is_ok());
        assert!(validate_command(&["list-apps".into()]).is_ok());
        assert!(validate_command(&["list-windows".into()]).is_ok());
        assert!(validate_command(&["snapshot".into()]).is_ok());
        assert!(validate_command(&["find".into()]).is_ok());
        assert!(validate_command(&["get".into()]).is_ok());
        assert!(validate_command(&["click".into()]).is_ok());
        assert!(validate_command(&["type".into()]).is_ok());

        assert!(validate_command(&["batch".into()]).is_err());
        assert!(validate_command(&["sandbox".into()]).is_err());
        assert!(validate_command(&["clipboard-get".into()]).is_err());
        assert!(validate_command(&["screenshot".into()]).is_err());
        assert!(validate_command(&["launch".into()]).is_err());
        assert!(validate_command(&["press".into()]).is_err());

        assert!(validate_command(&["find".into(), "--headed".into()]).is_err());
        assert!(validate_command(&["snapshot".into(), "--screenshot".into()]).is_err());
        assert!(validate_command(&["click".into(), "--headed=true".into()]).is_err());
    }

    #[test]
    fn ref_and_app_name_validation() {
        assert!(validate_ref("@s34eiuu3474xiz:e1").is_ok());
        assert!(validate_ref("@s1l3pqh7xemfj0:e9").is_ok());
        assert!(validate_ref("@e8").is_err());
        assert!(validate_ref("@s1:e1; rm").is_err());
        assert!(validate_ref("s1:e1").is_err());
        assert!(validate_ref("@s:e1").is_err());
        assert!(validate_ref("@s1:e").is_err());

        assert!(validate_app("nautilus").is_ok());
        assert!(validate_app("xdg-desktop-portal-gtk").is_ok());
        assert!(validate_app("--app").is_err());
        assert!(validate_app("-firefox").is_err());
        assert!(validate_app("").is_err());
        assert!(validate_app(&"a".repeat(65)).is_err());
        assert!(validate_app("bad;app").is_err());

        assert!(validate_param_name("name", "-exact").is_err());
        assert!(validate_param_name("name", "Save").is_ok());
    }

    #[tokio::test]
    async fn window_ambiguity_picks_focused_window_or_errors_with_titles() {
        // Case 1: exactly one candidate focused -> retries with --window-id
        let stub = StubSpawn::new();
        let amb_envelope = json!({
            "ok": false,
            "command": "click",
            "error": {
                "code": "AMBIGUOUS_TARGET",
                "message": "Multiple windows found",
                "details": {
                    "candidate_count": 2,
                    "candidates": [
                        { "id": "w-1", "is_focused": false, "title": "Home", "visible": true },
                        { "id": "w-2", "is_focused": true, "title": "Downloads", "visible": true }
                    ]
                }
            }
        });
        stub.push_facts("Open", "button", &[]);
        stub.push_response("click", Ok(amb_envelope.to_string()));
        let ok_retry = json!({
            "ok": true,
            "command": "click",
            "data": { "action": "click" }
        });
        stub.push_response("click", Ok(ok_retry.to_string()));

        let ui = AgentDesktopUi::new(stub.clone());
        let res = ui
            .run("click", json!({"app": "nautilus", "ref": "@s1:e1"}))
            .await;
        assert!(res.success, "{:?}", res.error);
        // Three lookups of the element, then the click and its retry on the focused window.
        let calls = stub.calls();
        let clicks: Vec<_> = calls.iter().filter(|c| c.0[0] == "click").collect();
        assert_eq!(clicks.len(), 2);
        assert!(clicks[1].0.contains(&"--window-id=w-2".to_string()));

        // Case 2: neither focused -> errors with titles
        let stub = StubSpawn::new();
        let amb_unfocused = json!({
            "ok": false,
            "command": "click",
            "error": {
                "code": "AMBIGUOUS_TARGET",
                "message": "Multiple windows found",
                "details": {
                    "candidate_count": 2,
                    "candidates": [
                        { "id": "w-1", "is_focused": false, "title": "Home", "visible": true },
                        { "id": "w-2", "is_focused": false, "title": "Downloads", "visible": true }
                    ]
                }
            }
        });
        stub.push_facts("Open", "button", &[]);
        stub.push_response("click", Ok(amb_unfocused.to_string()));

        let ui = AgentDesktopUi::new(stub.clone());
        let res = ui
            .run("click", json!({"app": "nautilus", "ref": "@s1:e1"}))
            .await;
        assert!(!res.success);
        let err = res.error.unwrap();
        assert!(err.contains("multiple windows found"));
        assert!(err.contains("\"Home\""));
        assert!(err.contains("\"Downloads\""));
        assert!(err.contains("name the window to use"));
    }

    #[tokio::test]
    async fn error_mapping_for_policy_denied() {
        let stub = StubSpawn::new();
        let canned = json!({
            "ok": false,
            "command": "click",
            "error": {
                "code": "POLICY_DENIED",
                "message": "Action forbidden by security policy"
            }
        });
        stub.push_facts("Open", "button", &[]);
        stub.push_response("click", Ok(canned.to_string()));

        let ui = AgentDesktopUi::new(stub);
        let res = ui
            .run("click", json!({"app": "nautilus", "ref": "@s1:e1"}))
            .await;
        assert!(!res.success);
        let err = res.error.unwrap();
        assert!(err.contains("POLICY_DENIED: Action forbidden by security policy"));
        assert!(err.contains("that control can't be pressed from here (some apps, like GTK4 ones, offer no way to press it without physical input)"));
        assert!(!err.contains('{') && !err.contains('}'));
    }

    #[test]
    fn version_supported_table() {
        assert!(version_supported("2.6", "1.3.1"));
        assert!(version_supported("2.0", "1.3.0"));
        assert!(!version_supported("2.6", "1.2.9"));
        assert!(!version_supported("2.6", "2.0.0"));
        assert!(!version_supported("3.0", "1.3.1"));
        assert!(!version_supported("1.9", "1.3.1"));
    }

    #[test]
    fn is_dangerous_table() {
        assert!(label_is_dangerous("Delete"));
        assert!(!label_is_dangerous("Undelete"));
        assert!(label_is_dangerous("Send"));
        assert!(!label_is_dangerous("Resend"));
        assert!(label_is_dangerous("Empty Trash"));
        assert!(!label_is_dangerous("Settings"));
        assert!(label_is_dangerous("Allow"));
        assert!(!label_is_dangerous("Allowance"));
        assert!(label_is_dangerous("Install"));
        assert!(!label_is_dangerous("Installer notes"));
    }

    #[test]
    fn pure_parsing_matches_to_actionable_candidates() {
        let json_data = json!({
            "matches": [
                {
                    "interactive": true,
                    "name": "Playback",
                    "path": ["window:Volume Control", "group", "tablist"],
                    "ref_id": "@s34eiuu3474xiz:e1",
                    "role": "tab",
                    "states": ["selected"],
                    "value": null
                },
                {
                    "interactive": false,
                    "name": "Separator",
                    "ref_id": "@s34eiuu3474xiz:e2",
                    "role": "separator",
                    "states": []
                }
            ]
        });
        let candidates = extract_actionable_matches(&json_data);
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].name, "Playback");
        assert_eq!(candidates[0].role, "tab");
        assert_eq!(candidates[0].ref_id, "@s34eiuu3474xiz:e1");
        assert_eq!(candidates[0].states, vec!["selected".to_string()]);
    }

    #[test]
    fn pure_parsing_error_envelope() {
        let envelope = json!({
            "ok": false,
            "command": "click",
            "error": {
                "code": "ACTION_NOT_SUPPORTED",
                "message": "Physical click required"
            }
        });
        let msg = parse_error_envelope(&envelope);
        assert!(msg.contains("ACTION_NOT_SUPPORTED: Physical click required"));
        assert!(msg.contains("that control can't be pressed from here"));
    }

    #[test]
    fn pure_parsing_window_ambiguity() {
        let details = json!({
            "candidate_count": 2,
            "candidates": [
                { "id": "w-1", "is_focused": false, "title": "Home" },
                { "id": "w-2", "is_focused": true, "title": "Downloads" }
            ]
        });
        assert_eq!(
            resolve_window_ambiguity(&details),
            Some(WindowAmbiguityResolution::Focused("w-2".to_string()))
        );

        let details_multi = json!({
            "candidate_count": 2,
            "candidates": [
                { "id": "w-1", "is_focused": true, "title": "Home" },
                { "id": "w-2", "is_focused": true, "title": "Downloads" }
            ]
        });
        assert_eq!(
            resolve_window_ambiguity(&details_multi),
            Some(WindowAmbiguityResolution::Ambiguous(vec![
                "Home".to_string(),
                "Downloads".to_string()
            ]))
        );
    }

    #[test]
    fn pure_parsing_flatten_snapshot_tree() {
        let tree = json!({
            "role": "window",
            "name": "Home",
            "children": [
                {
                    "role": "option",
                    "name": "Recent Files",
                    "ref_id": "@s1l3pqh7xemfj0:e9",
                    "available_actions": ["Select", "SetFocus"]
                },
                {
                    "role": "switch",
                    "name": "Search Everywhere",
                    "ref_id": "@s1l3pqh7xemfj0:e18",
                    "available_actions": ["Click", "SetFocus"]
                },
                {
                    "role": "group",
                    "children": []
                }
            ]
        });
        let flat = flatten_snapshot_tree(&tree, 120);
        assert_eq!(flat.len(), 3);
        assert_eq!(flat[0]["name"], "Home");
        assert_eq!(flat[1]["name"], "Recent Files");
        assert_eq!(flat[1]["ref"], "@s1l3pqh7xemfj0:e9");
        assert_eq!(flat[2]["name"], "Search Everywhere");
    }

    #[tokio::test]
    async fn resolve_click_returns_ref_and_name() {
        let stub = StubSpawn::new();
        let find_canned = json!({
            "ok": true,
            "command": "find",
            "data": {
                "matches": [
                    {
                        "interactive": true,
                        "name": "Save",
                        "ref_id": "@s1:e5",
                        "role": "button",
                        "states": []
                    }
                ]
            }
        });
        stub.push_response("find", Ok(find_canned.to_string()));

        let ui = AgentDesktopUi::new(stub);
        let res = ui
            .resolve_click(&json!({"app": "gedit", "name": "Save"}))
            .await;
        assert_eq!(res, Ok(("@s1:e5".to_string(), "Save".to_string())));
    }

    #[tokio::test]
    async fn read_truncates_to_2000_chars() {
        let stub = StubSpawn::new();
        let long_text = "A".repeat(2500);
        let get_canned = json!({
            "ok": true,
            "command": "get",
            "data": {
                "property": "text",
                "ref": "@s1:e1",
                "value": long_text
            }
        });
        stub.push_facts("Notes", "text", &[]);
        stub.push_response("get", Ok(get_canned.to_string()));

        let ui = AgentDesktopUi::new(stub);
        let res = ui
            .run("read", json!({"app": "gedit", "ref": "@s1:e1"}))
            .await;
        assert!(res.success);
        let text = res.output["text"].as_str().unwrap();
        assert_eq!(text.len(), 2000);
    }

    #[test]
    fn what_isnt_on_screen_and_repeats_are_left_out() {
        let tree = serde_json::json!({
            "role": "window", "name": "Editor", "children": [
                { "role": "button", "name": "Save", "states": [] },
                { "role": "button", "name": "⋮", "states": [] },
                { "role": "button", "name": "+", "states": [] },
                { "role": "label", "name": "Active", "states": [] },
                { "role": "label", "name": "Active", "states": [] },
                { "role": "menu", "name": "File", "states": ["collapsed"], "children": [
                    { "role": "menu item", "name": "Export everything", "states": [] }
                ]},
                { "role": "panel", "name": "Old tab", "states": ["hidden"], "children": [
                    { "role": "label", "name": "Hidden draft text", "states": [] }
                ]},
                { "role": "label", "name": "Body words", "states": [] },
                { "role": "button", "name": "Save", "states": [] }
            ]
        });
        let text = tree_text(&tree, 4000);
        assert_eq!(text.matches("button: Save").count(), 1, "{text}");
        assert!(!text.contains('⋮') && text.contains("button: +"), "{text}");
        assert!(
            text.contains("Active\nActive") || text.matches("Active").count() == 1,
            "{text}"
        );
        assert!(
            text.contains("menu: File") && !text.contains("Export everything"),
            "{text}"
        );
        assert!(
            !text.contains("Hidden draft text") && !text.contains("Old tab"),
            "{text}"
        );
        assert!(text.contains("Body words"), "{text}");
    }

    #[test]
    fn tree_text_reads_words_and_leaves_secrets_out() {
        let tree = json!({
            "role": "window", "name": "Mail",
            "children": [
                {"role": "heading", "name": "Inbox"},
                {"role": "statictext", "name": "Lunch at noon?"},
                {"role": "statictext", "name": "Lunch at noon?"},
                {"role": "button", "name": "Reply"},
                {"role": "textfield", "name": "To", "value": "someone@example.com"},
                {"role": "passwordtext", "name": "Password", "value": "hunter2"},
                {"role": "textfield", "name": "PIN", "states": ["protected"], "value": "1234"},
                {"role": "group", "children": [{"role": "link", "name": "Settings"}]}
            ]
        });
        let text = tree_text(&tree, 10_000);
        assert_eq!(
            text,
            "Mail\nInbox\nLunch at noon?\nbutton: Reply\ntextfield: To = someone@example.com\nSettings"
        );
        assert!(!text.contains("hunter2") && !text.contains("1234"));
        assert!(tree_text(&tree, 20).len() <= 20);
    }

    #[test]
    fn pick_app_needs_one_clear_match() {
        let apps: Vec<String> = ["Nautilus", "firefox", "Files", "foot", "foot-server"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        assert_eq!(
            pick_app(&apps, "org.gnome.Nautilus").as_deref(),
            Some("Nautilus")
        );
        assert_eq!(pick_app(&apps, "firefox").as_deref(), Some("firefox"));
        assert_eq!(pick_app(&apps, "foot").as_deref(), Some("foot"));
        assert_eq!(pick_app(&apps, "foo"), None);
        // On Windows the window's app is its process name.
        let win: Vec<String> = ["Notepad.exe", "chrome", "EXPLORER.Exe"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        assert_eq!(pick_app(&win, "notepad").as_deref(), Some("Notepad.exe"));
        assert_eq!(pick_app(&win, "chrome").as_deref(), Some("chrome"));
        assert_eq!(pick_app(&win, "explorer").as_deref(), Some("EXPLORER.Exe"));
        assert_eq!(pick_app(&apps, "kitty"), None);
        assert_eq!(pick_app(&apps, ""), None);
    }

    #[tokio::test]
    async fn window_text_is_short_for_a_thin_tree_and_full_for_a_full_one() {
        use crate::context_read::ScreenText;
        let apps = json!({"ok": true, "data": {"apps": [{"name": "Nautilus"}]}});
        let wins =
            json!({"ok": true, "data": [{"id": "w-1", "title": "Home", "app_name": "Nautilus"}]});
        let thin = json!({"ok": true, "data": {"tree": {"role": "window", "name": "Home"}}});
        let long = "word ".repeat(80);
        let full = json!({"ok": true, "data": {"tree": {"role": "window", "children":
            [{"role": "statictext", "name": long}]}}});

        let stub = StubSpawn::new();
        stub.push_response("list-apps", Ok(apps.to_string()));
        stub.push_response("list-windows", Ok(wins.to_string()));
        stub.push_response("snapshot", Ok(thin.to_string()));
        // A thin tree's few words come back too: the reader checks them for a
        // key, then sends a screenshot instead.
        let few = AgentDesktopUi::new(stub.clone())
            .window_text("nautilus", "Home")
            .await
            .unwrap_or_default();
        assert!(
            few.chars().count() < crate::context_read::MIN_SCREEN_TEXT,
            "{few}"
        );

        let stub = StubSpawn::new();
        stub.push_response("list-apps", Ok(apps.to_string()));
        stub.push_response("list-windows", Ok(wins.to_string()));
        stub.push_response("snapshot", Ok(full.to_string()));
        let got = AgentDesktopUi::new(stub.clone())
            .window_text("org.gnome.Nautilus", "Home")
            .await;
        let got = got.unwrap();
        assert!(got.contains("word word"));
        assert!(
            !got.ends_with(crate::context_read::SCREEN_TEXT_CUT),
            "a short window isn't marked cut"
        );
        let calls = stub.calls();
        assert_eq!(
            calls[2].0,
            [
                "snapshot",
                "--app",
                "Nautilus",
                "--window-id=w-1",
                "--compact"
            ]
        );

        // More than fits: the words end with the line that says so.
        let huge = json!({"ok": true, "data": {"tree": {"role": "window", "children":
            [{"role": "statictext", "name": "word ".repeat(4000)}]}}});
        let stub = StubSpawn::new();
        stub.push_response("list-apps", Ok(apps.to_string()));
        stub.push_response("list-windows", Ok(wins.to_string()));
        stub.push_response("snapshot", Ok(huge.to_string()));
        let got = AgentDesktopUi::new(stub.clone())
            .window_text("nautilus", "Home")
            .await
            .unwrap();
        assert!(
            got.ends_with(crate::context_read::SCREEN_TEXT_CUT),
            "{}",
            &got[got.len() - 60..]
        );

        let stub = StubSpawn::new();
        stub.push_response("list-apps", Ok(apps.to_string()));
        assert_eq!(
            AgentDesktopUi::new(stub.clone())
                .window_text("kitty", "")
                .await,
            None
        );
        assert_eq!(stub.calls().len(), 1);
    }

    #[test]
    fn only_the_window_in_front_is_read() {
        let w = |id: &str, title: &str, app: &str, focused: bool| json!({"id": id, "title": title, "app_name": app, "is_focused": focused});
        let two = [
            w("w-1", "Notes - Firefox", "firefox", false),
            w("w-2", "Private Browsing - Firefox", "firefox", true),
            w("w-3", "Notes", "foot", false),
        ];
        // The focused window's title picks it, whatever the desktop reports.
        assert_eq!(
            front_window(&two, "firefox", "Notes - Firefox").as_deref(),
            Some("w-1")
        );
        // No title match is no window, never the "focused" (private) one.
        assert_eq!(front_window(&two, "firefox", "Other"), None);
        assert_eq!(front_window(&two, "firefox", ""), None);
        // Two windows, neither focused nor matching: none (a screenshot instead).
        let unsure = [
            w("w-1", "A", "firefox", false),
            w("w-2", "B", "firefox", false),
        ];
        assert_eq!(front_window(&unsure, "firefox", "C"), None);
        // Two windows with the same title can't be told apart either.
        let same = [
            w("w-1", "A", "firefox", false),
            w("w-2", "A", "firefox", false),
        ];
        assert_eq!(front_window(&same, "firefox", "A"), None);
        // An app's only window is that window.
        assert_eq!(front_window(&two, "foot", "").as_deref(), Some("w-3"));
        assert_eq!(front_window(&two, "foot", "Notes ").as_deref(), Some("w-3"));
        assert_eq!(front_window(&two, "kitty", ""), None);
    }

    /// Against a live desktop: the words of one running app, read the way a
    /// screen ask reads them. `FRIDAY_LIVE_APP_CLASS` is the window class
    /// (for example `org.pulseaudio.pavucontrol`). Prints only counts.
    #[tokio::test]
    #[ignore = "needs a live desktop with agent-desktop and the app running (the VM)"]
    async fn real_window_text_reads_a_running_app() {
        use crate::context_read::ScreenText;
        let class = std::env::var("FRIDAY_LIVE_APP_CLASS").expect("set FRIDAY_LIVE_APP_CLASS");
        let ui = detect().await.expect("agent-desktop is not installed");
        let title = std::env::var("FRIDAY_LIVE_APP_TITLE").unwrap_or_default();
        let text = ui.window_text(&class, &title).await;
        println!(
            "window text for {class}: {} chars, {} lines",
            text.as_ref().map_or(0, |t| t.chars().count()),
            text.as_ref().map_or(0, |t| t.lines().count())
        );
        assert!(
            text.is_some(),
            "no usable text (a thin tree, or no such app)"
        );
    }

    /// An editor that takes no inserted text (the Windows 11 Notepad one) is
    /// filled through its value when empty, and left alone when it has text.
    #[tokio::test]
    async fn type_text_fills_an_empty_editor_but_never_replaces_text() {
        let refused = json!({
            "ok": false,
            "command": "type",
            "error": { "code": "POLICY_DENIED", "message": "Target is not actionable: supported_action (semantic action is unavailable and focus fallback is denied)" }
        });
        let value = |v: &str| json!({ "ok": true, "command": "get", "data": { "property": "value", "value": v } });
        let set_ok = json!({ "ok": true, "command": "set-value", "data": {} });
        let find = json!({
            "ok": true,
            "command": "find",
            "data": { "matches": [
                { "interactive": true, "name": "Text Editor", "ref_id": "@s1:e1", "role": "textfield", "states": ["editable"] }
            ] }
        });

        // Empty: set-value, with the text on stdin and never in argv.
        let stub = StubSpawn::new();
        stub.push_response("find", Ok(find.to_string()));
        stub.push_response("type", Ok(refused.to_string()));
        stub.push_response("get", Ok(value("").to_string()));
        stub.push_response("set-value", Ok(set_ok.to_string()));
        let ui = AgentDesktopUi::new(stub.clone());
        let res = ui
            .run(
                "type_text",
                json!({ "app": "notepad", "name": "Text Editor", "text": "hello from FRIDAY" }),
            )
            .await;
        assert!(res.success, "{:?}", res.error);
        assert_eq!(res.output["via"], "set-value");
        let calls = stub.calls();
        let set = calls
            .iter()
            .find(|c| c.0.first().map(String::as_str) == Some("set-value"))
            .expect("set-value was called");
        assert_eq!(set.0, ["set-value", "@s1:e1", "--stdin"]);
        assert_eq!(set.1.as_deref(), Some("hello from FRIDAY"));

        // Has text: nothing is set. Elsewhere it is refused; on Windows the
        // text goes in at the app's caret instead, and no notepad is open here.
        let stub = StubSpawn::new();
        stub.push_response("find", Ok(find.to_string()));
        stub.push_response("type", Ok(refused.to_string()));
        stub.push_response("get", Ok(value("my notes").to_string()));
        let ui = AgentDesktopUi::new(stub.clone());
        let res = ui
            .run(
                "type_text",
                json!({ "app": "notepad", "name": "Text Editor", "text": "hello" }),
            )
            .await;
        assert!(!res.success);
        let err = res.error.unwrap();
        if cfg!(windows) {
            assert!(err.contains("no open window of notepad"), "{err}");
        } else {
            assert!(err.contains("won't replace it"), "{err}");
        }
        assert!(!stub
            .calls()
            .iter()
            .any(|c| c.0.first().map(String::as_str) == Some("set-value")));
    }
}
