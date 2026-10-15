//! The Hyprland host: serves the `windows.*` bridge intents on a Linux
//! Hyprland desktop — FRIDAY OS's counterpart of the C# Windows host. The
//! [`crate::WindowsAdapter`] above it is unchanged: it forwards the same
//! intents and decodes the same [`BridgeResponse`]s, so `windows.open` works
//! on FRIDAY OS exactly as it does on Windows.
//!
//! Every OS touch goes through a [`CommandRunner`] (`hyprctl`, `wtype`,
//! `wl-paste`/`wl-copy`, `grim`), so the recipes are proven against a scripted
//! runner and only qualified live on the target.
//!
//! Dispatches use Hyprland's Lua API (`hl.dsp.*`, 0.55+) with the legacy
//! dispatcher as a fallback. A dispatch counts as accepted only when hyprctl
//! exits 0 *and* prints nothing or `ok`: Hyprland reports a refused dispatch
//! (e.g. moving a fullscreen window) as a warning with exit status 0.
//!
//! Window ids are Hyprland client addresses (`0x…`). Every id and string that
//! reaches a Lua expression or shell command is validated or quoted first —
//! request parameters are model-controlled.

use std::collections::HashSet;
use std::sync::Arc;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use base64::Engine;
use friday_context_collector::{CommandOutput, CommandRunner, RealCommandRunner};
use serde_json::{json, Value};

use crate::ipc::IpcTransport;
use crate::model::{Bounds, Window, WindowState};
use crate::protocol::{BridgeFrame, BridgeResponse};

/// App names the planner produces (Windows executable names arrive with the
/// `.exe` stripped) → Linux launch candidates, first installed wins. A name
/// with no alias is launched as-is when it is on `PATH`.
const APP_ALIASES: &[(&[&str], &[&str])] = &[
    (
        &["browser", "chrome", "google", "chromium", "edge", "msedge"],
        &[
            "chromium",
            "google-chrome-stable",
            "firefox",
            "microsoft-edge-stable",
        ],
    ),
    (&["firefox"], &["firefox", "chromium"]),
    (
        &["terminal", "wt", "cmd", "powershell", "console", "shell"],
        &["xdg-terminal-exec", "alacritty", "foot", "kitty", "ghostty"],
    ),
    (
        &["notepad", "editor", "text", "gedit"],
        &["gnome-text-editor", "mousepad", "kate", "gedit"],
    ),
    (
        &["explorer", "files", "file", "nautilus"],
        &["nautilus", "thunar", "dolphin", "nemo"],
    ),
    (
        &["calculator", "calc"],
        &["gnome-calculator", "qalculate-gtk", "kcalc"],
    ),
    (&["code", "vscode"], &["code", "codium"]),
    (&["winword", "word"], &["libreoffice --writer"]),
    (&["excel"], &["libreoffice --calc"]),
];

/// A new window that matches neither the launched binary nor its class is
/// accepted after it has been seen this long (e.g. `gnome-text-editor` maps
/// to class `org.gnome.TextEditor`).
const UNMATCHED_WINDOW_GRACE: Duration = Duration::from_millis(1200);

#[derive(Clone)]
pub struct HyprlandTransport {
    runner: Arc<dyn CommandRunner>,
    on_path: Arc<dyn Fn(&str) -> bool + Send + Sync>,
    launch_timeout: Duration,
    poll_interval: Duration,
}

impl Default for HyprlandTransport {
    fn default() -> Self {
        Self::new()
    }
}

impl HyprlandTransport {
    /// The production transport: real commands, real `PATH`.
    pub fn new() -> Self {
        Self::with_runner(
            Arc::new(RealCommandRunner {
                timeout: Some(Duration::from_secs(10)),
            }),
            Arc::new(binary_on_path),
        )
    }

    pub fn with_runner(
        runner: Arc<dyn CommandRunner>,
        on_path: Arc<dyn Fn(&str) -> bool + Send + Sync>,
    ) -> Self {
        Self {
            runner,
            on_path,
            launch_timeout: Duration::from_secs(8),
            poll_interval: Duration::from_millis(150),
        }
    }

    /// Override how long `open` waits for a window, and how often it looks.
    pub fn with_timing(mut self, launch_timeout: Duration, poll_interval: Duration) -> Self {
        self.launch_timeout = launch_timeout;
        self.poll_interval = poll_interval;
        self
    }

    /// Serve one intent. Failures become error codes the adapter normalizes
    /// through [`crate::error::BridgeError::from_code`].
    pub fn handle(&self, intent: &str, payload: &Value) -> BridgeResponse {
        let verb = intent.strip_prefix("windows.").unwrap_or(intent);
        let result = match verb {
            "open" => self.open(payload),
            "close" => self.close(payload),
            "focus" => self.focus(payload),
            "move" => self.move_window(payload),
            "resize" => self.resize(payload),
            "list" => self.list(),
            "foreground" => self.foreground(),
            "type" => self.type_text(payload),
            "capture" => self.capture(payload),
            "clipboard.read" => self.clipboard_read(),
            "clipboard.write" => self.clipboard_write(payload),
            // Wayland has no pointer-injection tool on FRIDAY OS yet.
            "click" | "scroll" => Err(format!("Unsupported: windows.{verb} on Hyprland")),
            other => Err(format!("Unsupported: windows.{other}")),
        };
        result.unwrap_or_else(|code| BridgeResponse::err(&code))
    }

