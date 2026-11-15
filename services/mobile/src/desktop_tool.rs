//! The desktop control loop: FRIDAY OS's native control of apps and windows.
//!
//! For a goal like "put Firefox on workspace 2 and pause the music", the model
//! picks one typed desktop action at a time (the verbs of
//! [`friday_actions::DesktopAdapter`]), FRIDAY runs it, the result goes back
//! to the model, and it repeats until the model says it is done or the step
//! limit is reached. The model never runs a command line: it can only choose a
//! verb and its parameters, which the adapter validates.
//!
//! Consent follows the verb's risk class. Looking and reversible changes run,
//! because the person asked for them. A destructive one (closing a window)
//! stops the loop and asks first, through the same confirmation flow as a
//! command or a file write; once confirmed, only that exact action runs.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use friday_actions::{ActionAdapter, ActionRequest, DesktopAdapter, DesktopRisk};
use friday_goal_execution::{Action, Capability, ExecutionContext, Tool, ToolResult};
use friday_investigation::LlmStrategyClient;
use serde_json::{json, Value};

use crate::goal_runner::ConfirmationGate;

/// Most actions one goal may take.
pub const MAX_STEPS: usize = 8;
const MODEL_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(45);
const RESULT_CHARS: usize = 1500;

/// The capability the planner routes desktop goals to.
pub const CAPABILITY: &str = "desktop.control";

/// Whether this machine has a desktop FRIDAY can drive: a Hyprland session,
/// and the owner hasn't turned it off (`FRIDAY_DESKTOP=0`).
pub fn enabled() -> bool {
    cfg!(target_os = "linux")
        && std::env::var("FRIDAY_DESKTOP")
            .map(|v| v != "0")
            .unwrap_or(true)
        && friday_actions::hypr_signature().is_some()
}

/// What the loop needs from the inside-of-apps adapter (AT-SPI, Linux only):
/// run a verb, and resolve a click to the element it would press, without
/// pressing it, so the consent decision can use the element's own label.
#[async_trait]
pub(crate) trait UiControl: Send + Sync {
    async fn run(&self, verb: &str, params: Value) -> friday_actions::ActionResult;
    /// The `(path, label)` a click with these parameters would press.
    async fn resolve_click(&self, params: &Value) -> Result<(String, String), String>;
    /// Whether pressing an element with this label could lose or send something.
    fn is_dangerous(&self, label: &str) -> bool;
}

#[cfg(target_os = "linux")]
#[async_trait]
impl UiControl for friday_actions::UiAdapter {
    async fn run(&self, verb: &str, params: Value) -> friday_actions::ActionResult {
        self.execute(ActionRequest::new("ui", verb, params)).await
    }
    async fn resolve_click(&self, params: &Value) -> Result<(String, String), String> {
        let p = |k: &str| params.get(k).and_then(Value::as_str);
        let app = p("app").ok_or("name the app to click in")?;
        let t = self.resolve_click_target(app, p("path"), p("name")).await?;
        Ok((t.path, t.label))
    }
    fn is_dangerous(&self, label: &str) -> bool {
        friday_actions::label_is_dangerous(label)
    }
}

/// The engine that reads and presses things inside apps. `FRIDAY_UI_ENGINE`
/// picks it (see `agent_desktop::choose_engine`): by default the installed
/// agent-desktop, else FRIDAY's own AT-SPI adapter on Linux, else nothing.
pub(crate) fn default_ui() -> Option<Arc<dyn UiControl>> {
    use crate::agent_desktop::{choose_engine, detect_blocking, Engine};
    let setting = std::env::var("FRIDAY_UI_ENGINE").ok();
    let found = detect_blocking();
    let engine = choose_engine(
        setting.as_deref(),
        found.is_some(),
        cfg!(target_os = "linux"),
    );
    tracing::info!("inside-app control engine: {engine:?}");
    match engine {
        Engine::AgentDesktop => found.map(|u| Arc::new(u) as Arc<dyn UiControl>),
        #[cfg(target_os = "linux")]
        Engine::Atspi => Some(Arc::new(friday_actions::UiAdapter::new())),
        _ => None,
    }
}

pub struct DesktopControlTool {
    model: Arc<dyn LlmStrategyClient>,
    adapter: DesktopAdapter,
    gate: Arc<Mutex<ConfirmationGate>>,
    /// Set by `task.cancel` (the Stop button, `friday cancel`, the kill key).
    abort: Arc<AtomicBool>,
    /// Where every action taken is recorded (`actions.jsonl`), if anywhere.
    audit_dir: Option<PathBuf>,
    /// Looking inside apps and pressing their controls (`ui_*` actions).
    ui: Option<Arc<dyn UiControl>>,
}

/// The directory of `actions.jsonl`: beside state.json (`FRIDAY_STATE_FILE`,
/// else `$XDG_STATE_HOME/friday`).
pub fn audit_dir() -> Option<PathBuf> {
    crate::shell_state::ShellState::default_path()
        .parent()
        .map(|d| d.to_path_buf())
}

impl DesktopControlTool {
    pub(crate) fn new(
        model: Arc<dyn LlmStrategyClient>,
        adapter: DesktopAdapter,
        gate: Arc<Mutex<ConfirmationGate>>,
        abort: Arc<AtomicBool>,
        audit_dir: Option<PathBuf>,
        ui: Option<Arc<dyn UiControl>>,
    ) -> Self {
        Self {
            model,
            adapter,
            gate,
            abort,
            audit_dir,
            ui,
        }
    }

