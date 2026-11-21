//! Desktop capability adapter (Linux, FRIDAY OS): native control of windows,
//! workspaces, apps, media, volume, brightness and the power profile.
//!
//! Typed verbs only. Nothing here runs a shell string: every command is a
//! program with an argument vector, a window is picked by matching the
//! compositor's own window list (never a user-supplied address), and an app is
//! launched by desktop-entry id or by an executable name that is a bare word.
//! Each verb has a [`Risk`] class so the permission layer can decide what runs
//! without asking again and what always needs a yes.
//!
//! The compositor is Hyprland (`hyprctl`); media is MPRIS (`playerctl`),
//! sound is PipeWire (`wpctl`), brightness is `brightnessctl`. The commands
//! run through an [`Exec`] so tests drive the adapter with a recording stub.

use async_trait::async_trait;
use serde_json::{json, Value};
use std::sync::Arc;

use crate::adapters::ActionAdapter;
use crate::request::{ActionRequest, ActionResult};

/// What a verb can do to the person's session; the consent rule hangs on it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Risk {
    /// Looks only (list windows, now playing).
    Read,
    /// Changes the session in a way that is undone by doing the opposite
    /// (focus, move, launch, volume, media).
    Reversible,
    /// May lose work or can't be taken back (close a window).
    Destructive,
}

/// The risk class of a desktop verb; `None` for a verb this adapter lacks.
pub fn risk_of(action: &str) -> Option<Risk> {
    Some(match action {
        "windows" | "active_window" | "workspaces" | "now_playing" | "find_app"
        | "system_health" => Risk::Read,
        "focus_window" | "move_window" | "switch_workspace" | "launch_app" | "media" | "volume"
        | "brightness" | "power_profile" => Risk::Reversible,
        "close_window" => Risk::Destructive,
        _ => return None,
    })
}

/// Runs a program and returns its stdout.
pub trait Exec: Send + Sync {
    fn run(&self, program: &str, args: &[String]) -> Result<String, String>;
}

/// Runs the real program.
pub struct SystemExec;

/// The running Hyprland session's signature: the environment's, or (for a
/// daemon started by systemd, which doesn't inherit it) the newest instance
/// in `$XDG_RUNTIME_DIR/hypr`. `None` when there is no Hyprland to talk to.
pub fn hypr_signature() -> Option<String> {
    if let Ok(sig) = std::env::var("HYPRLAND_INSTANCE_SIGNATURE") {
        if !sig.is_empty() {
            return Some(sig);
        }
    }
    let dir = std::path::Path::new(&std::env::var("XDG_RUNTIME_DIR").ok()?).join("hypr");
    std::fs::read_dir(dir)
        .ok()?
        .filter_map(|e| e.ok())
        .filter(|e| e.path().join(".socket.sock").exists())
        .max_by_key(|e| e.metadata().and_then(|m| m.modified()).ok())
        .and_then(|e| e.file_name().into_string().ok())
}

impl Exec for SystemExec {
    fn run(&self, program: &str, args: &[String]) -> Result<String, String> {
        let mut cmd = std::process::Command::new(program);
        cmd.args(args);
        if program == "hyprctl" {
            if let Some(sig) = hypr_signature() {
                cmd.env("HYPRLAND_INSTANCE_SIGNATURE", sig);
            }
        }
        let out = cmd
            .output()
            .map_err(|e| format!("couldn't run {program}: {e}"))?;
        if out.status.success() {
            Ok(String::from_utf8_lossy(&out.stdout).to_string())
        } else {
            let err = String::from_utf8_lossy(&out.stderr);
            Err(format!("{program} failed: {}", err.trim()))
        }
    }
}

pub struct DesktopAdapter {
    exec: Arc<dyn Exec>,
    lua: std::sync::OnceLock<bool>,
}

impl Default for DesktopAdapter {
    fn default() -> Self {
        Self::new()
    }
}

/// A window, as much of it as an answer needs.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Window {
    address: String,
    class: String,
    title: String,
    workspace: String,
}

impl DesktopAdapter {
    pub fn new() -> Self {
        Self::with_exec(Arc::new(SystemExec))
    }

    pub fn with_exec(exec: Arc<dyn Exec>) -> Self {
        Self {
            exec,
            lua: std::sync::OnceLock::new(),
        }
    }

    /// Hyprland 0.55 and newer takes `hyprctl dispatch` in Lua
    /// (`hl.dsp.focus({ ... })`); older ones take the classic
    /// `focuswindow address:0x...`. An unreadable version means the newer.
    fn lua_dispatch(&self) -> bool {
        *self.lua.get_or_init(|| {
            let out = self
                .exec
                .run("hyprctl", &["version".into()])
                .unwrap_or_default();
            let version = out.split_whitespace().nth(1).unwrap_or("");
            let mut nums = version.split('.').map(|n| n.parse::<u32>().unwrap_or(0));
            let (major, minor) = (nums.next().unwrap_or(0), nums.next().unwrap_or(0));
            version.is_empty() || (major, minor) >= (0, 55)
        })
    }