    fn open(&self, p: &Value) -> Result<BridgeResponse, String> {
        let requested = str_param(p, "path")?;
        let mut argv = self.resolve_app(requested)?;
        let binary = argv[0]
            .rsplit('/')
            .next()
            .unwrap_or(&argv[0])
            .to_lowercase();
        if let Some(args) = p.get("args").and_then(Value::as_str) {
            argv.extend(split_args(args)?);
        }
        let cmdline = argv
            .iter()
            .map(|a| shell_quote(a))
            .collect::<Vec<_>>()
            .join(" ");

        let before = self.clients()?;
        let before_ids: HashSet<&str> = before.iter().map(address_of).collect();
        self.dispatch(
            &format!("hl.dsp.exec_cmd({})", lua_string(&cmdline)?),
            &["exec", &cmdline],
        )?;

        let deadline = Instant::now() + self.launch_timeout;
        let mut unmatched: Option<(Value, Instant)> = None;
        loop {
            let fresh: Vec<Value> = self
                .clients()?
                .into_iter()
                .filter(|c| !before_ids.contains(address_of(c)))
                .collect();
            if let Some(c) = fresh.iter().find(|c| matches_app(c, &binary)) {
                return Ok(opened(c, "New Instance"));
            }
            if let Some(first) = fresh.into_iter().next() {
                let seen = unmatched.get_or_insert((first, Instant::now()));
                if seen.1.elapsed() >= UNMATCHED_WINDOW_GRACE {
                    return Ok(opened(&seen.0, "New Instance"));
                }
            }
            if Instant::now() >= deadline {
                break;
            }
            std::thread::sleep(self.poll_interval);
        }
        if let Some((c, _)) = unmatched {
            return Ok(opened(&c, "New Instance"));
        }
        // Single-instance apps hand the launch to the running instance: that
        // window now represents the request.
        if let Some(c) = before.iter().find(|c| matches_app(c, &binary)) {
            self.focus_address(address_of(c))?;
            return Ok(opened(c, "Existing Instance"));
        }
        Err("Timeout".into())
    }

    fn close(&self, p: &Value) -> Result<BridgeResponse, String> {
        let address = window_address(p)?;
        self.client(&address)?;
        self.dispatch(
            &format!("hl.dsp.window.close({{ window = \"address:{address}\" }})"),
            &["closewindow", &format!("address:{address}")],
        )?;
        Ok(BridgeResponse::ok(json!({ "closed": address })))
    }

    fn focus(&self, p: &Value) -> Result<BridgeResponse, String> {
        let address = window_address(p)?;
        self.client(&address)?;
        self.focus_address(&address)?;
        Ok(BridgeResponse::ok(json!({ "focused": address })))
    }

    fn move_window(&self, p: &Value) -> Result<BridgeResponse, String> {
        let address = window_address(p)?;
        let (x, y) = (i32_param(p, "x")?, i32_param(p, "y")?);
        self.make_floating(&address)?;
        self.dispatch(
            &format!("hl.dsp.window.move({{ window = \"address:{address}\", x = {x}, y = {y} }})"),
            &[
                "movewindowpixel",
                &format!("exact {x} {y},address:{address}"),
            ],
        )?;
        Ok(BridgeResponse::ok(window_json(&self.client(&address)?)))
    }

    fn resize(&self, p: &Value) -> Result<BridgeResponse, String> {
        let address = window_address(p)?;
        let (w, h) = (i32_param(p, "width")?, i32_param(p, "height")?);
        if w <= 0 || h <= 0 {
            return Err("InvalidArgument: width and height must be positive".into());
        }
        self.make_floating(&address)?;
        self.dispatch(
            &format!(
                "hl.dsp.window.resize({{ window = \"address:{address}\", x = {w}, y = {h} }})"
            ),
            &[
                "resizewindowpixel",
                &format!("exact {w} {h},address:{address}"),
            ],
        )?;
        Ok(BridgeResponse::ok(window_json(&self.client(&address)?)))
    }

    fn list(&self) -> Result<BridgeResponse, String> {
        let windows: Vec<Value> = self.clients()?.iter().map(window_json).collect();
        Ok(BridgeResponse::ok(Value::Array(windows)))
    }

    fn foreground(&self) -> Result<BridgeResponse, String> {
        let active = self.query("activewindow")?;
        if address_of(&active).is_empty() {
            return Err("WindowNotFound".into());
        }
        Ok(BridgeResponse::ok(window_json(&active)))
    }

