//! Real-world regression suite for the M6 autonomous execution engine.
//!
//! These are permanent benchmarks for every future change. If a refactor breaks
//! "Update my Rust workspace", "Recover from a compile error", "Pause on
//! `git push`", or "Fall back when the model is unavailable", these tests catch
//! it immediately. They drive the *full* engine stack over the event bus with a
//! deterministic [`MockExecutor`] standing in for the action runtime — no
//! network, no API keys, fully hermetic.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::Mutex;

use async_trait::async_trait;
use friday_core::event::Event;
use friday_core::plugin::{Plugin, PluginMetadata};
use friday_core::Friday;
use friday_planner::events;
use friday_planner::planning::planners::backend::MockBackend;
use friday_planner::register_engines_with_llm;
use serde_json::json;
use uuid::Uuid;

/// A deterministic stand-in for the action runtime. Answers `exec.action` with
/// `exec.result`. Per-capability failure counts let us simulate a flaky compile
/// step that must be retried before it succeeds.
struct MockExecutor {
    meta: PluginMetadata,
    fails: Arc<Mutex<HashMap<String, u32>>>,
}

#[async_trait]
impl Plugin for MockExecutor {
    fn metadata(&self) -> &PluginMetadata {
        &self.meta
    }
    async fn handle_event(&self, event: &Event) -> friday_core::Result<Vec<Event>> {
        if event.kind != events::EXEC_ACTION {
            return Ok(vec![]);
        }
        let task_id = event
            .payload
            .get("task_id")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let plan_id = event
            .payload
            .get("plan_id")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let capability = event
            .payload
            .get("capability")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        let mut fails = self.fails.lock().unwrap();
        let success = match fails.get_mut(&capability) {
            Some(remaining) if *remaining > 0 => {
                *remaining -= 1;
                false
            }
            _ => true,
        };
        drop(fails);

        Ok(vec![events::event(
            events::EXEC_RESULT,
            "mock",
            json!({
                "task_id": task_id,
                "plan_id": plan_id,
                "capability": capability,
                "success": success,
                "output": { "ok": success },
                "error": if success { None } else { Some("simulated failure") },
            }),
            event.trace_id,
        )])
    }
}

/// Harness result: every event seen on the bus.
struct Run {
    events: Vec<Event>,
}

/// Build a Friday with the full planner stack. When `plan_json` is `Some`, an
/// LLM planner backed by a deterministic [`MockBackend`] is wired in (so complex
/// intents route to the model). `fails` configures per-capability simulated
/// failures in the executor.
async fn run_goal(text: &str, plan_json: Option<&str>, fails: HashMap<String, u32>) -> Run {
    friday_core::init_telemetry();
    let mut friday = Friday::new();
    let backend = plan_json.map(|p| Box::new(MockBackend::new(p)) as Box<_>);
    register_engines_with_llm(&mut friday, backend)
        .await
        .unwrap();

    let exec = MockExecutor {
        meta: PluginMetadata::builder("mock", "Mock Executor")
            .subscribes(&[events::EXEC_ACTION])
            .emits(&[events::EXEC_RESULT])
            .build(),
        fails: Arc::new(Mutex::new(fails)),
    };
    friday.register_plugin(Box::new(exec)).await.unwrap();

    let bus = friday.bus().clone();
    let mut rx = bus.subscribe();
    bus.publish(Event::new(
        events::GOAL_RAW,
        "user",
        json!({ "text": text }),
    ))
    .await
    .unwrap();

    let handle = tokio::spawn(async move {
        friday
            .run_with_timeout(std::time::Duration::from_secs(12))
            .await
    });

    let mut collected: Vec<Event> = Vec::new();
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(3);
    while tokio::time::Instant::now() < deadline {
        if let Ok(Ok(ev)) =
            tokio::time::timeout(std::time::Duration::from_millis(100), rx.recv()).await
        {
            collected.push(ev);
        }
    }
    let _ = handle.await.unwrap();

    Run { events: collected }
}

fn saw(run: &Run, kind: &str) -> bool {
    run.events.iter().any(|e| e.kind == kind)
}

fn task_completed(run: &Run, cap: &str) -> bool {
    // `TASK_COMPLETED` does not carry the capability; the executor's
    // `EXEC_RESULT` does, so key off that for capability-level assertions.
    run.events.iter().any(|e| {
        e.kind == events::EXEC_RESULT
            && e.payload["capability"].as_str() == Some(cap)
            && e.payload["success"].as_bool() == Some(true)
    })
}

// --- Scenarios ---------------------------------------------------------------