    fn focus(&self, address: &str) -> Result<String, String> {
        if self.lua_dispatch() {
            self.hypr_lua(&format!(
                "hl.dsp.focus({{ window = \"address:{address}\" }})"
            ))
        } else {
            self.dispatch(&["focuswindow", &format!("address:{address}")])
        }
    }

    fn move_to_workspace(&self, address: &str, ws: &str) -> Result<String, String> {
        if self.lua_dispatch() {
            self.hypr_lua(&format!(
                "hl.dsp.window.move({{ workspace = \"{}\", follow = false, window = \"address:{address}\" }})",
                lua_workspace(ws)
            ))
        } else {
            self.dispatch(&["movetoworkspacesilent", &format!("{ws},address:{address}")])
        }
    }

    fn close(&self, address: &str) -> Result<String, String> {
        if self.lua_dispatch() {
            self.hypr_lua(&format!(
                "hl.dsp.window.close({{ window = \"address:{address}\" }})"
            ))
        } else {
            self.dispatch(&["closewindow", &format!("address:{address}")])
        }
    }

    fn goto_workspace(&self, ws: &str) -> Result<String, String> {
        if self.lua_dispatch() {
            self.hypr_lua(&format!(
                "hl.dsp.focus({{ workspace = \"{}\" }})",
                lua_workspace(ws)
            ))
        } else {
            self.dispatch(&["workspace", ws])
        }
    }

    fn exec_app(&self, app: &str) -> Result<String, String> {
        if self.lua_dispatch() {
            self.hypr_lua(&format!("hl.dsp.exec_cmd(\"{app}\")"))
        } else {
            self.dispatch(&["exec", app])
        }
    }

    /// `hyprctl dispatch <lua expression>`. Every value in it was validated
    /// (hex address, bare name, workspace charset), so none can hold a quote.
    fn hypr_lua(&self, expression: &str) -> Result<String, String> {
        self.dispatch(&[expression])
    }

    fn windows(&self) -> Result<Vec<Window>, String> {
        let raw = self.exec.run("hyprctl", &["-j".into(), "clients".into()])?;
        let list: Value =
            serde_json::from_str(&raw).map_err(|e| format!("unreadable window list: {e}"))?;
        Ok(list
            .as_array()
            .map(|a| {
                a.iter()
                    .filter(|w| w.get("mapped").and_then(Value::as_bool).unwrap_or(true))
                    .map(|w| Window {
                        address: s(w, "address"),
                        class: s(w, "class"),
                        title: s(w, "title"),
                        workspace: w
                            .pointer("/workspace/name")
                            .and_then(Value::as_str)
                            .unwrap_or("")
                            .to_string(),
                    })
                    .collect()
            })
            .unwrap_or_default())
    }

    /// The one window that `target` (class or title, any case, a part of it
    /// is enough) names. More than one match is an error that lists them, so
    /// the model can ask rather than guess.
    fn pick(&self, target: &str) -> Result<Window, String> {
        let t = target.trim().to_lowercase();
        if t.is_empty() {
            return Err("name the window (its app or title)".into());
        }
        let all = self.windows()?;
        let exact: Vec<&Window> = all.iter().filter(|w| w.class.to_lowercase() == t).collect();
        let hits: Vec<&Window> = if exact.is_empty() {
            all.iter()
                .filter(|w| {
                    w.class.to_lowercase().contains(&t) || w.title.to_lowercase().contains(&t)
                })
                .collect()
        } else {
            exact
        };
        match hits.as_slice() {
            [] => Err(format!("no open window matches \"{target}\"")),
            [one] => {
                if !one.address.starts_with("0x")
                    || !one.address[2..].chars().all(|c| c.is_ascii_hexdigit())
                {
                    return Err("the compositor gave an odd window address".into());
                }
                Ok((*one).clone())
            }
            many => Err(format!(
                "\"{target}\" matches {} windows ({}); say which",
                many.len(),
                many.iter()
                    .map(|w| format!("{}: {}", w.class, w.title))
                    .collect::<Vec<_>>()
                    .join("; ")
            )),
        }
    }

    fn dispatch(&self, args: &[&str]) -> Result<String, String> {
        let mut v: Vec<String> = vec!["dispatch".into()];
        v.extend(args.iter().map(|a| a.to_string()));
        // Hyprland answers "ok"; anything else is it refusing the request
        // (it still exits 0).
        let out = self.exec.run("hyprctl", &v)?;
        match out.trim() {
            "" | "ok" => Ok(out),
            other => Err(format!("Hyprland refused it: {other}")),
        }
    }