    fn type_text(&self, p: &Value) -> Result<BridgeResponse, String> {
        let address = window_address(p)?;
        let text = str_param(p, "text")?;
        self.client(&address)?;
        self.focus_address(&address)?;
        std::thread::sleep(Duration::from_millis(50));
        let out = self.run("wtype", &["--", text])?;
        if out.status != Some(0) {
            return Err(format!("InputFailed: {}", output_text(&out)));
        }
        Ok(BridgeResponse::ok(json!({ "ok": true })))
    }

    fn capture(&self, p: &Value) -> Result<BridgeResponse, String> {
        let address = window_address(p)?;
        let win = to_window(&self.client(&address)?);
        let Bounds {
            x,
            y,
            width,
            height,
        } = win.bounds;
        if width == 0 || height == 0 {
            return Err("InvalidHandle".into());
        }
        // grim captures the screen region the window occupies.
        let geometry = format!("{x},{y} {width}x{height}");
        let out = self.run("grim", &["-g", &geometry, "-"])?;
        if out.status != Some(0) || out.stdout.is_empty() {
            return Err(format!("CaptureFailed: {}", output_text(&out)));
        }
        let data = base64::engine::general_purpose::STANDARD.encode(&out.stdout);
        Ok(BridgeResponse::ok(
            json!({ "data": data, "width": width, "height": height }),
        ))
    }

    fn clipboard_read(&self) -> Result<BridgeResponse, String> {
        let out = self.run("wl-paste", &["--no-newline"])?;
        if out.status == Some(0) {
            let text = String::from_utf8_lossy(&out.stdout).into_owned();
            return Ok(BridgeResponse::ok(json!({ "text": text })));
        }
        let err = output_text(&out);
        if err.contains("No selection") || err.contains("Nothing is copied") {
            return Ok(BridgeResponse::ok(json!({ "text": "" })));
        }
        Err(format!("ClipboardUnavailable: {err}"))
    }

    fn clipboard_write(&self, p: &Value) -> Result<BridgeResponse, String> {
        let text = str_param(p, "text")?;
        // wl-copy forks a server that serves the selection. Run it with
        // stdout/stderr on /dev/null: an inherited capture pipe would stay
        // open until the selection is replaced, hanging the call.
        let out = self.run(
            "sh",
            &[
                "-c",
                "wl-copy -- \"$1\" >/dev/null 2>&1",
                "friday-clipboard",
                text,
            ],
        )?;
        if out.status != Some(0) {
            return Err(format!("ClipboardUnavailable: {}", output_text(&out)));
        }
        Ok(BridgeResponse::ok(json!({ "ok": true })))
    }

    /// Floating, non-fullscreen: the state in which move/resize apply.
    fn make_floating(&self, address: &str) -> Result<(), String> {
        let client = self.client(address)?;
        if client["fullscreen"].as_i64().unwrap_or(0) != 0 {
            self.dispatch(
                &format!(
                    "hl.dsp.window.fullscreen_state({{ window = \"address:{address}\", internal = 0, client = 0 }})"
                ),
                &[],
            )?;
        }
        if client["floating"].as_bool() != Some(true) {
            self.dispatch(
                &format!("hl.dsp.window.float({{ window = \"address:{address}\", action = \"toggle\" }})"),
                &["togglefloating", &format!("address:{address}")],
            )?;
        }
        Ok(())
    }

    fn focus_address(&self, address: &str) -> Result<(), String> {
        self.dispatch(
            &format!("hl.dsp.focus({{ window = \"address:{address}\" }})"),
            &["focuswindow", &format!("address:{address}")],
        )
    }

    /// Run a Lua dispatch, falling back to the legacy dispatcher syntax.
    fn dispatch(&self, lua: &str, legacy: &[&str]) -> Result<(), String> {
        let out = self.run("hyprctl", &["dispatch", lua])?;
        if accepted(&out) {
            return Ok(());
        }
        if !legacy.is_empty() {
            let mut args = vec!["dispatch"];
            args.extend_from_slice(legacy);
            if accepted(&self.run("hyprctl", &args)?) {
                return Ok(());
            }
        }
        Err(format!("DispatchRefused: {}", output_text(&out)))
    }

    fn query(&self, what: &str) -> Result<Value, String> {
        let out = self.run("hyprctl", &["-j", what])?;
        if out.status != Some(0) {
            tracing::warn!("[hyprland] hyprctl -j {what} failed: {}", output_text(&out));
            return Err("HostUnavailable".into());
        }
        serde_json::from_slice(&out.stdout).map_err(|_| "ProtocolError".to_string())
    }

    /// Mapped client windows.
    fn clients(&self) -> Result<Vec<Value>, String> {
        let clients = self.query("clients")?;
        Ok(clients
            .as_array()
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .filter(|c| c["mapped"].as_bool().unwrap_or(true))
            .collect())
    }

    fn client(&self, address: &str) -> Result<Value, String> {
        self.clients()?
            .into_iter()
            .find(|c| address_of(c) == address)
            .ok_or_else(|| "WindowNotFound".to_string())
    }

