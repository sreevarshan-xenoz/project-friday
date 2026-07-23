//! Sprint 001A harness core: for each [`Scenario`], execute over IPC, measure
//! latency, verify belief against reality (State Accuracy), and emit an
//! [`Outcome`] plus any benchmark-linked [`Issue`].
//!
//! Deliberately contains **no recovery and no Desktop State Model** — only facts.

use std::time::Instant;

use chrono::Utc;
use serde_json::json;

use base64::engine::Engine;
use friday_actions::adapters::ActionAdapter;
use friday_actions::request::{ActionRequest, ActionResult};
use friday_windows::model::Window;
use friday_windows::WindowsAdapter;

use crate::corpus::{Injection, Scenario, Verify};
use crate::issue::Issue;

/// Transport mode. `Real` talks to the running C# `windows-host`; `Mock` uses the
/// in-process mock so the harness is runnable without .NET.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Real,
    Mock,
}

/// One recorded IPC call.
#[derive(Debug, Clone)]
pub struct Call {
    pub intent: String,
    pub ok: bool,
    pub latency_ms: u128,
    pub ts: String,
}

/// Result of running one scenario.
#[derive(Debug)]
pub struct Outcome {
    pub id: String,
    pub group: String,
    pub action_ok: bool,
    pub latency_ms: u128,
    pub state_accuracy: Option<bool>,
    pub passed: bool,
    pub trace: Vec<Call>,
    pub issues: Vec<Issue>,
    pub screenshot: Option<Vec<u8>>,
    pub note: String,
    /// How the launch was resolved by the host, when applicable
    /// (`New Instance` / `Existing Instance` / `Failed`). Surfaced as a
    /// qualification metric so single-instance app behavior is observable.
    pub launch_resolution: Option<String>,
}

fn now() -> String {
    Utc::now().to_rfc3339()
}

async fn call(
    adapter: &WindowsAdapter,
    intent: &str,
    params: serde_json::Value,
    trace: &mut Vec<Call>,
) -> ActionResult {
    let verb = intent.strip_prefix("windows.").unwrap_or(intent);
    let start = Instant::now();
    let res = adapter
        .execute(ActionRequest::new("windows", verb, params))
        .await;
    trace.push(Call {
        intent: intent.to_string(),
        ok: res.success,
        latency_ms: start.elapsed().as_millis(),
        ts: now(),
    });
    res
}

fn needs_window_id(intent: &str) -> bool {
    matches!(
        intent,
        "windows.focus"
            | "windows.move"
            | "windows.resize"
            | "windows.close"
            | "windows.capture"
            | "windows.type"
            | "windows.click"
            | "windows.scroll"
    )
}

#[cfg(windows)]
fn kill_process(pid: u32) {
    let _ = std::process::Command::new("taskkill")
        .args(["/PID", &pid.to_string(), "/F"])
        .status();
}

#[cfg(not(windows))]
fn kill_process(_pid: u32) {}