    fn run_verb(&self, req: &ActionRequest) -> Result<Value, String> {
        let p = |k: &str| {
            req.parameters
                .get(k)
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|v| !v.is_empty())
        };
        match req.action.as_str() {
            "windows" => Ok(json!(self
                .windows()?
                .iter()
                .map(|w| json!({"app": w.class, "title": w.title, "workspace": w.workspace}))
                .collect::<Vec<_>>())),
            "active_window" => {
                let raw = self
                    .exec
                    .run("hyprctl", &["-j".into(), "activewindow".into()])?;
                let w: Value =
                    serde_json::from_str(&raw).map_err(|e| format!("unreadable window: {e}"))?;
                Ok(json!({"app": s(&w, "class"), "title": s(&w, "title")}))
            }
            "workspaces" => {
                let raw = self
                    .exec
                    .run("hyprctl", &["-j".into(), "workspaces".into()])?;
                let list: Value = serde_json::from_str(&raw)
                    .map_err(|e| format!("unreadable workspaces: {e}"))?;
                Ok(json!(list
                    .as_array()
                    .map(|a| a
                        .iter()
                        .map(|w| json!({"name": s(w, "name"), "windows": w.get("windows").and_then(Value::as_u64).unwrap_or(0)}))
                        .collect::<Vec<_>>())
                    .unwrap_or_default()))
            }
            "focus_window" => {
                let w = self.pick(p("target").ok_or("name the window to focus")?)?;
                self.focus(&w.address)?;
                Ok(json!({"focused": w.class, "title": w.title}))
            }
            "move_window" => {
                let w = self.pick(p("target").ok_or("name the window to move")?)?;
                let ws = workspace_arg(p("workspace").ok_or("name the workspace")?)?;
                self.move_to_workspace(&w.address, &ws)?;
                // Done only when it's there: look again. Only a numbered
                // workspace can be compared (a named, special or relative
                // one is reported differently from how it's asked for).
                if ws.chars().all(|c| c.is_ascii_digit()) {
                    if let Some(now) = self.windows()?.iter().find(|x| x.address == w.address) {
                        if now.workspace != ws {
                            return Err(format!(
                                "asked to move {} to workspace {ws}, but it's still on {}",
                                w.class, now.workspace
                            ));
                        }
                    }
                }
                // Where it was, so it can be put back (friday undo).
                Ok(
                    json!({"moved": w.class, "workspace": ws, "address": w.address, "from": w.workspace}),
                )
            }
            "switch_workspace" => {
                let ws = workspace_arg(p("workspace").ok_or("name the workspace")?)?;
                self.goto_workspace(&ws)?;
                Ok(json!({"workspace": ws}))
            }
            "close_window" => {
                let w = self.pick(p("target").ok_or("name the window to close")?)?;
                self.close(&w.address)?;
                // An app can ask to save first: say it's still open rather
                // than "closed".
                std::thread::sleep(std::time::Duration::from_millis(CLOSE_SETTLE_MS));
                if self.windows()?.iter().any(|x| x.address == w.address) {
                    return Ok(json!({"asked_to_close": w.class, "title": w.title,
                        "still_open": true, "note": "it's still open: it may be asking whether to save"}));
                }
                Ok(json!({"closed": w.class, "title": w.title}))
            }
            "launch_app" => {
                let app = p("app").ok_or("name the app to launch")?;
                self.launch(app)
            }
            "find_app" => {
                let query =
                    p("query").ok_or("say what to look for (an app name or what it does)")?;
                let apps = super::apps::find_apps(&super::apps::app_dirs(), query, 8);
                Ok(
                    json!({"apps": apps.iter().map(|a| json!({"id": a.id, "name": a.name})).collect::<Vec<_>>()}),
                )
            }
            "media" => {
                let cmd = match p("command")
                    .ok_or("say what to do: play, pause, play-pause, next, previous or stop")?
                {
                    c @ ("play" | "pause" | "play-pause" | "next" | "previous" | "stop") => c,
                    other => return Err(format!("unknown media command \"{other}\"")),
                };
                self.exec.run("playerctl", &[cmd.into()])?;
                Ok(json!({"media": cmd}))
            }
            "system_health" => {
                // Looks only. Each part is best effort: a missing tool is a
                // blank, not a failure, so one broken part never hides the rest.
                let cmd = |program: &str, args: &[&str]| {
                    let args: Vec<String> = args.iter().map(|a| a.to_string()).collect();
                    self.exec.run(program, &args).unwrap_or_default()
                };
                let lines = |text: String, n: usize| -> Vec<String> {
                    text.lines()
                        .map(|l| l.chars().take(200).collect::<String>())
                        .filter(|l| !l.trim().is_empty())
                        .take(n)
                        .collect()
                };
                let battery = |file: &str| {
                    cmd("cat", &[&format!("/sys/class/power_supply/BAT0/{file}")])
                        .trim()
                        .to_string()
                };
                Ok(json!({
                    "failed_services": lines(cmd("systemctl", &["--failed", "--no-legend", "--plain"]), 10),
                    "failed_user_services": lines(cmd("systemctl", &["--user", "--failed", "--no-legend", "--plain"]), 10),
                    "errors_this_boot": lines(cmd("journalctl", &["-b", "-p", "3", "--no-pager", "-q", "-n", "8", "-o", "cat"]), 8),
                    "disk_root": lines(cmd("df", &["-hP", "/"]), 2),
                    "memory": lines(cmd("free", &["-m"]), 3),
                    "load": cmd("cat", &["/proc/loadavg"]).trim(),
                    "uptime": cmd("uptime", &["-p"]).trim(),
                    "battery": {"percent": battery("capacity"), "status": battery("status")},
                }))
            }
            "now_playing" => {
                let out = self
                    .exec
                    .run(
                        "playerctl",
                        &[
                            "metadata".into(),
                            "--format".into(),
                            "{{artist}} - {{title}}".into(),
                        ],
                    )
                    .unwrap_or_default();
                let t = out.trim();
                Ok(json!({"now_playing": if t.is_empty() { Value::Null } else { json!(t) }}))
            }
            "volume" => {
                if let Some(m) = p("mute") {
                    let v = match m {
                        "on" | "true" => "1",
                        "off" | "false" => "0",
                        "toggle" => "toggle",
                        other => return Err(format!("mute is on, off or toggle, not \"{other}\"")),
                    };
                    self.exec.run(
                        "wpctl",
                        &["set-mute".into(), "@DEFAULT_AUDIO_SINK@".into(), v.into()],
                    )?;
                    return Ok(json!({"mute": m}));
                }
                let level = percent(&req.parameters, "level")?;
                self.exec.run(
                    "wpctl",
                    &[
                        "set-volume".into(),
                        "@DEFAULT_AUDIO_SINK@".into(),
                        format!("{level}%"),
                    ],
                )?;
                Ok(json!({"volume": level}))
            }
            "brightness" => {
                let level = percent(&req.parameters, "level")?;
                self.exec.run(
                    "brightnessctl",
                    &["set".into(), format!("{}%", level.max(1))],
                )?;
                Ok(json!({"brightness": level.max(1)}))
            }
            "power_profile" => {
                let profile = match p("profile")
                    .ok_or("name the profile: performance, balanced or power-saver")?
                {
                    c @ ("performance" | "balanced" | "power-saver") => c,
                    other => return Err(format!("unknown power profile \"{other}\"")),
                };
                self.exec
                    .run("powerprofilesctl", &["set".into(), profile.into()])?;
                Ok(json!({"power_profile": profile}))
            }
            other => Err(format!("the desktop adapter has no \"{other}\"")),
        }
    }

    /// Launch by desktop-entry id (`firefox`, `org.gnome.Nautilus`) through
    /// `gtk-launch`; failing that, by an executable's bare name.
    fn launch(&self, app: &str) -> Result<Value, String> {
        let bare = |a: &str| {
            !a.is_empty()
                && a.chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-' | '+'))
        };
        if !bare(app) {
            return Err("launch an app by its name (like firefox), not a command line".into());
        }
        if self.exec.run("gtk-launch", &[app.into()]).is_ok() {
            return Ok(json!({"launched": app}));
        }
        self.exec_app(app)?;
        Ok(json!({"launched": app}))
    }
}

