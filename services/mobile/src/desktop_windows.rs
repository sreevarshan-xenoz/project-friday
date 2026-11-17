//! The Windows side of the desktop control loop: the same verbs the model
//! uses on FRIDAY OS (`windows`, `focus_window`, `close_window`, `launch_app`,
//! `active_window`), done through FRIDAY's Windows host, the C# service the
//! tray runs. The host does the Win32 work behind its named pipe; this only
//! translates verbs and picks the one window a name means. Built on every
//! platform so it is tested against a stand-in host; it is used on Windows.

use std::sync::Arc;

use async_trait::async_trait;
use friday_actions::{ActionAdapter, ActionRequest, ActionResult};
use serde_json::{json, Value};

/// The verbs this adapter has (a subset of FRIDAY OS's: no moving a window
/// to another desktop and no power profile on Windows).
const VERBS: &[&str] = &[
    "windows",
    "active_window",
    "focus_window",
    "close_window",
    "launch_app",
    "find_app",
    "media",
    "now_playing",
    "volume",
    "brightness",
    "workspaces",
    "switch_workspace",
];

/// Windows that are the shell itself, not something a person asks about.
const SHELL: &[&str] = &["Program Manager", "Windows Input Experience"];

pub struct WindowsDesktop {
    host: Arc<dyn ActionAdapter>,
}

/// A window as the loop shows it: the app (process name without `.exe`),
/// the title and the host's id for it.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Window {
    id: String,
    app: String,
    title: String,
    state: String,
}

impl WindowsDesktop {
    /// Over any host adapter (the pipe in production, a stand-in in tests).
    pub fn new(host: Arc<dyn ActionAdapter>) -> Self {
        Self { host }
    }

    /// Over the running host's pipe for this session (`FRIDAY_SESSION`, else
    /// "default", the same name the host listens on).
    #[cfg(windows)]
    pub fn for_current_session() -> Self {
        let session = std::env::var("FRIDAY_SESSION").unwrap_or_else(|_| "default".into());
        Self::new(Arc::new(friday_windows::WindowsAdapter::for_session(
            &session,
        )))
    }

    async fn call(&self, verb: &str, params: Value) -> Result<Value, String> {
        let res = self
            .host
            .execute(ActionRequest::new("windows", verb, params))
            .await;
        if res.success {
            Ok(res.output)
        } else {
            Err(match res.error.as_deref() {
                Some("HostUnavailable") => {
                    "FRIDAY's Windows helper isn't running (restart FRIDAY from its tray icon)"
                        .to_string()
                }
                Some("WindowNotFound") => "that window is gone".to_string(),
                Some("AppNotFound") => {
                    "no installed app matches that name (try its name as in the Start menu)"
                        .to_string()
                }
                Some(other) => other.strip_prefix("Unknown:").unwrap_or(other).to_string(),
                None => "failed".to_string(),
            })
        }
    }

    async fn windows(&self) -> Result<Vec<Window>, String> {
        let list = self.call("list", json!({})).await?;
        Ok(list
            .as_array()
            .map(|a| {
                a.iter()
                    .map(window_from)
                    .filter(|w| !w.id.is_empty() && !w.title.trim().is_empty())
                    .filter(|w| !SHELL.contains(&w.title.as_str()))
                    .collect()
            })
            .unwrap_or_default())
    }

    /// The one window `target` (an app or part of its title, any case) names.
    /// More than one is an error that lists them, so the model asks rather
    /// than guesses.
    async fn pick(&self, target: &str) -> Result<Window, String> {
        let t = target.trim().to_lowercase();
        if t.is_empty() {
            return Err("name the window (its app or title)".into());
        }
        let all = self.windows().await?;
        let exact: Vec<&Window> = all.iter().filter(|w| w.app.to_lowercase() == t).collect();
        let hits: Vec<&Window> = if exact.is_empty() {
            all.iter()
                .filter(|w| {
                    w.app.to_lowercase().contains(&t) || w.title.to_lowercase().contains(&t)
                })
                .collect()
        } else {
            exact
        };
        match hits.as_slice() {
            [] => Err(format!("no open window matches \"{target}\"")),
            [one] => Ok((*one).clone()),
            many => Err(format!(
                "\"{target}\" matches {} windows ({}); say which",
                many.len(),
                many.iter()
                    .map(|w| format!("{}: {}", w.app, w.title))
                    .collect::<Vec<_>>()
                    .join("; ")
            )),
        }
    }