    fn resolve_app(&self, requested: &str) -> Result<Vec<String>, String> {
        let requested = requested.trim();
        if requested.starts_with('/') {
            if is_safe_token(requested) && (self.on_path)(requested) {
                return Ok(vec![requested.to_string()]);
            }
            return Err(format!("AppNotFound: {requested}"));
        }
        let lower = requested.to_lowercase();
        let name = lower.strip_suffix(".exe").unwrap_or(&lower);
        if !is_safe_token(name) || name.starts_with('-') || name.contains('/') {
            return Err(format!("AppNotFound: {name}"));
        }
        let aliases = APP_ALIASES
            .iter()
            .find(|(names, _)| names.contains(&name))
            .map(|(_, candidates)| candidates.to_vec())
            .unwrap_or_default();
        aliases
            .into_iter()
            .chain(std::iter::once(name))
            .map(|candidate| {
                candidate
                    .split_whitespace()
                    .map(String::from)
                    .collect::<Vec<_>>()
            })
            .find(|argv| (self.on_path)(&argv[0]))
            .ok_or_else(|| format!("AppNotFound: {name}"))
    }

    fn run(&self, program: &str, args: &[&str]) -> Result<CommandOutput, String> {
        let args: Vec<String> = args.iter().map(|a| a.to_string()).collect();
        self.runner.run(program, &args).map_err(|e| {
            tracing::warn!("[hyprland] {program} could not run: {e}");
            "HostUnavailable".to_string()
        })
    }
}

#[async_trait]
impl IpcTransport for HyprlandTransport {
    async fn call(&self, frame: BridgeFrame) -> Result<BridgeFrame, String> {
        let this = self.clone();
        let intent = frame.message.action.clone();
        let payload = frame.message.payload.clone();
        // Commands block (and `open` polls), so keep them off the async runtime.
        let response = tokio::task::spawn_blocking(move || this.handle(&intent, &payload))
            .await
            .map_err(|e| e.to_string())?;
        Ok(frame.response_to(response))
    }
}

fn opened(client: &Value, resolution: &str) -> BridgeResponse {
    let mut response = BridgeResponse::ok(window_json(client));
    response.meta = Some([("launch_resolution".to_string(), resolution.to_string())].into());
    response
}

fn to_window(c: &Value) -> Window {
    let num = |key: &str, i: usize| c[key][i].as_i64().unwrap_or(0);
    let state = if c["hidden"].as_bool() == Some(true) {
        WindowState::Hidden
    } else if c["fullscreen"].as_i64().unwrap_or(0) != 0 {
        WindowState::Maximized
    } else {
        WindowState::Normal
    };
    Window {
        id: address_of(c).to_string(),
        title: c["title"].as_str().unwrap_or_default().to_string(),
        process: c["class"].as_str().unwrap_or_default().to_string(),
        process_id: c["pid"].as_i64().filter(|p| *p > 0).unwrap_or(0) as u32,
        bounds: Bounds {
            x: num("at", 0) as i32,
            y: num("at", 1) as i32,
            width: num("size", 0).max(0) as u32,
            height: num("size", 1).max(0) as u32,
        },
        state,
        monitor: c["monitor"].as_i64().filter(|m| *m >= 0).unwrap_or(0) as u32,
    }
}

fn window_json(c: &Value) -> Value {
    serde_json::to_value(to_window(c)).unwrap_or(Value::Null)
}

fn address_of(c: &Value) -> &str {
    c["address"].as_str().unwrap_or_default()
}

/// Whether a client window belongs to the launched binary (`chromium` →
/// class `chromium`, `alacritty` → `Alacritty`).
fn matches_app(c: &Value, binary: &str) -> bool {
    ["class", "initialClass"].iter().any(|key| {
        let class = c[*key].as_str().unwrap_or_default().to_lowercase();
        !class.is_empty() && (class.contains(binary) || binary.contains(&class))
    })
}

fn accepted(out: &CommandOutput) -> bool {
    out.status == Some(0) && matches!(output_text(out).as_str(), "" | "ok")
}

fn output_text(out: &CommandOutput) -> String {
    let mut text = String::from_utf8_lossy(&out.stdout).into_owned();
    text.push_str(&String::from_utf8_lossy(&out.stderr));
    text.trim().to_string()
}

fn window_address(p: &Value) -> Result<String, String> {
    let id = p
        .get("window_id")
        .and_then(Value::as_str)
        .ok_or("InvalidHandle")?;
    let valid = id
        .strip_prefix("0x")
        .is_some_and(|hex| !hex.is_empty() && hex.chars().all(|c| c.is_ascii_hexdigit()));
    if valid {
        Ok(id.to_string())
    } else {
        Err("InvalidHandle".into())
    }
}

fn str_param<'a>(p: &'a Value, key: &str) -> Result<&'a str, String> {
    p.get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| format!("InvalidArgument: missing {key}"))
}

fn i32_param(p: &Value, key: &str) -> Result<i32, String> {
    p.get(key)
        .and_then(Value::as_i64)
        .and_then(|v| i32::try_from(v).ok())
        .ok_or_else(|| format!("InvalidArgument: missing {key}"))
}