/// How long a closed window gets to go before it's looked for again.
#[cfg(not(test))]
const CLOSE_SETTLE_MS: u64 = 400;
#[cfg(test)]
const CLOSE_SETTLE_MS: u64 = 0;

/// A relative workspace in Lua dispatch is `r+1` / `r-1` (as in our bindings).
fn lua_workspace(ws: &str) -> String {
    if ws.starts_with('+') || ws.starts_with('-') {
        format!("r{ws}")
    } else {
        ws.to_string()
    }
}

fn s(v: &Value, key: &str) -> String {
    v.get(key).and_then(Value::as_str).unwrap_or("").to_string()
}

/// A workspace by number (`3`, `-1`, `+1`) or a plain name.
fn workspace_arg(ws: &str) -> Result<String, String> {
    let ok = !ws.is_empty()
        && ws.len() <= 24
        && ws
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '+'));
    if ok {
        Ok(ws.to_string())
    } else {
        Err(format!("\"{ws}\" isn't a workspace number or name"))
    }
}

/// A 0 to 100 percentage from a number or a "40" / "40%" string.
fn percent(params: &Value, key: &str) -> Result<u32, String> {
    let raw = params
        .get(key)
        .ok_or_else(|| format!("give a {key} from 0 to 100"))?;
    let n = raw
        .as_u64()
        .or_else(|| {
            raw.as_str()
                .and_then(|t| t.trim().trim_end_matches('%').parse().ok())
        })
        .ok_or_else(|| format!("{key} is a number from 0 to 100"))?;
    if n > 100 {
        Err(format!("{key} is a number from 0 to 100"))
    } else {
        Ok(n as u32)
    }
}