    /// Resolves when the goal is cancelled.
    async fn cancelled(&self) {
        while !self.abort.load(Ordering::SeqCst) {
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
    }

    /// One line in `actions.jsonl` for an action that ran: when, what, how it
    /// went. Never the output (window titles can be private). A write that
    /// fails never stops the action.
    fn record(&self, verb: &str, params: &Value, ok: bool, goal: &str) {
        let Some(dir) = &self.audit_dir else { return };
        let risk = friday_actions::desktop_risk_of(verb)
            .map(|r| format!("{r:?}").to_lowercase())
            .unwrap_or_else(|| "unknown".into());
        let line = json!({
            "ts": chrono::Utc::now().to_rfc3339(),
            "verb": verb,
            "params": params,
            "risk": risk,
            "ok": ok,
            "goal": goal.chars().take(120).collect::<String>(),
        });
        let _ = std::fs::create_dir_all(dir);
        let mut opts = std::fs::OpenOptions::new();
        opts.create(true).append(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            opts.mode(0o600);
        }
        if let Ok(mut f) = opts.open(dir.join("actions.jsonl")) {
            use std::io::Write;
            let _ = writeln!(f, "{line}");
        }
    }
}

const GUIDE: &str = r#"You control the person's Linux desktop (Hyprland) for them. Do what they asked, one action at a time.
Reply with ONLY one JSON object, nothing else:
  {"action": "<name>", "params": {...}}   to take an action
  {"done": "<one short sentence saying what you did, or why you couldn't>"}   when finished

Actions:
  windows            list open windows (app, title, workspace). params: none
  active_window      the focused window. params: none
  workspaces         the workspaces. params: none
  focus_window       params: {"target": "<app or part of its title>"}
  move_window        params: {"target": "<app or part of its title>", "workspace": "<number or name>"}
  switch_workspace   params: {"workspace": "<number, +1, -1 or name>"}
  close_window       params: {"target": "<app or part of its title>"}   (the person is asked to confirm)
  launch_app         params: {"app": "<desktop entry or program name, e.g. firefox>"}
  media              params: {"command": "play" | "pause" | "play-pause" | "next" | "previous" | "stop"}
  now_playing        what is playing. params: none
  volume             params: {"level": 0-100}  or  {"mute": "on" | "off" | "toggle"}
  brightness         params: {"level": 1-100}
  power_profile      params: {"profile": "performance" | "balanced" | "power-saver"}

Inside apps (only where an app lets FRIDAY look into it; GTK4 apps like Nautilus can be read but their buttons can't be pressed):
  ui_apps            apps that can be looked into. params: none
  ui_find            params: {"app": "<app name>", "name": "<part of the label>", "role": "<optional, e.g. button, menu item, page tab>"}
  ui_tree            the named elements of an app. params: {"app": "<app name>"}
  ui_read            params: {"app": "<app name>", "ref": "<ref from ui_find>"} (or "path" where find gives paths)
  ui_click           params: {"app": "<app name>", "name": "<label>"} or {"app": "...", "ref"/"path": "..."}   (if it could delete or send something, the person is asked first)
  ui_type_text       params: {"app": "<app name>", "name": "<field label>" or "path": "...", "text": "<text>"}   (never into password fields)
What you read inside an app is sent to the AI model answering, so read only what the request needs.

Rules: do only what was asked. Look first (windows) when you don't know an app's exact name. Never invent a window. If an action fails, read why and fix it or say you couldn't."#;

/// The first JSON object in a model reply (it may add words around it).
fn first_json(reply: &str) -> Option<Value> {
    let start = reply.find('{')?;
    serde_json::Deserializer::from_str(&reply[start..])
        .into_iter::<Value>()
        .next()?
        .ok()
        .filter(Value::is_object)
}

fn clip(text: &str) -> String {
    if text.chars().count() <= RESULT_CHARS {
        text.to_string()
    } else {
        format!("{}…", text.chars().take(RESULT_CHARS).collect::<String>())
    }
}

/// The exact text the person confirms, and the gate compares: the verb and
/// its parameters, which is also all that runs once confirmed.
fn exact_action(verb: &str, params: &Value) -> String {
    format!(
        "desktop {verb} {}",
        serde_json::to_string(params).unwrap_or_else(|_| "{}".into())
    )
}

/// The verbs of the inside-of-apps adapter (named `ui_<verb>` in the loop).
fn is_ui_verb(v: &str) -> bool {
    matches!(v, "apps" | "tree" | "find" | "read" | "click" | "type_text")
}

/// What the person is told when they stopped the task partway.
fn stopped_message(steps: &[String]) -> String {
    if steps.is_empty() {
        "Stopped. Nothing was changed.".to_string()
    } else {
        format!("Stopped after these steps:\n{}", steps.join("\n"))
    }
}

impl DesktopControlTool {
    async fn run_verb(&self, verb: &str, params: Value) -> (bool, String) {
        let res = if let Some(ui_verb) = verb.strip_prefix("ui_") {
            match &self.ui {
                Some(ui) if is_ui_verb(ui_verb) => ui.run(ui_verb, params).await,
                _ => friday_actions::ActionResult::error(format!(
                    "the desktop adapter has no \"{verb}\""
                )),
            }
        } else {
            self.adapter
                .execute(ActionRequest::new("desktop", verb, params))
                .await
        };
        if res.success {
            (true, clip(&res.output.to_string()))
        } else {
            (false, res.error.unwrap_or_else(|| "failed".into()))
        }
    }

    /// A confirmed action runs by itself, with no model in between.
    async fn run_confirmed(&self, confirmed: &str) -> ToolResult {
        let Some(rest) = confirmed.strip_prefix("desktop ") else {
            return ToolResult::error("that confirmation is not for a desktop action");
        };
        let (verb, json_text) = rest.split_once(' ').unwrap_or((rest, "{}"));
        let params: Value = serde_json::from_str(json_text).unwrap_or(json!({}));
        // The person has just said yes to this exact click: tell the adapter,
        // which refuses a dangerous button on its own say-so.
        let mut to_run = params.clone();
        if verb == "ui_click" {
            to_run["confirmed"] = json!(true);
        }
        let (ok, out) = self.run_verb(verb, to_run).await;
        self.record(verb, &params, ok, "(confirmed)");
        if ok {
            ToolResult::success(format!("Did: {verb} {json_text}\nResult: {out}"))
        } else {
            ToolResult::error(out)
        }
    }
}

#[async_trait]
impl Tool for DesktopControlTool {
    fn name(&self) -> &str {
        "desktop"
    }

    fn description(&self) -> &str {
        "Controls windows, workspaces, apps, media, volume and brightness on this desktop (desktop.control)"
    }

    fn capabilities(&self) -> Vec<Capability> {
        vec![Capability::Custom(CAPABILITY.into())]
    }

    async fn execute(&self, action: &Action, _ctx: &ExecutionContext) -> ToolResult {
        // A confirmed destructive action: run exactly that and nothing else.
        let confirmed = self.gate.lock().unwrap().confirm.clone();
        if let Some(c) = confirmed.filter(|c| c.trim_start().starts_with("desktop ")) {
            if let Err(err) = self.gate.lock().unwrap().check_or_record("desktop", &c) {
                return err;
            }
            return self.run_confirmed(c.trim()).await;
        }

        let goal = friday_goal_execution::planning::goal_segment(&action.instruction)
            .trim()
            .to_string();
        let mut steps: Vec<String> = Vec::new();
        let mut log = String::new();
        for n in 1..=MAX_STEPS {
            if self.abort.load(Ordering::SeqCst) {
                return ToolResult::error(stopped_message(&steps));
            }
            let prompt = format!(
                "{GUIDE}\n\nThe person's request: {goal}\n\nSteps so far:\n{}\n\nYour next reply (JSON only):",
                if log.is_empty() { "(none yet)" } else { log.trim_end() }
            );
            let reply = tokio::select! {
                _ = self.cancelled() => return ToolResult::error(stopped_message(&steps)),
                r = tokio::time::timeout(MODEL_TIMEOUT, self.model.complete(&prompt)) => match r {
                    Ok(Ok(r)) => r,
                    Ok(Err(e)) => return ToolResult::error(format!("couldn't ask the model: {e}")),
                    Err(_) => {
                        return ToolResult::error("the model took too long to choose the next step")
                    }
                },
            };
            let Some(choice) = first_json(&reply) else {
                log.push_str(&format!(
                    "{n}. (your reply wasn't a JSON object; reply with JSON only)\n"
                ));
                continue;
            };
            if let Some(done) = choice.get("done").and_then(Value::as_str) {
                let mut out = format!("Done: {done}");
                if !steps.is_empty() {
                    out.push_str(&format!("\nSteps taken:\n{}", steps.join("\n")));
                }
                return ToolResult::success(out);
            }
            let Some(verb) = choice.get("action").and_then(Value::as_str) else {
                log.push_str(&format!("{n}. (no \"action\" or \"done\" in your reply)\n"));
                continue;
            };
            let params = choice
                .get("params")
                .cloned()
                .filter(Value::is_object)
                .unwrap_or(json!({}));
            let mut params = params;
            let is_ui = verb.starts_with("ui_");
            let risk = if is_ui {
                // Inside-of-apps actions are known by the UI adapter; whether a
                // click is dangerous is decided below, from the button's label.
                if self.ui.is_some() && is_ui_verb(&verb[3..]) {
                    Some(DesktopRisk::Reversible)
                } else {
                    None
                }
            } else {
                friday_actions::desktop_risk_of(verb)
            };
            let Some(mut risk) = risk else {
                log.push_str(&format!(
                    "{n}. {verb}: not an action; use one from the list\n"
                ));
                continue;
            };
            if verb == "ui_click" {
                // Find the button first without pressing it: the decision uses
                // its own label, and the action the person confirms names it.
                let ui = self.ui.as_ref().expect("checked above");
                match ui.resolve_click(&params).await {
                    Err(e) => {
                        self.record(verb, &params, false, &goal);
                        log.push_str(&format!("{n}. {verb} -> failed: {e}\n"));
                        steps.push(format!("- {verb}: failed"));
                        continue;
                    }
                    Ok((path, label)) => {
                        let app = params.get("app").cloned().unwrap_or(json!(""));
                        params = json!({"app": app, "label": label, "path": path});
                        if ui.is_dangerous(&label) {
                            risk = DesktopRisk::Destructive;
                        }
                    }
                }
            }
            if risk == DesktopRisk::Destructive {
                // Ask first, with the exact action; nothing has been closed.
                let exact = exact_action(verb, &params);
                let asked = self.gate.lock().unwrap().check_or_record("desktop", &exact);
                if let Err(err) = asked {
                    return err;
                }
            }
            let (ok, out) = self.run_verb(verb, params.clone()).await;
            self.record(verb, &params, ok, &goal);
            let line = format!(
                "{n}. {verb} {} -> {}{}",
                serde_json::to_string(&params).unwrap_or_default(),
                if ok { "ok: " } else { "failed: " },
                out
            );
            log.push_str(&line);
            log.push('\n');
            steps.push(format!(
                "- {verb} {}: {}",
                serde_json::to_string(&params).unwrap_or_default(),
                if ok { "ok" } else { "failed" }
            ));
        }
        ToolResult::error(format!(
            "stopped after {MAX_STEPS} steps without finishing. Steps taken:\n{}",
            if steps.is_empty() {
                "(none)".to_string()
            } else {
                steps.join("\n")
            }
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use friday_actions::DesktopExec;

    const CLIENTS: &str = r#"[
      {"address":"0xaaa1","mapped":true,"class":"firefox","title":"Docs","workspace":{"id":1,"name":"1"}},
      {"address":"0xbbb2","mapped":true,"class":"foot","title":"zsh","workspace":{"id":2,"name":"2"}}
    ]"#;

    /// Answers the window list and records every command.
    #[derive(Default)]
    struct Stub(Mutex<Vec<String>>);

    impl Stub {
        fn calls(&self) -> Vec<String> {
            self.0.lock().unwrap().clone()
        }
    }

    impl DesktopExec for Stub {
        fn run(&self, program: &str, args: &[String]) -> Result<String, String> {
            let line = format!("{program} {}", args.join(" "));
            self.0.lock().unwrap().push(line.clone());
            Ok(if line == "hyprctl -j clients" {
                CLIENTS.into()
            } else {
                String::new()
            })
        }
    }

    /// Replies with each scripted reply in turn, and keeps the prompts it got.
    struct Script {
        replies: Mutex<Vec<String>>,
        prompts: Mutex<Vec<String>>,
    }

    impl Script {
        fn new(replies: &[&str]) -> Arc<Self> {
            let mut r: Vec<String> = replies.iter().map(|s| s.to_string()).collect();
            r.reverse();
            Arc::new(Self {
                replies: Mutex::new(r),
                prompts: Mutex::new(Vec::new()),
            })
        }
    }

    #[async_trait]
    impl LlmStrategyClient for Script {
        async fn complete(&self, prompt: &str) -> Result<String, String> {
            self.prompts.lock().unwrap().push(prompt.to_string());
            self.replies
                .lock()
                .unwrap()
                .pop()
                .ok_or_else(|| "the script ran out".to_string())
        }
    }

    fn tool(
        model: Arc<Script>,
        stub: Arc<Stub>,
        gate: Arc<Mutex<ConfirmationGate>>,
    ) -> DesktopControlTool {
        DesktopControlTool::new(
            model,
            DesktopAdapter::with_exec(stub),
            gate,
            Arc::default(),
            None,
            None,
        )
    }

    async fn run(t: &DesktopControlTool, goal: &str) -> ToolResult {
        let action = Action::single(goal, Capability::Custom(CAPABILITY.into()), "g1");
        t.execute(&action, &ExecutionContext::for_goal("g1")).await
    }

    #[tokio::test]
    async fn the_model_chooses_steps_and_the_tool_runs_them_until_done() {
        let model = Script::new(&[
            r#"{"action":"windows","params":{}}"#,
            r#"Sure! {"action":"move_window","params":{"target":"firefox","workspace":"2"}}"#,
            r#"{"action":"media","params":{"command":"pause"}}"#,
            r#"{"done":"Moved Firefox to workspace 2 and paused the music."}"#,
        ]);
        let stub = Arc::new(Stub::default());
        let t = tool(model.clone(), stub.clone(), Arc::default());
        let r = run(&t, "put firefox on workspace 2 and pause the music").await;
        assert!(r.success, "{:?}", r.error);
        assert!(
            r.output
                .starts_with("Done: Moved Firefox to workspace 2 and paused the music."),
            "{}",
            r.output
        );
        assert!(
            r.output.contains("- move_window") && r.output.contains("- media"),
            "{}",
            r.output
        );
        let calls = stub.calls();
        assert!(
            calls.contains(&r#"hyprctl dispatch hl.dsp.window.move({ workspace = "2", follow = false, window = "address:0xaaa1" })"#.to_string()),
            "{calls:?}"
        );
        assert!(calls.contains(&"playerctl pause".to_string()), "{calls:?}");
        // The model saw the request and the earlier results.
        let prompts = model.prompts.lock().unwrap();
        assert!(prompts[0].contains("The person's request: put firefox on workspace 2"));
        assert!(
            prompts[2].contains("1. windows {} -> ok:") && prompts[2].contains("firefox"),
            "{}",
            prompts[2]
        );
    }

    #[tokio::test]
    async fn a_destructive_step_stops_and_asks_with_the_exact_action() {
        let model = Script::new(&[r#"{"action":"close_window","params":{"target":"firefox"}}"#]);
        let stub = Arc::new(Stub::default());
        let gate = Arc::new(Mutex::new(ConfirmationGate::default()));
        let t = tool(model, stub.clone(), gate.clone());
        let r = run(&t, "close the firefox window").await;
        assert!(!r.success);
        assert_eq!(
            r.error.unwrap(),
            r#"Confirm to run: desktop close_window {"target":"firefox"}"#
        );
        let pending = gate
            .lock()
            .unwrap()
            .pending
            .clone()
            .expect("a pending confirmation");
        assert_eq!(
            pending.action,
            r#"desktop close_window {"target":"firefox"}"#
        );
        assert!(
            !stub.calls().iter().any(|c| c.contains("window.close")),
            "nothing was closed"
        );
    }

    #[tokio::test]
    async fn once_confirmed_only_that_action_runs_and_the_model_is_not_asked() {
        let model = Script::new(&[]);
        let stub = Arc::new(Stub::default());
        let gate = Arc::new(Mutex::new(ConfirmationGate::default()));
        gate.lock().unwrap().confirm = Some(r#"desktop close_window {"target":"firefox"}"#.into());
        let t = tool(model.clone(), stub.clone(), gate);
        let r = run(&t, "close the firefox window").await;
        assert!(r.success, "{:?}", r.error);
        assert!(stub.calls().contains(
            &r#"hyprctl dispatch hl.dsp.window.close({ window = "address:0xaaa1" })"#.to_string()
        ));
        assert!(
            model.prompts.lock().unwrap().is_empty(),
            "no model call after a confirmation"
        );
    }

    #[tokio::test]
    async fn a_confirmation_for_another_action_is_not_enough() {
        // The confirmation is for one close; the model now asks for another.
        let model = Script::new(&[r#"{"action":"close_window","params":{"target":"foot"}}"#]);
        let stub = Arc::new(Stub::default());
        let gate = Arc::new(Mutex::new(ConfirmationGate::default()));
        gate.lock().unwrap().confirm = Some("git status".into());
        let t = tool(model, stub.clone(), gate);
        let r = run(&t, "close the terminal").await;
        assert!(!r.success);
        assert!(r
            .error
            .unwrap()
            .starts_with("Confirm to run: desktop close_window"));
        assert!(!stub.calls().iter().any(|c| c.contains("window.close")));
    }

    #[tokio::test]
    async fn a_confirmation_for_a_command_never_runs_a_desktop_action() {
        let model = Script::new(&[]);
        let stub = Arc::new(Stub::default());
        let gate = Arc::new(Mutex::new(ConfirmationGate::default()));
        gate.lock().unwrap().confirm = Some("desktop rm_rf {}".into());
        let t = tool(model, stub.clone(), gate);
        let r = run(&t, "anything").await;
        assert!(!r.success, "an unknown verb must not run");
        assert!(stub.calls().is_empty());
    }

    #[tokio::test]
    async fn bad_replies_and_unknown_actions_are_fed_back_then_the_loop_stops_at_the_limit() {
        let mut replies = vec![
            "I would like to focus firefox",
            r#"{"action":"format_disk","params":{}}"#,
        ];
        replies.extend(std::iter::repeat_n(
            r#"{"action":"workspaces","params":{}}"#,
            MAX_STEPS,
        ));
        let model = Script::new(&replies);
        let stub = Arc::new(Stub::default());
        let t = tool(model.clone(), stub, Arc::default());
        let r = run(&t, "tidy up").await;
        assert!(!r.success);
        assert!(r
            .error
            .unwrap()
            .contains(&format!("stopped after {MAX_STEPS} steps")));
        let prompts = model.prompts.lock().unwrap();
        assert_eq!(
            prompts.len(),
            MAX_STEPS,
            "no more model calls than the limit"
        );
        assert!(
            prompts[1].contains("wasn't a JSON object"),
            "{}",
            prompts[1]
        );
        assert!(
            prompts[2].contains("format_disk: not an action"),
            "{}",
            prompts[2]
        );
    }

    #[tokio::test]
    async fn a_failed_action_is_reported_to_the_model_which_can_say_so() {
        let model = Script::new(&[
            r#"{"action":"focus_window","params":{"target":"gimp"}}"#,
            r#"{"done":"GIMP isn't open, so I couldn't focus it."}"#,
        ]);
        let stub = Arc::new(Stub::default());
        let t = tool(model.clone(), stub, Arc::default());
        let r = run(&t, "focus gimp").await;
        assert!(r.success);
        assert!(r.output.contains("GIMP isn't open"));
        assert!(model.prompts.lock().unwrap()[1].contains("failed: no open window matches"));
    }

    #[tokio::test]
    async fn a_model_error_ends_the_goal_with_a_plain_message() {
        let model = Script::new(&[]);
        let t = tool(model, Arc::new(Stub::default()), Arc::default());
        let r = run(&t, "pause the music").await;
        assert!(!r.success);
        assert!(r.error.unwrap().contains("couldn't ask the model"));
    }

    /// Sets the abort flag as it answers its first question, like a Stop
    /// pressed while FRIDAY is partway through.
    struct StopsAfterFirst {
        abort: Arc<AtomicBool>,
        asked: Mutex<usize>,
    }

    #[async_trait]
    impl LlmStrategyClient for StopsAfterFirst {
        async fn complete(&self, _prompt: &str) -> Result<String, String> {
            *self.asked.lock().unwrap() += 1;
            self.abort.store(true, Ordering::SeqCst);
            Ok(r#"{"action":"media","params":{"command":"pause"}}"#.into())
        }
    }

    /// Never answers.
    struct Silent;

    #[async_trait]
    impl LlmStrategyClient for Silent {
        async fn complete(&self, _prompt: &str) -> Result<String, String> {
            std::future::pending().await
        }
    }

    fn temp_dir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("friday-desktop-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    #[tokio::test]
    async fn a_stop_before_the_first_step_changes_nothing_and_asks_no_model() {
        let model = Script::new(&[r#"{"action":"media","params":{"command":"pause"}}"#]);
        let stub = Arc::new(Stub::default());
        let t = DesktopControlTool::new(
            model.clone(),
            DesktopAdapter::with_exec(stub.clone()),
            Arc::default(),
            Arc::new(AtomicBool::new(true)),
            None,
            None,
        );
        let r = run(&t, "pause the music").await;
        assert!(!r.success);
        assert_eq!(r.error.unwrap(), "Stopped. Nothing was changed.");
        assert!(model.prompts.lock().unwrap().is_empty());
        assert!(stub.calls().is_empty());
    }

    #[tokio::test]
    async fn a_stop_partway_ends_the_loop_after_the_step_in_flight() {
        let abort = Arc::new(AtomicBool::new(false));
        let model = Arc::new(StopsAfterFirst {
            abort: abort.clone(),
            asked: Mutex::new(0),
        });
        let stub = Arc::new(Stub::default());
        let t = DesktopControlTool::new(
            model.clone(),
            DesktopAdapter::with_exec(stub.clone()),
            Arc::default(),
            abort,
            None,
            None,
        );
        let r = run(&t, "pause the music and then open firefox").await;
        assert!(!r.success);
        let e = r.error.unwrap();
        assert!(
            e.starts_with("Stopped after these steps:") && e.contains("- media"),
            "{e}"
        );
        assert_eq!(
            *model.asked.lock().unwrap(),
            1,
            "no second question after the stop"
        );
        assert!(stub.calls().contains(&"playerctl pause".to_string()));
        assert!(!stub
            .calls()
            .iter()
            .any(|c| c.contains("exec_cmd") || c.contains("gtk-launch")));
    }

    #[tokio::test]
    async fn a_stop_while_the_model_is_thinking_does_not_wait_for_it() {
        let abort = Arc::new(AtomicBool::new(false));
        let t = DesktopControlTool::new(
            Arc::new(Silent),
            DesktopAdapter::with_exec(Arc::new(Stub::default())),
            Arc::default(),
            abort.clone(),
            None,
            None,
        );
        let flag = abort.clone();
        tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(250)).await;
            flag.store(true, Ordering::SeqCst);
        });
        let started = std::time::Instant::now();
        let r = run(&t, "pause the music").await;
        assert_eq!(r.error.unwrap(), "Stopped. Nothing was changed.");
        assert!(
            started.elapsed() < std::time::Duration::from_secs(5),
            "waited for the model"
        );
    }

    #[tokio::test]
    async fn every_action_taken_is_recorded_without_its_output() {
        let dir = temp_dir("audit");
        let model = Script::new(&[
            r#"{"action":"windows","params":{}}"#,
            r#"{"action":"focus_window","params":{"target":"gimp"}}"#,
            r#"{"action":"media","params":{"command":"pause"}}"#,
            r#"{"done":"ok"}"#,
        ]);
        let t = DesktopControlTool::new(
            model,
            DesktopAdapter::with_exec(Arc::new(Stub::default())),
            Arc::default(),
            Arc::default(),
            Some(dir.clone()),
            None,
        );
        run(&t, "pause the music").await;
        let text = std::fs::read_to_string(dir.join("actions.jsonl")).unwrap();
        let lines: Vec<Value> = text
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect();
        assert_eq!(lines.len(), 3, "{text}");
        assert_eq!(lines[0]["verb"], "windows");
        assert_eq!(lines[0]["ok"], true);
        assert_eq!(lines[1]["verb"], "focus_window");
        assert_eq!(lines[1]["ok"], false);
        assert_eq!(lines[2]["risk"], "reversible");
        assert_eq!(lines[2]["goal"], "pause the music");
        assert!(lines[0]["ts"].as_str().unwrap().contains('T'));
        // Window titles are in the adapter's output: not in the log.
        assert!(!text.contains("Docs") && !text.contains("zsh"), "{text}");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(dir.join("actions.jsonl"))
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(mode & 0o777, 0o600);
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn a_confirmed_close_is_recorded_as_confirmed() {
        let dir = temp_dir("confirmed");
        let gate = Arc::new(Mutex::new(ConfirmationGate::default()));
        gate.lock().unwrap().confirm = Some(r#"desktop close_window {"target":"firefox"}"#.into());
        let t = DesktopControlTool::new(
            Script::new(&[]),
            DesktopAdapter::with_exec(Arc::new(Stub::default())),
            gate,
            Arc::default(),
            Some(dir.clone()),
            None,
        );
        run(&t, "close the firefox window").await;
        let text = std::fs::read_to_string(dir.join("actions.jsonl")).unwrap();
        let line: Value = serde_json::from_str(text.lines().next().unwrap()).unwrap();
        assert_eq!(line["verb"], "close_window");
        assert_eq!(line["risk"], "destructive");
        assert_eq!(line["goal"], "(confirmed)");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A stand-in for the inside-of-apps adapter: records what ran, resolves a
    /// click by name from a fixed list of (label, path).
    struct StubUi {
        ran: Mutex<Vec<(String, Value)>>,
        buttons: Vec<(&'static str, &'static str)>,
    }

    impl StubUi {
        fn new(buttons: &[(&'static str, &'static str)]) -> Arc<Self> {
            Arc::new(Self {
                ran: Mutex::new(Vec::new()),
                buttons: buttons.to_vec(),
            })
        }
        fn ran(&self) -> Vec<(String, Value)> {
            self.ran.lock().unwrap().clone()
        }
    }

    #[async_trait]
    impl UiControl for StubUi {
        async fn run(&self, verb: &str, params: Value) -> friday_actions::ActionResult {
            self.ran.lock().unwrap().push((verb.to_string(), params));
            friday_actions::ActionResult::success(json!([{"name": "Save", "path": "0/1"}]))
        }
        async fn resolve_click(&self, params: &Value) -> Result<(String, String), String> {
            let want = params["name"].as_str().unwrap_or("").to_lowercase();
            self.buttons
                .iter()
                .find(|(label, _)| label.to_lowercase() == want)
                .map(|(l, p)| (p.to_string(), l.to_string()))
                .ok_or_else(|| format!("no clickable element matches \"{want}\""))
        }
        fn is_dangerous(&self, label: &str) -> bool {
            matches!(label.to_lowercase().as_str(), "delete" | "send" | "pay")
        }
    }

    fn tool_with_ui(
        model: Arc<Script>,
        ui: Arc<StubUi>,
        gate: Arc<Mutex<ConfirmationGate>>,
    ) -> DesktopControlTool {
        DesktopControlTool::new(
            model,
            DesktopAdapter::with_exec(Arc::new(Stub::default())),
            gate,
            Arc::default(),
            None,
            Some(ui),
        )
    }

    #[tokio::test]
    async fn the_model_can_look_inside_an_app_and_press_a_harmless_button() {
        let model = Script::new(&[
            r#"{"action":"ui_find","params":{"app":"gedit","name":"save"}}"#,
            r#"{"action":"ui_click","params":{"app":"gedit","name":"Save"}}"#,
            r#"{"done":"Saved the file."}"#,
        ]);
        let ui = StubUi::new(&[("Save", "0/1"), ("Delete", "0/2")]);
        let t = tool_with_ui(model, ui.clone(), Arc::default());
        let r = run(&t, "save the file in gedit").await;
        assert!(r.success, "{:?}", r.error);
        let ran = ui.ran();
        assert_eq!(ran[0].0, "find");
        // The click is by the resolved path, with the button's label alongside.
        assert_eq!(ran[1].0, "click");
        assert_eq!(ran[1].1["path"], "0/1");
        assert_eq!(ran[1].1["label"], "Save");
        assert!(
            ran[1].1.get("confirmed").is_none(),
            "no confirmation was needed or claimed"
        );
    }

    #[tokio::test]
    async fn a_button_that_could_lose_something_asks_first_naming_the_button() {
        let model =
            Script::new(&[r#"{"action":"ui_click","params":{"app":"files","name":"Delete"}}"#]);
        let ui = StubUi::new(&[("Delete", "0/2")]);
        let gate = Arc::new(Mutex::new(ConfirmationGate::default()));
        let t = tool_with_ui(model, ui.clone(), gate.clone());
        let r = run(&t, "delete the selected file").await;
        assert!(!r.success);
        assert_eq!(
            r.error.unwrap(),
            r#"Confirm to run: desktop ui_click {"app":"files","label":"Delete","path":"0/2"}"#
        );
        assert!(ui.ran().is_empty(), "nothing was pressed");
        assert_eq!(
            gate.lock().unwrap().pending.clone().unwrap().action,
            r#"desktop ui_click {"app":"files","label":"Delete","path":"0/2"}"#
        );
    }

    #[tokio::test]
    async fn once_confirmed_the_button_is_pressed_and_told_it_was_confirmed() {
        let ui = StubUi::new(&[("Delete", "0/2")]);
        let gate = Arc::new(Mutex::new(ConfirmationGate::default()));
        gate.lock().unwrap().confirm =
            Some(r#"desktop ui_click {"app":"files","label":"Delete","path":"0/2"}"#.into());
        let model = Script::new(&[]);
        let t = tool_with_ui(model.clone(), ui.clone(), gate);
        let r = run(&t, "delete the selected file").await;
        assert!(r.success, "{:?}", r.error);
        let ran = ui.ran();
        assert_eq!(ran.len(), 1);
        assert_eq!(ran[0].1["confirmed"], true);
        assert!(model.prompts.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn a_click_that_cannot_be_resolved_is_reported_and_nothing_is_pressed() {
        let model = Script::new(&[
            r#"{"action":"ui_click","params":{"app":"files","name":"Nope"}}"#,
            r#"{"done":"There is no such button."}"#,
        ]);
        let ui = StubUi::new(&[("Save", "0/1")]);
        let t = tool_with_ui(model.clone(), ui.clone(), Arc::default());
        let r = run(&t, "press nope").await;
        assert!(r.success);
        assert!(ui.ran().is_empty());
        assert!(model.prompts.lock().unwrap()[1].contains("failed: no clickable element matches"));
    }

    #[tokio::test]
    async fn without_an_inside_of_apps_adapter_ui_actions_are_not_actions() {
        let model = Script::new(&[
            r#"{"action":"ui_apps","params":{}}"#,
            r#"{"done":"I can't look inside apps here."}"#,
        ]);
        let t = tool(model.clone(), Arc::new(Stub::default()), Arc::default());
        let r = run(&t, "what apps can you see inside").await;
        assert!(r.success);
        assert!(model.prompts.lock().unwrap()[1].contains("ui_apps: not an action"));
    }

    #[tokio::test]
    async fn an_unknown_ui_verb_never_reaches_the_adapter() {
        let model = Script::new(&[
            r#"{"action":"ui_delete_everything","params":{}}"#,
            r#"{"done":"ok"}"#,
        ]);
        let ui = StubUi::new(&[]);
        let t = tool_with_ui(model, ui.clone(), Arc::default());
        run(&t, "x").await;
        assert!(ui.ran().is_empty());
    }

    /// The whole chain on a live session: the loop, the click resolved first,
    /// the real AT-SPI adapter and a real GTK3 app (pavucontrol open). The model
    /// is scripted. Run in the FRIDAY OS test VM:
    /// `FRIDAY_TEST_REAL_ATSPI=1 cargo test -p friday-mobile --lib real_ui_loop -- --ignored`
    #[cfg(target_os = "linux")]
    #[tokio::test]
    #[ignore = "drives a live GTK3 app over the accessibility bus"]
    async fn real_ui_loop_presses_a_tab_in_a_live_app() {
        if std::env::var("FRIDAY_TEST_REAL_ATSPI").as_deref() != Ok("1") {
            eprintln!("set FRIDAY_TEST_REAL_ATSPI=1 to run this against the live session");
            return;
        }
        let dir = temp_dir("real-ui");
        let model = Script::new(&[
            r#"{"action":"ui_find","params":{"app":"pavucontrol","name":"","role":"page tab"}}"#,
            r#"{"action":"ui_click","params":{"app":"pavucontrol","name":"Configuration"}}"#,
            r#"{"action":"ui_click","params":{"app":"pavucontrol","name":"Playback"}}"#,
            r#"{"done":"Looked at the tabs and pressed Configuration, then Playback."}"#,
        ]);
        let t = DesktopControlTool::new(
            model.clone(),
            DesktopAdapter::new(),
            Arc::default(),
            Arc::default(),
            Some(dir.clone()),
            default_ui(),
        );
        let r = run(&t, "show me pavucontrol's configuration tab, then playback").await;
        assert!(r.success, "{:?} / {}", r.error, r.output);
        let text = std::fs::read_to_string(dir.join("actions.jsonl")).unwrap();
        let lines: Vec<Value> = text
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect();
        assert_eq!(lines.len(), 3, "{text}");
        assert!(lines.iter().all(|l| l["ok"] == true), "{text}");
        assert_eq!(lines[1]["verb"], "ui_click");
        assert_eq!(lines[1]["params"]["label"], "Configuration");
        // The model was shown the tabs it found.
        assert!(model.prompts.lock().unwrap()[1].contains("Configuration"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The whole chain with agent-desktop as the engine, on a live session: the
    /// loop with a scripted model, the real binary (FRIDAY_AGENT_DESKTOP) and a
    /// real GTK4 window (Nautilus open): find a toggle, press it by name, and see
    /// its state flip. Run in the FRIDAY OS test VM:
    /// `FRIDAY_TEST_REAL_ATSPI=1 FRIDAY_UI_ENGINE=agent-desktop FRIDAY_AGENT_DESKTOP=/path cargo test -p friday-mobile --lib real_agent_desktop -- --ignored`
    #[tokio::test]
    #[ignore = "drives a live GTK4 app through agent-desktop"]
    async fn real_agent_desktop_loop_flips_a_toggle() {
        if std::env::var("FRIDAY_TEST_REAL_ATSPI").as_deref() != Ok("1") {
            eprintln!("set FRIDAY_TEST_REAL_ATSPI=1 to run this against the live session");
            return;
        }
        let ui = default_ui().expect("agent-desktop not found (FRIDAY_AGENT_DESKTOP)");
        let dir = temp_dir("real-ad");
        let states_of = |ui: Arc<dyn UiControl>| async move {
            let r = ui
                .run("find", json!({"app": "nautilus", "name": "Search Everywhere", "role": "switch", "window": "Home"}))
                .await;
            assert!(r.success, "{:?}", r.error);
            r.output[0]["states"].clone()
        };
        let before = states_of(ui.clone()).await;
        let model = Script::new(&[
            r#"{"action":"ui_find","params":{"app":"nautilus","name":"Search Everywhere","role":"switch"}}"#,
            r#"{"action":"ui_click","params":{"app":"nautilus","name":"Search Everywhere"}}"#,
            r#"{"done":"Pressed the Search Everywhere switch."}"#,
        ]);
        let t = DesktopControlTool::new(
            model.clone(),
            DesktopAdapter::new(),
            Arc::default(),
            Arc::default(),
            Some(dir.clone()),
            Some(ui.clone()),
        );
        let r = run(&t, "turn on search in nautilus").await;
        assert!(r.success, "{:?} / {}", r.error, r.output);
        let text = std::fs::read_to_string(dir.join("actions.jsonl")).unwrap();
        let lines: Vec<Value> = text
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect();
        assert!(lines.iter().all(|l| l["ok"] == true), "{text}");
        assert_eq!(lines[1]["verb"], "ui_click");
        assert_eq!(lines[1]["params"]["label"], "Search Everywhere");
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        let after = states_of(ui.clone()).await;
        assert_ne!(before, after, "the switch did not change state");
        // Put it back.
        let _ = ui
            .run(
                "click",
                json!({"app": "nautilus", "name": "Search Everywhere", "window": "Home"}),
            )
            .await;
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn finds_the_json_in_a_chatty_reply() {
        assert_eq!(
            first_json(r#"ok {"done":"x"} thanks"#),
            Some(json!({"done":"x"}))
        );
        assert_eq!(first_json("no json here"), None);
        assert_eq!(first_json("[1,2]"), None);
    }
}