/// Run a single scenario and return its outcome (with issues if it failed).
pub async fn run_scenario(adapter: &WindowsAdapter, scenario: &Scenario, mode: Mode) -> Outcome {
    let mut trace: Vec<Call> = Vec::new();

    // Setup: open a window we act on.
    let mut setup_resolution: Option<String> = None;
    let opened = if let Some(setup) = &scenario.setup {
        let res = call(adapter, setup.intent, setup.params.clone(), &mut trace).await;
        setup_resolution = res
            .metadata
            .get("launch_resolution")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        if !res.success {
            return Outcome {
                id: scenario.id.into(),
                group: scenario.group.into(),
                action_ok: false,
                latency_ms: 0,
                state_accuracy: Some(false),
                passed: false,
                trace,
                issues: vec![issue_for(
                    scenario,
                    "setup open failed",
                    "window opened",
                    &format!("open returned error: {:?}", res.error),
                )],
                screenshot: None,
                note: format!("setup failed: {:?}", res.error),
                launch_resolution: None,
            };
        }
        let win: Window = match serde_json::from_value(res.output.clone()) {
            Ok(w) => w,
            Err(e) => {
                return Outcome {
                    id: scenario.id.into(),
                    group: scenario.group.into(),
                    action_ok: false,
                    latency_ms: 0,
                    state_accuracy: Some(false),
                    passed: false,
                    trace,
                    issues: vec![issue_for(
                        scenario,
                        "setup open returned unparseable window",
                        "valid Window json",
                        &e.to_string(),
                    )],
                    screenshot: None,
                    note: "setup parse error".into(),
                    launch_resolution: None,
                };
            }
        };
        Some((win.id, win.process_id))
    } else {
        None
    };

    // Failure injection.
    if let Some(Injection::KillProcess) = scenario.injection {
        if let Some((ref id, pid)) = opened {
            match mode {
                Mode::Real => kill_process(pid),
                Mode::Mock => {
                    let _ = call(
                        adapter,
                        "windows.__crash",
                        json!({ "window_id": id }),
                        &mut trace,
                    )
                    .await;
                }
            }
        }
    }

    // Build the action, injecting the opened window id where required.
    let mut params = scenario.action.params.clone();
    if needs_window_id(scenario.action.intent) {
        if let Some((ref id, _)) = opened {
            params["window_id"] = json!(id);
        }
    }

    let start = Instant::now();
    let action_res = call(adapter, scenario.action.intent, params, &mut trace).await;
    let latency = start.elapsed().as_millis();

    // Verify belief against reality. The window id comes from setup (when the
    // scenario opens during setup) or from the action output (when the action
    // itself opens, e.g. Launch).
    let opened_id = opened.as_ref().map(|(id, _)| id.clone()).or_else(|| {
        action_res
            .output
            .get("id")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
    });
    let (state_accuracy, note, screenshot) =
        verify(scenario, adapter, &opened_id, &action_res, &mut trace).await;
    // Surface the host's error in the evidence note so a failure is debuggable
    // without reproducing it (criterion: useful Evidence Packs).
    let note = if !action_res.success {
        format!("{note} | host_error={:?}", action_res.error)
    } else {
        note
    };
    let launch_resolution = action_res
        .metadata
        .get("launch_resolution")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .or(setup_resolution);

    let action_ok = action_res.success;
    // For ErrorExpected, a graceful failure is success.
    let expected_ok = matches!(scenario.verify, Verify::ErrorExpected);
    let base_ok = if expected_ok { !action_ok } else { action_ok };
    let passed = base_ok && state_accuracy != Some(false);

    let mut issues = Vec::new();
    if !passed {
        let (expected, actual) = expected_actual(scenario, action_ok, state_accuracy, expected_ok);
        issues.push(issue_for(scenario, &note, &expected, &actual));
    }

    Outcome {
        id: scenario.id.into(),
        group: scenario.group.into(),
        action_ok,
        latency_ms: latency,
        state_accuracy,
        passed,
        trace,
        issues,
        screenshot,
        note,
        launch_resolution,
    }
}

fn issue_for(scenario: &Scenario, note: &str, expected: &str, actual: &str) -> Issue {
    let id = format!("WIN-{}", scenario.id.to_uppercase().replace('.', "-"));
    let benchmark = format!(
        "Windows Qualification / {} / {}",
        scenario.group,
        scenario
            .injection
            .map(|_| "Failure Injection".to_string())
            .unwrap_or_else(|| "Corpus".to_string())
    );
    Issue {
        id,
        title: format!("Windows qualification failure: {}", scenario.id),
        observed: format!("corpus {} ({})", scenario.id, scenario.group),
        expected: expected.to_string(),
        actual: format!("{actual} — {note}"),
        benchmark,
        status: "Open".into(),
    }
}

fn expected_actual(
    scenario: &Scenario,
    action_ok: bool,
    state_accuracy: Option<bool>,
    expected_ok: bool,
) -> (String, String) {
    let _ = scenario;
    let expected = if expected_ok {
        "action fails gracefully (error returned)"
    } else {
        "action succeeds and belief matches reality"
    };
    let actual = format!("action_ok={action_ok}, state_accuracy={:?}", state_accuracy);
    (expected.into(), actual)
}