    async fn run_verb(&self, req: &ActionRequest) -> Result<Value, String> {
        let p = |k: &str| {
            req.parameters
                .get(k)
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|v| !v.is_empty())
        };
        match req.action.as_str() {
            "windows" => Ok(json!(self
                .windows()
                .await?
                .iter()
                .map(|w| json!({"app": w.app, "title": w.title, "state": w.state}))
                .collect::<Vec<_>>())),
            "active_window" => {
                let w = window_from(&self.call("foreground", json!({})).await?);
                Ok(json!({"app": w.app, "title": w.title}))
            }
            "focus_window" => {
                let w = self
                    .pick(p("target").ok_or("name the window to focus")?)
                    .await?;
                self.call("focus", json!({ "window_id": w.id })).await?;
                Ok(json!({"focused": w.app, "title": w.title}))
            }
            "close_window" => {
                let w = self
                    .pick(p("target").ok_or("name the window to close")?)
                    .await?;
                self.call("close", json!({ "window_id": w.id })).await?;
                Ok(json!({"closed": w.app, "title": w.title}))
            }
            "launch_app" => {
                let app = p("app").ok_or("name the app to launch")?;
                // A name, never a path or a command line.
                if !app
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-' | '+' | ' '))
                {
                    return Err(
                        "launch an app by its name (like notepad), not a command line".into(),
                    );
                }
                self.call("open", json!({ "path": app })).await?;
                Ok(json!({"launched": app}))
            }
            "find_app" => {
                let query =
                    p("query").ok_or("say what to look for (an app name or what it does)")?;
                let found = self.call("findapp", json!({ "query": query })).await?;
                Ok(json!({"apps": found["apps"]}))
            }
            "media" => {
                let command = match p("command")
                    .ok_or("say what to do: play, pause, play-pause, next, previous or stop")?
                {
                    c @ ("play" | "pause" | "play-pause" | "next" | "previous" | "stop") => c,
                    other => return Err(format!("unknown media command \"{other}\"")),
                };
                self.call("media", json!({ "command": command })).await?;
                Ok(json!({"media": command}))
            }
            "now_playing" => {
                let now = self.call("nowplaying", json!({})).await?;
                let text = |k: &str| {
                    now.get(k)
                        .and_then(Value::as_str)
                        .map(str::trim)
                        .filter(|t| !t.is_empty())
                };
                Ok(match (text("artist"), text("title")) {
                    (Some(a), Some(t)) => {
                        json!({"now_playing": format!("{a} - {t}"), "status": now["status"]})
                    }
                    (None, Some(t)) => json!({"now_playing": t, "status": now["status"]}),
                    _ => json!({"now_playing": Value::Null}),
                })
            }
            "volume" => {
                if let Some(mute) = p("mute") {
                    let mute = match mute {
                        "on" | "true" => "on",
                        "off" | "false" => "off",
                        "toggle" => "toggle",
                        other => return Err(format!("mute is on, off or toggle, not \"{other}\"")),
                    };
                    let now = self.call("volume", json!({ "mute": mute })).await?;
                    return Ok(
                        json!({"mute": mute, "muted": now["muted"], "volume": now["level"]}),
                    );
                }
                let level = percent(&req.parameters, "level")?;
                let now = self.call("volume", json!({ "level": level })).await?;
                Ok(json!({"volume": now["level"], "muted": now["muted"]}))
            }
            "brightness" => {
                let level = percent(&req.parameters, "level")?;
                if level == 0 {
                    return Err("brightness is a number from 1 to 100".into());
                }
                let now = self.call("brightness", json!({ "level": level })).await?;
                Ok(json!({"brightness": now["level"]}))
            }
            "workspaces" => {
                let now = self.call("desktops", json!({})).await?;
                Ok(json!({"desktops": now["count"], "current": now["current"]}))
            }
            "switch_workspace" => {
                let want = p("workspace")
                    .map(str::to_string)
                    .or_else(|| {
                        req.parameters
                            .get("workspace")
                            .and_then(Value::as_u64)
                            .map(|n| n.to_string())
                    })
                    .ok_or("say which desktop: a number, +1 or -1")?;
                let params = match want.to_lowercase().trim_start_matches("desktop ").trim() {
                    "+1" | "next" => json!({ "by": 1 }),
                    "-1" | "previous" | "prev" => json!({ "by": -1 }),
                    n => match n.parse::<u32>() {
                        Ok(n) if n >= 1 => json!({ "to": n }),
                        _ => return Err("a desktop is a number from 1, or +1 or -1".into()),
                    },
                };
                let now = self.call("desktops", params).await?;
                Ok(json!({"desktops": now["count"], "current": now["current"]}))
            }
            other => Err(format!("unknown desktop action \"{other}\"")),
        }
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

fn window_from(w: &Value) -> Window {
    let s = |k: &str| w.get(k).and_then(Value::as_str).unwrap_or("").to_string();
    let process = s("process");
    Window {
        id: s("id"),
        app: process
            .strip_suffix(".exe")
            .or_else(|| process.strip_suffix(".EXE"))
            .unwrap_or(&process)
            .to_string(),
        title: s("title"),
        state: s("state"),
    }
}

#[async_trait]
impl ActionAdapter for WindowsDesktop {
    fn tool(&self) -> &str {
        "desktop"
    }