#[async_trait]
impl ActionAdapter for DesktopAdapter {
    fn tool(&self) -> &str {
        "desktop"
    }

    fn actions(&self) -> Vec<&str> {
        vec![
            "windows",
            "active_window",
            "workspaces",
            "focus_window",
            "move_window",
            "switch_workspace",
            "close_window",
            "launch_app",
            "find_app",
            "media",
            "now_playing",
            "system_health",
            "volume",
            "brightness",
            "power_profile",
        ]
    }

    async fn execute(&self, req: ActionRequest) -> ActionResult {
        let started = std::time::Instant::now();
        let risk = match risk_of(&req.action) {
            Some(r) => r,
            None => {
                return ActionResult::error(format!(
                    "the desktop adapter has no \"{}\"",
                    req.action
                ))
            }
        };
        // The commands block briefly (hyprctl answers in milliseconds).
        let exec = self.exec.clone();
        let adapter = DesktopAdapter::with_exec(exec);
        let result = tokio::task::spawn_blocking(move || adapter.run_verb(&req)).await;
        let result = match result {
            Ok(r) => r,
            Err(e) => Err(format!("the desktop action stopped: {e}")),
        };
        let out = match result {
            Ok(v) => ActionResult::success(v),
            Err(e) => ActionResult::error(e),
        };
        out.with_latency_ms(started.elapsed().as_millis())
            .with_metadata("risk", json!(format!("{risk:?}").to_lowercase()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    const CLIENTS: &str = r#"[
      {"address":"0xaaa1","mapped":true,"class":"firefox","title":"Rust docs - Mozilla Firefox","workspace":{"id":1,"name":"1"}},
      {"address":"0xbbb2","mapped":true,"class":"foot","title":"zsh","workspace":{"id":2,"name":"2"}},
      {"address":"0xccc3","mapped":true,"class":"foot","title":"btop","workspace":{"id":2,"name":"2"}},
      {"address":"0xddd4","mapped":false,"class":"ghost","title":"unmapped","workspace":{"id":3,"name":"3"}}
    ]"#;

    /// Answers the compositor's queries, fails `gtk-launch` for names in
    /// `no_entry`, and records every command.
    struct Stub {
        calls: Mutex<Vec<String>>,
        no_entry: Vec<&'static str>,
        version: &'static str,
        dispatch_reply: &'static str,
        /// Moves and closes that took effect (a stuck window is in `stuck`).
        moved: Mutex<Vec<(String, String)>>,
        closed: Mutex<Vec<String>>,
        stuck: Vec<&'static str>,
    }

    impl Stub {
        /// Hyprland's current Lua dispatch (what an unreadable version means).
        fn new(no_entry: &[&'static str]) -> Arc<Self> {
            Arc::new(Self {
                calls: Mutex::new(Vec::new()),
                no_entry: no_entry.to_vec(),
                version: "",
                dispatch_reply: "ok",
                moved: Mutex::new(Vec::new()),
                closed: Mutex::new(Vec::new()),
                stuck: Vec::new(),
            })
        }
        /// An older Hyprland, which takes the classic dispatch syntax.
        fn classic(no_entry: &[&'static str]) -> Arc<Self> {
            Arc::new(Self {
                calls: Mutex::new(Vec::new()),
                no_entry: no_entry.to_vec(),
                version: "Hyprland 0.50.1 built from branch v0.50.1",
                dispatch_reply: "ok",
                moved: Mutex::new(Vec::new()),
                closed: Mutex::new(Vec::new()),
                stuck: Vec::new(),
            })
        }
        /// A Hyprland that refuses every dispatch.
        fn refusing() -> Arc<Self> {
            Arc::new(Self {
                calls: Mutex::new(Vec::new()),
                no_entry: Vec::new(),
                version: "",
                dispatch_reply: "error: no such window",
                moved: Mutex::new(Vec::new()),
                closed: Mutex::new(Vec::new()),
                stuck: Vec::new(),
            })
        }
        fn calls(&self) -> Vec<String> {
            self.calls.lock().unwrap().clone()
        }
        /// The last command that did something (not a `hyprctl -j` look).
        fn last(&self) -> String {
            self.calls()
                .into_iter()
                .rev()
                .find(|c| !c.starts_with("hyprctl -j "))
                .unwrap_or_default()
        }
        /// One whose dispatches Hyprland accepts but that doesn't move or close.
        fn stuck(stuck: &[&'static str]) -> Arc<Self> {
            Arc::new(Self {
                calls: Mutex::new(Vec::new()),
                no_entry: Vec::new(),
                version: "",
                dispatch_reply: "ok",
                moved: Mutex::new(Vec::new()),
                closed: Mutex::new(Vec::new()),
                stuck: stuck.to_vec(),
            })
        }
    }

    impl Exec for Stub {
        fn run(&self, program: &str, args: &[String]) -> Result<String, String> {
            let line = format!("{program} {}", args.join(" "));
            self.calls.lock().unwrap().push(line.clone());
            if line == "hyprctl version" {
                return Ok(self.version.into());
            }
            if program == "hyprctl" && args[0] == "dispatch" {
                // What the dispatch did, unless the window is stuck.
                let addr = line.split("address:").nth(1).map(|a| {
                    a.chars()
                        .take_while(|c| c.is_ascii_alphanumeric())
                        .collect::<String>()
                });
                if let (Some(addr), "ok") = (addr, self.dispatch_reply) {
                    if !self.stuck.contains(&addr.as_str()) {
                        let ws = line
                            .split("workspace = \"")
                            .nth(1)
                            .map(|r| r.split('"').next().unwrap_or("").to_string())
                            .or_else(|| {
                                line.split("movetoworkspacesilent ")
                                    .nth(1)
                                    .map(|r| r.split(',').next().unwrap_or("").to_string())
                            });
                        if line.contains("close") {
                            self.closed.lock().unwrap().push(addr);
                        } else if let Some(ws) = ws {
                            self.moved.lock().unwrap().push((addr, ws));
                        }
                    }
                }
                return Ok(self.dispatch_reply.into());
            }
            if line == "hyprctl -j clients" {
                let mut list: Vec<Value> = serde_json::from_str(CLIENTS).unwrap();
                list.retain(|w| !self.closed.lock().unwrap().contains(&s(w, "address")));
                for (addr, ws) in self.moved.lock().unwrap().iter() {
                    for w in list.iter_mut().filter(|w| s(w, "address") == *addr) {
                        w["workspace"] = json!({"name": ws});
                    }
                }
                return Ok(Value::Array(list).to_string());
            }
            if line == "hyprctl -j activewindow" {
                return Ok(r#"{"class":"foot","title":"zsh"}"#.into());
            }
            if line == "hyprctl -j workspaces" {
                return Ok(r#"[{"name":"1","windows":1},{"name":"2","windows":2}]"#.into());
            }
            if program == "gtk-launch" && self.no_entry.contains(&args[0].as_str()) {
                return Err("no such entry".into());
            }
            if line == "systemctl --failed --no-legend --plain" {
                return Ok("friday-shell.service loaded failed failed FRIDAY Shell\n".into());
            }
            if line == "df -hP /" {
                return Ok(
                    "Filesystem Size Used Avail Use% Mounted on\n/dev/x 100G 97G 3G 97% /\n".into(),
                );
            }
            if program == "playerctl" && args[0] == "metadata" {
                return Ok("Some Artist - A Song\n".into());
            }
            Ok(String::new())
        }
    }

    async fn run(stub: &Arc<Stub>, action: &str, params: Value) -> ActionResult {
        DesktopAdapter::with_exec(stub.clone())
            .execute(ActionRequest::new("desktop", action, params))
            .await
    }

    #[tokio::test]
    async fn system_health_reports_what_is_wrong_and_changes_nothing() {
        let stub = Stub::new(&[]);
        let r = run(&stub, "system_health", json!({})).await;
        assert!(r.success, "{:?}", r.error);
        let text = r.output.to_string();
        assert!(text.contains("friday-shell.service"), "{text}");
        assert!(text.contains("97%"), "{text}");
        assert!(stub.calls().iter().all(|c| [
            "systemctl",
            "journalctl",
            "df",
            "free",
            "cat",
            "uptime"
        ]
        .iter()
        .any(|p| c.starts_with(p))));
    }

    #[test]
    fn every_verb_has_a_risk_and_only_close_is_destructive() {
        let a = DesktopAdapter::new();
        for verb in a.actions() {
            assert!(risk_of(verb).is_some(), "{verb} has no risk class");
        }
        let destructive: Vec<_> = a
            .actions()
            .into_iter()
            .filter(|v| risk_of(v) == Some(Risk::Destructive))
            .collect();
        assert_eq!(destructive, ["close_window"]);
        assert_eq!(risk_of("windows"), Some(Risk::Read));
        assert_eq!(risk_of("launch_app"), Some(Risk::Reversible));
        assert_eq!(risk_of("rm_rf"), None);
    }

    #[tokio::test]
    async fn lists_mapped_windows_compactly_and_marks_the_risk() {
        let stub = Stub::new(&[]);
        let r = run(&stub, "windows", json!({})).await;
        assert!(r.success, "{:?}", r.error);
        let list = r.output.as_array().unwrap();
        assert_eq!(list.len(), 3, "the unmapped window is not listed");
        assert_eq!(
            list[0],
            json!({"app":"firefox","title":"Rust docs - Mozilla Firefox","workspace":"1"})
        );
        assert_eq!(r.metadata["risk"], json!("read"));
    }

    #[tokio::test]
    async fn focuses_the_window_the_target_names() {
        let stub = Stub::new(&[]);
        let r = run(&stub, "focus_window", json!({"target": "Firefox"})).await;
        assert!(r.success, "{:?}", r.error);
        assert_eq!(
            stub.last(),
            r#"hyprctl dispatch hl.dsp.focus({ window = "address:0xaaa1" })"#
        );
        // A part of the title is enough.
        let r = run(&stub, "focus_window", json!({"target": "btop"})).await;
        assert!(r.success);
        assert_eq!(
            stub.last(),
            r#"hyprctl dispatch hl.dsp.focus({ window = "address:0xccc3" })"#
        );
    }

    #[tokio::test]
    async fn an_ambiguous_or_missing_window_is_an_error_not_a_guess() {
        let stub = Stub::new(&[]);
        let r = run(&stub, "focus_window", json!({"target": "foot"})).await;
        assert!(!r.success);
        let e = r.error.unwrap();
        assert!(
            e.contains("matches 2 windows") && e.contains("zsh") && e.contains("btop"),
            "{e}"
        );
        let r = run(&stub, "close_window", json!({"target": "gimp"})).await;
        assert!(r.error.unwrap().contains("no open window matches"));
        assert!(
            !stub.calls().iter().any(|c| c.contains("dispatch")),
            "nothing was dispatched"
        );
    }

    #[tokio::test]
    async fn moving_closing_and_switching_dispatch_exactly() {
        let stub = Stub::new(&[]);
        assert!(
            run(
                &stub,
                "move_window",
                json!({"target": "firefox", "workspace": "3"})
            )
            .await
            .success
        );
        assert_eq!(
            stub.last(),
            r#"hyprctl dispatch hl.dsp.window.move({ workspace = "3", follow = false, window = "address:0xaaa1" })"#
        );
        assert!(
            run(&stub, "switch_workspace", json!({"workspace": "+1"}))
                .await
                .success
        );
        assert_eq!(
            stub.last(),
            r#"hyprctl dispatch hl.dsp.focus({ workspace = "r+1" })"#
        );
        let r = run(&stub, "close_window", json!({"target": "btop"})).await;
        assert!(r.success);
        assert_eq!(
            stub.last(),
            r#"hyprctl dispatch hl.dsp.window.close({ window = "address:0xccc3" })"#
        );
        assert_eq!(r.metadata["risk"], json!("destructive"));
    }

    #[tokio::test]
    async fn a_move_or_close_is_reported_as_it_turned_out() {
        // Hyprland said ok, but the window didn't move: not "done".
        let stub = Stub::stuck(&["0xaaa1", "0xccc3"]);
        let r = run(
            &stub,
            "move_window",
            json!({"target": "firefox", "workspace": "3"}),
        )
        .await;
        assert!(!r.success);
        assert!(r.error.unwrap().contains("still on 1"));
        // A window that stays open (asking to save) is said to be open.
        let r = run(&stub, "close_window", json!({"target": "btop"})).await;
        assert!(r.success);
        assert_eq!(r.output["still_open"], json!(true));
        // One that went is closed.
        let stub = Stub::new(&[]);
        let r = run(&stub, "close_window", json!({"target": "btop"})).await;
        assert_eq!(r.output["closed"], json!("foot"));
        assert!(r.output.get("still_open").is_none());
    }

    #[tokio::test]
    async fn an_older_hyprland_gets_the_classic_dispatch_syntax() {
        let stub = Stub::classic(&["btop"]);
        assert!(
            run(&stub, "focus_window", json!({"target": "firefox"}))
                .await
                .success
        );
        assert_eq!(stub.last(), "hyprctl dispatch focuswindow address:0xaaa1");
        assert!(
            run(
                &stub,
                "move_window",
                json!({"target": "firefox", "workspace": "3"})
            )
            .await
            .success
        );
        assert_eq!(
            stub.last(),
            "hyprctl dispatch movetoworkspacesilent 3,address:0xaaa1"
        );
        assert!(
            run(&stub, "switch_workspace", json!({"workspace": "+1"}))
                .await
                .success
        );
        assert_eq!(stub.last(), "hyprctl dispatch workspace +1");
        assert!(
            run(&stub, "close_window", json!({"target": "btop"}))
                .await
                .success
        );
        assert_eq!(stub.last(), "hyprctl dispatch closewindow address:0xccc3");
        assert!(
            run(&stub, "launch_app", json!({"app": "btop"}))
                .await
                .success
        );
        assert_eq!(stub.last(), "hyprctl dispatch exec btop");
    }

    #[tokio::test]
    async fn a_refused_dispatch_is_an_error_not_a_success() {
        let stub = Stub::refusing();
        let r = run(&stub, "close_window", json!({"target": "btop"})).await;
        assert!(!r.success);
        assert!(r
            .error
            .unwrap()
            .contains("Hyprland refused it: error: no such window"));
    }

    #[tokio::test]
    async fn workspace_names_cannot_carry_extra_dispatch_arguments() {
        let stub = Stub::new(&[]);
        for bad in ["1;exec rm -rf ~", "2,address:0x1", "a b", "1\")", ""] {
            let r = run(&stub, "switch_workspace", json!({"workspace": bad})).await;
            assert!(!r.success, "{bad:?} was accepted");
        }
        assert!(stub.calls().is_empty());
    }

    #[tokio::test]
    async fn launches_by_entry_then_by_bare_name_and_never_a_command_line() {
        let stub = Stub::new(&["btop"]);
        assert!(
            run(&stub, "launch_app", json!({"app": "firefox"}))
                .await
                .success
        );
        assert_eq!(stub.last(), "gtk-launch firefox");
        assert!(
            run(&stub, "launch_app", json!({"app": "btop"}))
                .await
                .success
        );
        assert_eq!(stub.last(), r#"hyprctl dispatch hl.dsp.exec_cmd("btop")"#);
        for bad in [
            "firefox --private-window",
            "sh -c 'x'",
            "a;b",
            "$(x)",
            "../bin/x",
            "a\")",
            "",
        ] {
            let r = run(&stub, "launch_app", json!({"app": bad})).await;
            assert!(!r.success, "{bad:?} was accepted");
        }
    }

    #[tokio::test]
    async fn media_volume_brightness_and_power_validate_their_input() {
        let stub = Stub::new(&[]);
        assert!(
            run(&stub, "media", json!({"command": "next"}))
                .await
                .success
        );
        assert_eq!(stub.last(), "playerctl next");
        assert!(
            !run(&stub, "media", json!({"command": "open-url"}))
                .await
                .success
        );

        assert!(run(&stub, "volume", json!({"level": 40})).await.success);
        assert_eq!(stub.last(), "wpctl set-volume @DEFAULT_AUDIO_SINK@ 40%");
        assert!(run(&stub, "volume", json!({"level": "25%"})).await.success);
        assert_eq!(stub.last(), "wpctl set-volume @DEFAULT_AUDIO_SINK@ 25%");
        assert!(!run(&stub, "volume", json!({"level": 150})).await.success);
        assert!(
            run(&stub, "volume", json!({"mute": "toggle"}))
                .await
                .success
        );
        assert_eq!(stub.last(), "wpctl set-mute @DEFAULT_AUDIO_SINK@ toggle");

        // Brightness never goes to 0 (a black screen can't be undone by looking).
        assert!(run(&stub, "brightness", json!({"level": 0})).await.success);
        assert_eq!(stub.last(), "brightnessctl set 1%");

        assert!(
            run(&stub, "power_profile", json!({"profile": "power-saver"}))
                .await
                .success
        );
        assert_eq!(stub.last(), "powerprofilesctl set power-saver");
        assert!(
            !run(&stub, "power_profile", json!({"profile": "turbo"}))
                .await
                .success
        );
    }

    #[tokio::test]
    async fn reports_what_is_playing_and_the_active_window() {
        let stub = Stub::new(&[]);
        assert_eq!(
            run(&stub, "now_playing", json!({})).await.output,
            json!({"now_playing": "Some Artist - A Song"})
        );
        assert_eq!(
            run(&stub, "active_window", json!({})).await.output,
            json!({"app":"foot","title":"zsh"})
        );
        assert_eq!(
            run(&stub, "workspaces", json!({})).await.output,
            json!([{"name":"1","windows":1},{"name":"2","windows":2}])
        );
    }

    #[tokio::test]
    async fn an_unknown_verb_is_refused() {
        let stub = Stub::new(&[]);
        let r = run(&stub, "run_shell", json!({"command": "id"})).await;
        assert!(!r.success);
        assert!(stub.calls().is_empty());
    }
}