fn is_safe_token(s: &str) -> bool {
    !s.is_empty()
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '+' | '-' | '/'))
}

fn split_args(args: &str) -> Result<Vec<String>, String> {
    if args.chars().any(char::is_control) {
        return Err("InvalidArgument: control characters in args".into());
    }
    Ok(args.split_whitespace().map(String::from).collect())
}

/// POSIX single-quote a word for the shell Hyprland runs `exec` through.
fn shell_quote(word: &str) -> String {
    let plain = word.chars().all(|c| {
        c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '+' | '-' | '/' | ':' | '=' | ',')
    });
    if plain && !word.is_empty() {
        word.to_string()
    } else {
        format!("'{}'", word.replace('\'', r"'\''"))
    }
}

/// A Lua string literal. Control characters are refused rather than escaped.
fn lua_string(s: &str) -> Result<String, String> {
    if s.chars().any(char::is_control) {
        return Err("InvalidArgument: control characters in command".into());
    }
    Ok(format!(
        "\"{}\"",
        s.replace('\\', r"\\").replace('"', "\\\"")
    ))
}

/// Whether `binary` is an executable file, by path or on `PATH`.
pub fn binary_on_path(binary: &str) -> bool {
    let is_exec = |path: &std::path::Path| {
        path.metadata().is_ok_and(|m| {
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                m.is_file() && m.permissions().mode() & 0o111 != 0
            }
            #[cfg(not(unix))]
            {
                m.is_file()
            }
        })
    };
    if binary.contains('/') {
        return is_exec(std::path::Path::new(binary));
    }
    std::env::var_os("PATH")
        .is_some_and(|paths| std::env::split_paths(&paths).any(|dir| is_exec(&dir.join(binary))))
}

#[cfg(test)]
mod tests {
    use super::*;
    use friday_context_collector::RunnerError;
    use std::collections::{HashMap, VecDeque};
    use std::sync::Mutex;

    /// Scripted runner: each argv answers from a queue whose last entry
    /// repeats (so `hyprctl -j clients` can change between polls). Records
    /// every call; unscripted calls fail loudly.
    #[derive(Default)]
    struct ScriptedRunner {
        script: Mutex<HashMap<Vec<String>, VecDeque<CommandOutput>>>,
        calls: Mutex<Vec<Vec<String>>>,
    }

    impl ScriptedRunner {
        fn on(self, argv: &[&str], outputs: Vec<CommandOutput>) -> Self {
            let key = argv.iter().map(|s| s.to_string()).collect();
            self.script.lock().unwrap().insert(key, outputs.into());
            self
        }

        fn calls(&self) -> Vec<Vec<String>> {
            self.calls.lock().unwrap().clone()
        }

        fn called(&self, argv: &[&str]) -> bool {
            self.calls().iter().any(|c| c == argv)
        }
    }

    impl CommandRunner for ScriptedRunner {
        fn run(&self, program: &str, args: &[String]) -> Result<CommandOutput, RunnerError> {
            let mut argv = vec![program.to_string()];
            argv.extend(args.iter().cloned());
            self.calls.lock().unwrap().push(argv.clone());
            let mut script = self.script.lock().unwrap();
            let queue = script
                .get_mut(&argv)
                .ok_or_else(|| RunnerError::Io(format!("unscripted: {argv:?}")))?;
            let out = if queue.len() > 1 {
                queue.pop_front().unwrap()
            } else {
                queue.front().cloned().unwrap()
            };
            Ok(out)
        }
    }

    fn ok(text: &str) -> CommandOutput {
        CommandOutput::ok_text(text)
    }

    fn client(address: &str, class: &str) -> Value {
        json!({
            "address": address, "mapped": true, "hidden": false, "class": class,
            "initialClass": class, "title": format!("{class} window"), "pid": 4242,
            "at": [10, 20], "size": [800, 600], "fullscreen": 0, "floating": false, "monitor": 0
        })
    }

    fn clients(list: &[Value]) -> CommandOutput {
        ok(&Value::Array(list.to_vec()).to_string())
    }

