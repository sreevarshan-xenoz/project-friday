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
const RESULT_CHARS: usize = 700;

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

pub struct DesktopControlTool {
    model: Arc<dyn LlmStrategyClient>,
    adapter: DesktopAdapter,
    gate: Arc<Mutex<ConfirmationGate>>,
}

impl DesktopControlTool {
    pub(crate) fn new(
        model: Arc<dyn LlmStrategyClient>,
        adapter: DesktopAdapter,
        gate: Arc<Mutex<ConfirmationGate>>,
    ) -> Self {
        Self {
            model,
            adapter,
            gate,
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

impl DesktopControlTool {
    async fn run_verb(&self, verb: &str, params: Value) -> (bool, String) {
        let res = self
            .adapter
            .execute(ActionRequest::new("desktop", verb, params))
            .await;
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
        let (ok, out) = self.run_verb(verb, params).await;
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
            let prompt = format!(
                "{GUIDE}\n\nThe person's request: {goal}\n\nSteps so far:\n{}\n\nYour next reply (JSON only):",
                if log.is_empty() { "(none yet)" } else { log.trim_end() }
            );
            let reply =
                match tokio::time::timeout(MODEL_TIMEOUT, self.model.complete(&prompt)).await {
                    Ok(Ok(r)) => r,
                    Ok(Err(e)) => return ToolResult::error(format!("couldn't ask the model: {e}")),
                    Err(_) => {
                        return ToolResult::error("the model took too long to choose the next step")
                    }
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
            let Some(risk) = friday_actions::desktop_risk_of(verb) else {
                log.push_str(&format!(
                    "{n}. {verb}: not an action; use one from the list\n"
                ));
                continue;
            };
            if risk == DesktopRisk::Destructive {
                // Ask first, with the exact action; nothing has been closed.
                let exact = exact_action(verb, &params);
                let asked = self.gate.lock().unwrap().check_or_record("desktop", &exact);
                if let Err(err) = asked {
                    return err;
                }
            }
            let (ok, out) = self.run_verb(verb, params.clone()).await;
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
        DesktopControlTool::new(model, DesktopAdapter::with_exec(stub), gate)
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