    fn actions(&self) -> Vec<&str> {
        VERBS.to_vec()
    }

    async fn execute(&self, req: ActionRequest) -> ActionResult {
        let started = std::time::Instant::now();
        if !VERBS.contains(&req.action.as_str()) {
            return ActionResult::error(format!(
                "the Windows desktop has no \"{}\" action",
                req.action
            ));
        }
        match self.run_verb(&req).await {
            Ok(output) => ActionResult::success(output),
            Err(e) => ActionResult::error(e),
        }
        .with_latency_ms(started.elapsed().as_millis())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// A stand-in for the host: a fixed window list, and a record of what was asked.
    struct Host {
        calls: Mutex<Vec<(String, Value)>>,
        windows: Value,
        down: bool,
    }

    impl Host {
        fn new(windows: Value) -> Arc<Self> {
            Arc::new(Self {
                calls: Mutex::new(vec![]),
                windows,
                down: false,
            })
        }
        fn calls(&self) -> Vec<(String, Value)> {
            self.calls.lock().unwrap().clone()
        }
    }

    #[async_trait]
    impl ActionAdapter for Host {
        fn tool(&self) -> &str {
            "windows"
        }
        fn actions(&self) -> Vec<&str> {
            vec![]
        }
        async fn execute(&self, req: ActionRequest) -> ActionResult {
            self.calls
                .lock()
                .unwrap()
                .push((req.action.clone(), req.parameters.clone()));
            if self.down {
                return ActionResult::error("HostUnavailable");
            }
            match req.action.as_str() {
                "list" => ActionResult::success(self.windows.clone()),
                "foreground" => ActionResult::success(self.windows[0].clone()),
                "nowplaying" => ActionResult::success(
                    json!({"playing": true, "status": "playing", "title": "Blue in Green", "artist": "Miles Davis"}),
                ),
                "volume" => ActionResult::success(json!({"level": 30, "muted": false})),
                "brightness" => ActionResult::success(json!({"level": req.parameters["level"]})),
                "desktops" => ActionResult::success(json!({"count": 3, "current": 2})),
                "findapp" => ActionResult::success(json!({"apps": ["Brave", "Brave Beta"]})),
                _ => ActionResult::success(json!({"ok": true})),
            }
        }
    }

    fn sample() -> Value {
        json!([
            {"id": "w1", "title": "notes.txt - Notepad", "process": "notepad.exe", "state": "Normal"},
            {"id": "w2", "title": "Docs - Mozilla Firefox", "process": "firefox.exe", "state": "Normal"},
            {"id": "w3", "title": "Inbox - Mozilla Firefox", "process": "firefox.exe", "state": "Minimized"},
            {"id": "w4", "title": "Program Manager", "process": "explorer.exe", "state": "Normal"},
            {"id": "w5", "title": "", "process": "svchost.exe", "state": "Hidden"}
        ])
    }

    async fn run(host: &Arc<Host>, action: &str, params: Value) -> ActionResult {
        WindowsDesktop::new(host.clone())
            .execute(ActionRequest::new("desktop", action, params))
            .await
    }

    #[tokio::test]
    async fn lists_real_windows_by_app_without_the_shell_or_untitled_ones() {
        let host = Host::new(sample());
        let r = run(&host, "windows", json!({})).await;
        assert!(r.success);
        let list = r.output.as_array().unwrap();
        assert_eq!(list.len(), 3);
        assert_eq!(list[0]["app"], "notepad");
        assert!(!r.output.to_string().contains("Program Manager"));
    }

    #[tokio::test]
    async fn focuses_the_one_window_the_name_picks() {
        let host = Host::new(sample());
        let r = run(&host, "focus_window", json!({"target": "Notepad"})).await;
        assert!(r.success, "{:?}", r.error);
        assert_eq!(
            host.calls().last().unwrap(),
            &("focus".to_string(), json!({"window_id": "w1"}))
        );
    }

    #[tokio::test]
    async fn an_ambiguous_or_missing_window_is_an_error_not_a_guess() {
        let host = Host::new(sample());
        let r = run(&host, "close_window", json!({"target": "firefox"})).await;
        assert!(!r.success);
        let msg = r.error.unwrap();
        assert!(
            msg.contains("matches 2 windows") && msg.contains("Docs"),
            "{msg}"
        );
        assert!(
            host.calls().iter().all(|(a, _)| a == "list"),
            "nothing was closed"
        );

        let r = run(&host, "close_window", json!({"target": "gimp"})).await;
        assert!(r.error.unwrap().contains("no open window matches"));
        let r = run(&host, "focus_window", json!({})).await;
        assert!(!r.success);
    }

    #[tokio::test]
    async fn closing_sends_the_host_that_windows_id() {
        let host = Host::new(sample());
        let r = run(&host, "close_window", json!({"target": "inbox"})).await;
        assert!(r.success, "{:?}", r.error);
        assert_eq!(
            host.calls().last().unwrap(),
            &("close".to_string(), json!({"window_id": "w3"}))
        );
    }

    #[tokio::test]
    async fn launches_by_name_and_never_a_command_line() {
        let host = Host::new(sample());
        assert!(
            run(&host, "launch_app", json!({"app": "notepad"}))
                .await
                .success
        );
        assert_eq!(
            host.calls().last().unwrap(),
            &("open".to_string(), json!({"path": "notepad"}))
        );
        for bad in [
            "notepad & calc",
            r"C:\Windows\notepad.exe",
            "notepad.exe --evil|x",
            "a\"b",
        ] {
            let before = host.calls().len();
            let r = run(&host, "launch_app", json!({"app": bad})).await;
            assert!(!r.success, "{bad} should be refused");
            assert_eq!(host.calls().len(), before, "{bad} reached the host");
        }
    }

    #[tokio::test]
    async fn a_missing_host_says_how_to_fix_it() {
        let host = Arc::new(Host {
            calls: Mutex::new(vec![]),
            windows: sample(),
            down: true,
        });
        let r = run(&host, "windows", json!({})).await;
        assert!(r.error.unwrap().contains("isn't running"));
    }

    #[tokio::test]
    async fn the_foreground_window_is_reported_as_app_and_title() {
        let host = Host::new(sample());
        let r = run(&host, "active_window", json!({})).await;
        assert_eq!(
            r.output,
            json!({"app": "notepad", "title": "notes.txt - Notepad"})
        );
    }

    #[tokio::test]
    async fn a_verb_windows_does_not_have_is_refused() {
        let host = Host::new(sample());
        for v in ["move_window", "power_profile"] {
            assert!(!run(&host, v, json!({})).await.success, "{v}");
        }
        assert!(host.calls().is_empty());
    }

    #[tokio::test]
    async fn media_commands_are_checked_and_sent_to_the_host() {
        let host = Host::new(sample());
        for c in ["play", "pause", "play-pause", "next", "previous", "stop"] {
            assert!(
                run(&host, "media", json!({"command": c})).await.success,
                "{c}"
            );
            assert_eq!(
                host.calls().last().unwrap(),
                &("media".to_string(), json!({"command": c}))
            );
        }
        let before = host.calls().len();
        for bad in [
            json!({"command": "rewind"}),
            json!({}),
            json!({"command": "play; calc"}),
        ] {
            assert!(!run(&host, "media", bad).await.success);
        }
        assert_eq!(host.calls().len(), before, "a bad command reached the host");
    }

    #[tokio::test]
    async fn volume_takes_a_level_or_a_mute_and_nothing_else() {
        let host = Host::new(sample());
        for (params, sent) in [
            (json!({"level": 40}), json!({"level": 40})),
            (json!({"level": "55%"}), json!({"level": 55})),
            (json!({"mute": "toggle"}), json!({"mute": "toggle"})),
            (json!({"mute": "true"}), json!({"mute": "on"})),
            (json!({"mute": "off"}), json!({"mute": "off"})),
        ] {
            let r = run(&host, "volume", params).await;
            assert!(r.success, "{:?}", r.error);
            assert_eq!(host.calls().last().unwrap(), &("volume".to_string(), sent));
        }
        let before = host.calls().len();
        for bad in [
            json!({"level": 101}),
            json!({"level": "loud"}),
            json!({"mute": "maybe"}),
            json!({}),
        ] {
            assert!(!run(&host, "volume", bad.clone()).await.success, "{bad}");
        }
        assert_eq!(host.calls().len(), before, "a bad volume reached the host");
        let r = run(&host, "volume", json!({"level": 30})).await;
        assert_eq!(r.output, json!({"volume": 30, "muted": false}));
    }

    #[tokio::test]
    async fn brightness_takes_a_level_from_1_to_100() {
        let host = Host::new(sample());
        let r = run(&host, "brightness", json!({"level": "60%"})).await;
        assert_eq!(r.output, json!({"brightness": 60}));
        assert_eq!(
            host.calls().last().unwrap(),
            &("brightness".to_string(), json!({"level": 60}))
        );
        let before = host.calls().len();
        for bad in [json!({"level": 0}), json!({"level": 101}), json!({})] {
            assert!(
                !run(&host, "brightness", bad.clone()).await.success,
                "{bad}"
            );
        }
        assert_eq!(host.calls().len(), before, "a bad level reached the host");
    }

    #[tokio::test]
    async fn desktops_are_listed_and_switched_by_number_or_step() {
        let host = Host::new(sample());
        let r = run(&host, "workspaces", json!({})).await;
        assert_eq!(r.output, json!({"desktops": 3, "current": 2}));
        for (ask, sent) in [
            (json!("3"), json!({"to": 3})),
            (json!(1), json!({"to": 1})),
            (json!("Desktop 2"), json!({"to": 2})),
            (json!("+1"), json!({"by": 1})),
            (json!("-1"), json!({"by": -1})),
            (json!("previous"), json!({"by": -1})),
        ] {
            let r = run(&host, "switch_workspace", json!({"workspace": ask.clone()})).await;
            assert!(r.success, "{ask}: {:?}", r.error);
            assert_eq!(
                host.calls().last().unwrap(),
                &("desktops".to_string(), sent)
            );
        }
        let before = host.calls().len();
        for bad in [json!("0"), json!("work"), json!("1; calc"), json!(-3)] {
            assert!(
                !run(&host, "switch_workspace", json!({"workspace": bad.clone()}))
                    .await
                    .success,
                "{bad}"
            );
        }
        assert!(!run(&host, "switch_workspace", json!({})).await.success);
        assert_eq!(host.calls().len(), before, "a bad desktop reached the host");
    }

    #[tokio::test]
    async fn find_app_asks_the_host_and_returns_the_names() {
        let host = Host::new(sample());
        let r = run(&host, "find_app", json!({"query": "brave browser"})).await;
        assert!(r.success, "{:?}", r.error);
        assert_eq!(r.output, json!({"apps": ["Brave", "Brave Beta"]}));
        assert_eq!(
            host.calls().last().unwrap(),
            &("findapp".to_string(), json!({"query": "brave browser"}))
        );
        assert!(!run(&host, "find_app", json!({})).await.success);
    }

    #[tokio::test]
    async fn now_playing_names_the_artist_and_title_or_nothing() {
        let host = Host::new(sample());
        let r = run(&host, "now_playing", json!({})).await;
        assert_eq!(r.output["now_playing"], "Miles Davis - Blue in Green");
    }

    #[tokio::test]
    async fn a_host_message_comes_through_without_its_internal_prefix() {
        struct Quiet;
        #[async_trait]
        impl ActionAdapter for Quiet {
            fn tool(&self) -> &str {
                "windows"
            }
            fn actions(&self) -> Vec<&str> {
                vec![]
            }
            async fn execute(&self, _: ActionRequest) -> ActionResult {
                ActionResult::error("Unknown:There is no audio output device to control")
            }
        }
        let r = WindowsDesktop::new(Arc::new(Quiet))
            .execute(ActionRequest::new(
                "desktop",
                "volume",
                json!({"level": 10}),
            ))
            .await;
        assert_eq!(
            r.error.unwrap(),
            "There is no audio output device to control"
        );
    }

    #[test]
    fn every_verb_has_a_risk_and_only_close_is_destructive() {
        use friday_actions::{desktop_risk_of, DesktopRisk};
        for v in VERBS {
            assert!(desktop_risk_of(v).is_some(), "{v} has no risk class");
        }
        let destructive: Vec<_> = VERBS
            .iter()
            .filter(|v| desktop_risk_of(v) == Some(DesktopRisk::Destructive))
            .collect();
        assert_eq!(destructive, vec![&"close_window"]);
    }
}