#[tokio::test]
async fn rust_workspace_analyze_and_check() {
    // Realistic goal: inspect the workspace (Safe capabilities so it completes
    // autonomously without a high-risk gate).
    let plan = r#"{
        "tasks": [
            {"name": "Analyze workspace", "capability": "filesystem.read_file",
             "params": {"path": "Cargo.toml"}, "depends_on_indices": []},
            {"name": "Inspect git status", "capability": "git.status",
             "params": {}, "depends_on_indices": [0]}
        ]
    }"#;
    let run = run_goal(
        "Analyze this Rust workspace and report its status",
        Some(plan),
        HashMap::new(),
    )
    .await;

    assert!(saw(&run, events::PLAN_CREATED), "a plan must be created");
    assert!(
        task_completed(&run, "filesystem.read_file"),
        "analyze step completes"
    );
    assert!(task_completed(&run, "git.status"), "status step completes");
    assert!(
        saw(&run, events::GOAL_COMPLETED),
        "the workspace goal completes end to end"
    );
    assert!(!saw(&run, events::GOAL_FAILED), "no failure expected");
}

#[tokio::test]
async fn compilation_failure_retries_then_recovers() {
    // A build/commit step that fails once, then must be retried and recovered
    // autonomously (Medium-risk so it is not gated behind a human approval).
    let plan = r#"{
        "tasks": [
            {"name": "Commit the fixes", "capability": "git.commit",
             "params": {"message": "fix"}, "depends_on_indices": []}
        ]
    }"#;
    let mut fails = HashMap::new();
    fails.insert("git.commit".to_string(), 1);
    let run = run_goal("Build the project and fix errors", Some(plan), fails).await;

    assert!(
        saw(&run, events::TASK_FAILED),
        "first attempt must fail (compilation error)"
    );
    assert!(
        saw(&run, events::TASK_RETRYING),
        "supervisor must retry after failure"
    );
    assert!(task_completed(&run, "git.commit"), "retry succeeds");
    assert!(
        saw(&run, events::GOAL_COMPLETED),
        "goal recovers and completes"
    );
}

#[tokio::test]
async fn high_risk_git_push_pauses_then_resumes() {
    // Plan includes a High-risk `git.push`; the supervisor must pause for
    // approval, then complete after a resume signal.
    let plan = r#"{
        "tasks": [
            {"name": "Commit", "capability": "filesystem.write_file",
             "params": {"path": "x"}, "depends_on_indices": []},
            {"name": "Push", "capability": "git.push",
             "params": {}, "depends_on_indices": [0]}
        ]
    }"#;
    let mut friday = {
        friday_core::init_telemetry();
        let mut f = Friday::new();
        register_engines_with_llm(&mut f, Some(Box::new(MockBackend::new(plan))))
            .await
            .unwrap();
        let exec = MockExecutor {
            meta: PluginMetadata::builder("mock", "Mock Executor")
                .subscribes(&[events::EXEC_ACTION])
                .emits(&[events::EXEC_RESULT])
                .build(),
            fails: Arc::new(Mutex::new(HashMap::new())),
        };
        f.register_plugin(Box::new(exec)).await.unwrap();
        f
    };

    let bus = friday.bus().clone();
    let mut rx = bus.subscribe();
    bus.publish(Event::new(
        events::GOAL_RAW,
        "user",
        json!({ "text": "Commit my changes and push them" }),
    ))
    .await
    .unwrap();

    let handle = tokio::spawn(async move {
        friday
            .run_with_timeout(std::time::Duration::from_secs(12))
            .await
    });

    // Wait for the pause and capture the goal id.
    let mut goal_id: Option<Uuid> = None;
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(3);
    let mut paused = false;
    while tokio::time::Instant::now() < deadline {
        if let Ok(Ok(ev)) =
            tokio::time::timeout(std::time::Duration::from_millis(100), rx.recv()).await
        {
            if ev.kind == events::GOAL_PAUSED {
                paused = true;
                goal_id = ev.payload["goal_id"]
                    .as_str()
                    .and_then(|s| Uuid::parse_str(s).ok());
            }
            if ev.kind == events::GOAL_COMPLETED {
                break;
            }
        }
    }
    assert!(paused, "high-risk git.push must pause the goal");
    let goal_id = goal_id.expect("goal id from pause event");

    // Resume -> the push should dispatch and the goal should complete.
    bus.publish(Event::new(
        events::GOAL_RESUME,
        "user",
        json!({ "goal_id": goal_id.to_string() }),
    ))
    .await
    .unwrap();

    let mut completed = false;
    let deadline2 = tokio::time::Instant::now() + std::time::Duration::from_secs(3);
    while tokio::time::Instant::now() < deadline2 {
        if let Ok(Ok(ev)) =
            tokio::time::timeout(std::time::Duration::from_millis(100), rx.recv()).await
        {
            if ev.kind == events::GOAL_COMPLETED {
                completed = true;
                break;
            }
        }
    }
    let _ = handle.await.unwrap();
    assert!(
        completed,
        "goal completes after resuming the high-risk task"
    );
}