    fn transport(
        runner: Arc<ScriptedRunner>,
        installed: &'static [&'static str],
    ) -> HyprlandTransport {
        HyprlandTransport::with_runner(runner, Arc::new(move |b: &str| installed.contains(&b)))
            .with_timing(Duration::from_millis(300), Duration::from_millis(1))
    }

    const LAUNCH_CHROMIUM: &str = "hl.dsp.exec_cmd(\"chromium\")";

    #[test]
    fn open_launches_the_installed_alias_and_returns_its_new_window() {
        let runner = Arc::new(
            ScriptedRunner::default()
                .on(
                    &["hyprctl", "-j", "clients"],
                    vec![
                        clients(&[client("0xa", "kitty")]),
                        clients(&[client("0xa", "kitty")]),
                        clients(&[client("0xa", "kitty"), client("0xb", "chromium")]),
                    ],
                )
                .on(&["hyprctl", "dispatch", LAUNCH_CHROMIUM], vec![ok("ok")]),
        );
        // `chrome.exe` is what the Windows-oriented planner produces.
        let res = transport(runner.clone(), &["chromium"])
            .handle("windows.open", &json!({ "path": "chrome.exe" }));
        assert!(res.ok, "{:?}", res.error);
        assert_eq!(res.output["id"], "0xb");
        assert_eq!(res.output["process"], "chromium");
        assert_eq!(res.output["bounds"]["width"], 800);
        assert_eq!(res.meta.unwrap()["launch_resolution"], "New Instance");
        assert!(runner.called(&["hyprctl", "dispatch", LAUNCH_CHROMIUM]));
    }

    #[test]
    fn open_falls_back_to_the_legacy_exec_dispatcher() {
        let runner = Arc::new(
            ScriptedRunner::default()
                .on(
                    &["hyprctl", "-j", "clients"],
                    vec![clients(&[]), clients(&[client("0xb", "chromium")])],
                )
                .on(
                    &["hyprctl", "dispatch", LAUNCH_CHROMIUM],
                    vec![CommandOutput::failed("Invalid dispatcher", 1)],
                )
                .on(&["hyprctl", "dispatch", "exec", "chromium"], vec![ok("ok")]),
        );
        let res = transport(runner.clone(), &["chromium"])
            .handle("windows.open", &json!({ "path": "browser" }));
        assert!(res.ok, "{:?}", res.error);
        assert!(runner.called(&["hyprctl", "dispatch", "exec", "chromium"]));
    }

    #[test]
    fn open_accepts_an_unmatched_class_and_reuses_an_existing_instance() {
        // gnome-text-editor's class does not contain its binary name.
        let runner = Arc::new(
            ScriptedRunner::default()
                .on(
                    &["hyprctl", "-j", "clients"],
                    vec![
                        clients(&[]),
                        clients(&[client("0xc", "org.gnome.TextEditor")]),
                    ],
                )
                .on(
                    &[
                        "hyprctl",
                        "dispatch",
                        "hl.dsp.exec_cmd(\"gnome-text-editor\")",
                    ],
                    vec![ok("")],
                ),
        );
        let res = transport(runner, &["gnome-text-editor"])
            .handle("windows.open", &json!({ "path": "notepad" }));
        assert!(res.ok, "{:?}", res.error);
        assert_eq!(res.output["id"], "0xc");

        // No new window, but the app was already running: focus it.
        let runner = Arc::new(
            ScriptedRunner::default()
                .on(
                    &["hyprctl", "-j", "clients"],
                    vec![clients(&[client("0xd", "chromium")])],
                )
                .on(&["hyprctl", "dispatch", LAUNCH_CHROMIUM], vec![ok("ok")])
                .on(
                    &[
                        "hyprctl",
                        "dispatch",
                        "hl.dsp.focus({ window = \"address:0xd\" })",
                    ],
                    vec![ok("ok")],
                ),
        );
        let res =
            transport(runner, &["chromium"]).handle("windows.open", &json!({ "path": "chromium" }));
        assert!(res.ok, "{:?}", res.error);
        assert_eq!(res.meta.unwrap()["launch_resolution"], "Existing Instance");
    }

    #[test]
    fn open_reports_missing_apps_and_timeouts_honestly() {
        let runner = Arc::new(ScriptedRunner::default());
        let res =
            transport(runner.clone(), &[]).handle("windows.open", &json!({ "path": "spotify" }));
        assert_eq!(res.error.as_deref(), Some("AppNotFound: spotify"));
        assert!(
            runner.calls().is_empty(),
            "nothing may launch for an unknown app"
        );

        let runner = Arc::new(
            ScriptedRunner::default()
                .on(&["hyprctl", "-j", "clients"], vec![clients(&[])])
                .on(&["hyprctl", "dispatch", LAUNCH_CHROMIUM], vec![ok("ok")]),
        );
        let res =
            transport(runner, &["chromium"]).handle("windows.open", &json!({ "path": "chromium" }));
        assert_eq!(res.error.as_deref(), Some("Timeout"));
    }

    #[test]
    fn open_quotes_args_and_refuses_shell_or_lua_injection() {
        let runner = Arc::new(
            ScriptedRunner::default()
                .on(
                    &["hyprctl", "-j", "clients"],
                    vec![clients(&[]), clients(&[client("0xb", "chromium")])],
                )
                .on(
                    &[
                        "hyprctl",
                        "dispatch",
                        r#"hl.dsp.exec_cmd("chromium 'https://x.y/?q=a;b' '\"quoted\"'")"#,
                    ],
                    vec![ok("ok")],
                ),
        );
        let res = transport(runner, &["chromium"]).handle(
            "windows.open",
            &json!({ "path": "chromium", "args": "https://x.y/?q=a;b \"quoted\"" }),
        );
        assert!(res.ok, "{:?}", res.error);

        let runner = Arc::new(ScriptedRunner::default());
        for path in [
            "chromium; rm -rf ~",
            "$(reboot)",
            "-rf",
            "../../bin/sh",
            "bin/sh x",
        ] {
            let res = transport(runner.clone(), &["chromium"])
                .handle("windows.open", &json!({ "path": path }));
            assert!(
                res.error.unwrap().starts_with("AppNotFound"),
                "path {path:?} must be refused"
            );
        }
        assert!(runner.calls().is_empty());
    }

    #[test]
    fn window_ids_must_be_hyprland_addresses() {
        let runner = Arc::new(ScriptedRunner::default());
        let t = transport(runner.clone(), &[]);
        for id in [
            "",
            "0x",
            "notepad",
            "0xabc\" }) hl.exec_cmd(\"reboot",
            "address:0xabc",
        ] {
            let res = t.handle("windows.close", &json!({ "window_id": id }));
            assert_eq!(res.error.as_deref(), Some("InvalidHandle"), "id {id:?}");
        }
        assert!(runner.calls().is_empty());
    }

    #[test]
    fn close_and_focus_target_the_address_and_detect_missing_windows() {
        let runner = Arc::new(
            ScriptedRunner::default()
                .on(
                    &["hyprctl", "-j", "clients"],
                    vec![clients(&[client("0xa", "kitty")])],
                )
                .on(
                    &[
                        "hyprctl",
                        "dispatch",
                        "hl.dsp.window.close({ window = \"address:0xa\" })",
                    ],
                    vec![ok("ok")],
                ),
        );
        let t = transport(runner, &[]);
        let res = t.handle("windows.close", &json!({ "window_id": "0xa" }));
        assert!(res.ok, "{:?}", res.error);
        assert_eq!(res.output["closed"], "0xa");
        let res = t.handle("windows.focus", &json!({ "window_id": "0xff" }));
        assert_eq!(res.error.as_deref(), Some("WindowNotFound"));
    }

    #[test]
    fn a_dispatch_warning_with_exit_zero_is_a_refusal() {
        let mut fullscreen = client("0xa", "kitty");
        fullscreen["floating"] = json!(true);
        let runner = Arc::new(
            ScriptedRunner::default()
                .on(&["hyprctl", "-j", "clients"], vec![clients(&[fullscreen])])
                .on(
                    &[
                        "hyprctl",
                        "dispatch",
                        "hl.dsp.window.move({ window = \"address:0xa\", x = 5, y = 6 })",
                    ],
                    vec![ok("warning: =[C]:-1: Window is fullscreen")],
                )
                .on(
                    &[
                        "hyprctl",
                        "dispatch",
                        "movewindowpixel",
                        "exact 5 6,address:0xa",
                    ],
                    vec![CommandOutput::failed("error: invalid dispatcher", 7)],
                ),
        );
        let res = transport(runner, &[]).handle(
            "windows.move",
            &json!({ "window_id": "0xa", "x": 5, "y": 6 }),
        );
        assert!(res.error.unwrap().starts_with("DispatchRefused"));
    }

    #[test]
    fn move_unfullscreens_and_floats_a_tiled_window_first() {
        let mut tiled = client("0xa", "kitty");
        tiled["fullscreen"] = json!(1);
        let mut moved = client("0xa", "kitty");
        moved["at"] = json!([5, 6]);
        moved["floating"] = json!(true);
        let runner = Arc::new(
            ScriptedRunner::default()
                .on(&["hyprctl", "-j", "clients"], vec![clients(&[tiled]), clients(&[moved])])
                .on(
                    &[
                        "hyprctl",
                        "dispatch",
                        "hl.dsp.window.fullscreen_state({ window = \"address:0xa\", internal = 0, client = 0 })",
                    ],
                    vec![ok("ok")],
                )
                .on(
                    &["hyprctl", "dispatch", "hl.dsp.window.float({ window = \"address:0xa\", action = \"toggle\" })"],
                    vec![ok("ok")],
                )
                .on(
                    &["hyprctl", "dispatch", "hl.dsp.window.move({ window = \"address:0xa\", x = 5, y = 6 })"],
                    vec![ok("ok")],
                ),
        );
        let res = transport(runner, &[]).handle(
            "windows.move",
            &json!({ "window_id": "0xa", "x": 5, "y": 6 }),
        );
        assert!(res.ok, "{:?}", res.error);
        assert_eq!(res.output["bounds"]["x"], 5);
    }

    #[test]
    fn list_and_foreground_map_hyprland_clients_to_windows() {
        let mut hidden = client("0xb", "Alacritty");
        hidden["hidden"] = json!(true);
        let mut unmapped = client("0xc", "ghost");
        unmapped["mapped"] = json!(false);
        let runner = Arc::new(
            ScriptedRunner::default()
                .on(
                    &["hyprctl", "-j", "clients"],
                    vec![clients(&[client("0xa", "kitty"), hidden, unmapped])],
                )
                .on(&["hyprctl", "-j", "activewindow"], vec![ok("{}")]),
        );
        let t = transport(runner, &[]);
        let res = t.handle("windows.list", &json!({}));
        let list = res.output.as_array().unwrap();
        assert_eq!(list.len(), 2, "unmapped clients are not windows");
        assert_eq!(list[0]["process_id"], 4242);
        assert_eq!(list[1]["state"], "Hidden");
        let res = t.handle("windows.foreground", &json!({}));
        assert_eq!(res.error.as_deref(), Some("WindowNotFound"));
    }

    #[test]
    fn type_focuses_then_types_text_verbatim() {
        let runner = Arc::new(
            ScriptedRunner::default()
                .on(
                    &["hyprctl", "-j", "clients"],
                    vec![clients(&[client("0xa", "kitty")])],
                )
                .on(
                    &[
                        "hyprctl",
                        "dispatch",
                        "hl.dsp.focus({ window = \"address:0xa\" })",
                    ],
                    vec![ok("ok")],
                )
                .on(&["wtype", "--", "-rf --help"], vec![ok("")]),
        );
        let res = transport(runner.clone(), &[]).handle(
            "windows.type",
            &json!({ "window_id": "0xa", "text": "-rf --help" }),
        );
        assert!(res.ok, "{:?}", res.error);
        let calls = runner.calls();
        let focus = calls.iter().position(|c| c[1] == "dispatch").unwrap();
        let typed = calls.iter().position(|c| c[0] == "wtype").unwrap();
        assert!(focus < typed, "the window must be focused before typing");
    }

    #[test]
    fn capture_grabs_the_window_region_as_base64_png() {
        let runner = Arc::new(
            ScriptedRunner::default()
                .on(
                    &["hyprctl", "-j", "clients"],
                    vec![clients(&[client("0xa", "kitty")])],
                )
                .on(
                    &["grim", "-g", "10,20 800x600", "-"],
                    vec![CommandOutput::ok_binary(b"\x89PNG".to_vec())],
                ),
        );
        let res = transport(runner, &[]).handle("windows.capture", &json!({ "window_id": "0xa" }));
        assert!(res.ok, "{:?}", res.error);
        assert_eq!(res.output["data"], "iVBORw==");
        assert_eq!(res.output["width"], 800);
    }

    #[test]
    fn clipboard_round_trip_and_empty_selection() {
        let runner = Arc::new(
            ScriptedRunner::default()
                .on(
                    &["wl-paste", "--no-newline"],
                    vec![CommandOutput::failed("Nothing is copied", 1)],
                )
                .on(
                    &[
                        "sh",
                        "-c",
                        "wl-copy -- \"$1\" >/dev/null 2>&1",
                        "friday-clipboard",
                        "a 'b' $c",
                    ],
                    vec![ok("")],
                ),
        );
        let t = transport(runner, &[]);
        assert_eq!(
            t.handle("windows.clipboard.read", &json!({})).output["text"],
            ""
        );
        let res = t.handle("windows.clipboard.write", &json!({ "text": "a 'b' $c" }));
        assert!(res.ok, "{:?}", res.error);
    }

    #[test]
    fn no_hyprland_session_is_host_unavailable() {
        let runner = Arc::new(ScriptedRunner::default().on(
            &["hyprctl", "-j", "clients"],
            vec![CommandOutput::failed(
                "HYPRLAND_INSTANCE_SIGNATURE not set!",
                1,
            )],
        ));
        let res = transport(runner, &[]).handle("windows.list", &json!({}));
        assert_eq!(res.error.as_deref(), Some("HostUnavailable"));
    }

    #[test]
    fn pointer_verbs_are_unsupported_not_faked() {
        let t = transport(Arc::new(ScriptedRunner::default()), &[]);
        for verb in ["windows.click", "windows.scroll"] {
            let res = t.handle(
                verb,
                &json!({ "window_id": "0xa", "x": 1, "y": 1, "delta": 1 }),
            );
            assert!(res.error.unwrap().starts_with("Unsupported"));
        }
    }

    #[tokio::test]
    async fn the_adapter_drives_the_transport_end_to_end() {
        use crate::WindowsAdapter;
        use friday_actions::adapters::ActionAdapter;
        use friday_actions::request::ActionRequest;

        let runner = Arc::new(
            ScriptedRunner::default()
                .on(
                    &["hyprctl", "-j", "clients"],
                    vec![clients(&[]), clients(&[client("0xb", "chromium")])],
                )
                .on(&["hyprctl", "dispatch", LAUNCH_CHROMIUM], vec![ok("ok")]),
        );
        let adapter = WindowsAdapter::new(Arc::new(transport(runner, &["chromium"])));
        let res = adapter
            .execute(ActionRequest::new(
                "windows",
                "open",
                json!({ "path": "chrome.exe" }),
            ))
            .await;
        assert!(res.success, "{:?}", res.error);
        assert_eq!(res.output["id"], "0xb");
        assert_eq!(res.metadata["launch_resolution"], "New Instance");
    }
}