/// Verify the scenario's claim, returning (state_accuracy, human note, optional
/// screenshot bytes).
async fn verify(
    scenario: &Scenario,
    adapter: &WindowsAdapter,
    opened_id: &Option<String>,
    action_res: &ActionResult,
    trace: &mut Vec<Call>,
) -> (Option<bool>, String, Option<Vec<u8>>) {
    match &scenario.verify {
        Verify::Launch => {
            let list = call(adapter, "windows.list", json!({}), trace).await;
            let fg = call(adapter, "windows.foreground", json!({}), trace).await;
            let in_list = list
                .output
                .as_array()
                .map(|a| {
                    a.iter()
                        .any(|w| w.get("id").and_then(|v| v.as_str()) == opened_id.as_deref())
                })
                .unwrap_or(false);
            let fg_ok = fg.output.get("id").and_then(|v| v.as_str()) == opened_id.as_deref();
            let acc = in_list && fg_ok;
            (
                Some(acc),
                format!("list_contains={in_list}, foreground_matches={fg_ok}"),
                None,
            )
        }
        Verify::Focus => {
            let fg = call(adapter, "windows.foreground", json!({}), trace).await;
            let fg_ok = fg.output.get("id").and_then(|v| v.as_str()) == opened_id.as_deref();
            (Some(fg_ok), format!("foreground_matches={fg_ok}"), None)
        }
        Verify::Bounds {
            x,
            y,
            width,
            height,
        } => {
            let list = call(adapter, "windows.list", json!({}), trace).await;
            let found = list
                .output
                .as_array()
                .and_then(|a| {
                    a.iter()
                        .find(|w| w.get("id").and_then(|v| v.as_str()) == opened_id.as_deref())
                })
                .cloned();
            match found {
                Some(w) => {
                    let b = w.get("bounds");
                    let ok = b.and_then(|b| b.get("x")).and_then(|v| v.as_i64()) == Some(*x as i64)
                        && b.and_then(|b| b.get("y")).and_then(|v| v.as_i64()) == Some(*y as i64)
                        && b.and_then(|b| b.get("width")).and_then(|v| v.as_u64())
                            == Some(*width as u64)
                        && b.and_then(|b| b.get("height")).and_then(|v| v.as_u64())
                            == Some(*height as u64);
                    (Some(ok), format!("bounds_match={ok}"), None)
                }
                None => (Some(false), "window not found in list".into(), None),
            }
        }
        Verify::Closed => {
            let list = call(adapter, "windows.list", json!({}), trace).await;
            let present = list
                .output
                .as_array()
                .map(|a| {
                    a.iter()
                        .any(|w| w.get("id").and_then(|v| v.as_str()) == opened_id.as_deref())
                })
                .unwrap_or(false);
            (
                Some(!present),
                format!("absent_from_list={}", !present),
                None,
            )
        }
        Verify::InputOk => (
            Some(action_res.success),
            format!("input_op_success={}", action_res.success),
            None,
        ),
        Verify::Clipboard { text } => {
            let read = call(adapter, "windows.clipboard.read", json!({}), trace).await;
            let got = read
                .output
                .get("text")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let ok = got == text;
            (
                Some(ok),
                format!("clipboard_roundtrip={ok} (got '{got}')"),
                None,
            )
        }
        Verify::Capture => {
            let has = action_res.output.get("data").is_some();
            let shot = action_res
                .output
                .get("data")
                .and_then(|v| v.as_str())
                .and_then(|s| base64::engine::general_purpose::STANDARD.decode(s).ok());
            (Some(has), format!("capture_has_data={has}"), shot)
        }
        Verify::Listed => {
            let list = call(adapter, "windows.list", json!({}), trace).await;
            let present = list
                .output
                .as_array()
                .map(|a| {
                    a.iter()
                        .any(|w| w.get("id").and_then(|v| v.as_str()) == opened_id.as_deref())
                })
                .unwrap_or(false);
            (Some(present), format!("listed={present}"), None)
        }
        Verify::FindByTitle { title } => {
            let list = call(adapter, "windows.list", json!({}), trace).await;
            let found = list
                .output
                .as_array()
                .map(|a| {
                    a.iter()
                        .any(|w| w.get("title").and_then(|v| v.as_str()) == Some(title.as_str()))
                })
                .unwrap_or(false);
            (Some(found), format!("title_found={found}"), None)
        }
        Verify::ErrorExpected => {
            let ok = !action_res.success;
            (
                Some(ok),
                format!("graceful_failure={ok} (action_ok={})", action_res.success),
                None,
            )
        }
    }
}