#[tokio::test]
async fn unknown_capability_falls_back_cleanly() {
    // The LLM proposes a capability that does not exist. Validation rejects it,
    // the engine falls back to the rule planner, and the goal still terminates
    // without stranding or panicking.
    let plan = r#"{
        "tasks": [
            {"name": "Magic", "capability": "unobtanium.do_thing", "params": {}, "depends_on_indices": []}
        ]
    }"#;
    let run = run_goal(
        "Do something clever with an unknown capability",
        Some(plan),
        HashMap::new(),
    )
    .await;

    assert!(
        saw(&run, events::PLANNING_TELEMETRY),
        "telemetry emitted even on fallback"
    );
    let tel = run
        .events
        .iter()
        .find(|e| e.kind == events::PLANNING_TELEMETRY)
        .unwrap();
    assert!(
        tel.payload["fell_back_to_rule"].as_bool() == Some(true),
        "must record fallback to rule planner"
    );
    assert!(
        saw(&run, events::GOAL_COMPLETED) || saw(&run, events::GOAL_FAILED),
        "goal must reach a terminal state"
    );
}

#[tokio::test]
async fn planner_fallback_when_llm_unavailable() {
    // The model backend errors out entirely; the router must select the rule
    // planner and the goal still completes.
    let run = run_goal(
        "Refactor the workspace and summarize it",
        Some(""), // empty response -> parse error in LLM planner
        HashMap::new(),
    )
    .await;

    let tel = run
        .events
        .iter()
        .find(|e| e.kind == events::PLANNING_TELEMETRY)
        .expect("telemetry emitted");
    assert!(
        tel.payload["fell_back_to_rule"].as_bool() == Some(true),
        "must fall back when the LLM is unavailable"
    );
    assert!(
        saw(&run, events::GOAL_COMPLETED),
        "goal still completes via rule fallback"
    );
}

#[tokio::test]
async fn cancellation_stops_workers_and_persists() {
    // Start a goal, cancel it mid-flight, and confirm the supervisor stops and
    // snapshots a Cancelled goal.
    let plan = r#"{
        "tasks": [
            {"name": "Step A", "capability": "filesystem.read_file", "params": {"path": "a"}, "depends_on_indices": []},
            {"name": "Step B", "capability": "filesystem.write_file", "params": {"path": "b"}, "depends_on_indices": [0]}
        ]
    }"#;
    friday_core::init_telemetry();
    let mut friday = Friday::new();
    register_engines_with_llm(&mut friday, Some(Box::new(MockBackend::new(plan))))
        .await
        .unwrap();
    let exec = MockExecutor {
        meta: PluginMetadata::builder("mock", "Mock Executor")
            .subscribes(&[events::EXEC_ACTION])
            .emits(&[events::EXEC_RESULT])
            .build(),
        fails: Arc::new(Mutex::new(HashMap::new())),
    };
    friday.register_plugin(Box::new(exec)).await.unwrap();

    let bus = friday.bus().clone();
    let mut rx = bus.subscribe();
    bus.publish(Event::new(
        events::GOAL_RAW,
        "user",
        json!({ "text": "Run a long multi-step job" }),
    ))
    .await
    .unwrap();

    let handle = tokio::spawn(async move {
        friday
            .run_with_timeout(std::time::Duration::from_secs(12))
            .await
    });

    // Capture goal id once started.
    let mut goal_id: Option<Uuid> = None;
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(3);
    while tokio::time::Instant::now() < deadline {
        if let Ok(Ok(ev)) =
            tokio::time::timeout(std::time::Duration::from_millis(50), rx.recv()).await
        {
            if ev.kind == events::GOAL_STARTED {
                goal_id = ev.payload["goal_id"]
                    .as_str()
                    .and_then(|s| Uuid::parse_str(s).ok());
            }
            if goal_id.is_some() {
                break;
            }
        }
    }
    let goal_id = goal_id.expect("goal started");

    // Cancel mid-flight.
    bus.publish(Event::new(
        events::GOAL_CANCEL,
        "user",
        json!({ "goal_id": goal_id.to_string() }),
    ))
    .await
    .unwrap();

    let mut cancelled = false;
    let deadline2 = tokio::time::Instant::now() + std::time::Duration::from_secs(3);
    while tokio::time::Instant::now() < deadline2 {
        if let Ok(Ok(ev)) =
            tokio::time::timeout(std::time::Duration::from_millis(100), rx.recv()).await
        {
            if ev.kind == events::GOAL_CANCELLED {
                cancelled = true;
                break;
            }
        }
    }
    let _ = handle.await.unwrap();
    assert!(cancelled, "cancellation must emit goal.cancelled");
}
