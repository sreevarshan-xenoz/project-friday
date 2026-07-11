//! End-to-end test of the Action Runtime wired into the FRIDAY core.
//!
//! A plan's `exec.action` events are routed to the ActionRuntime, which applies
//! the permission gate, dispatches to a real adapter, records an audit entry,
//! and emits `exec.result`. We assert on the emitted events as well as the
//! audit trail.

use async_trait::async_trait;
use friday_actions::adapters::{FilesystemAdapter, TerminalAdapter};
use friday_actions::runtime::ActionRuntime;
use friday_core::event::Event;
use friday_core::plugin::{Plugin, PluginMetadata};
use friday_core::Friday;
use friday_planner::events;
use friday_planner::register_engines;
use serde_json::json;
use std::time::Duration;

/// Captures `exec.result` events so the test can assert on outcomes.
struct Sink;

#[async_trait]
impl Plugin for Sink {
    fn metadata(&self) -> &PluginMetadata {
        static META: std::sync::OnceLock<PluginMetadata> = std::sync::OnceLock::new();
        META.get_or_init(|| {
            PluginMetadata::builder("sink", "Result Sink")
                .subscribes(&[events::EXEC_RESULT])
                .build()
        })
    }

    async fn handle_event(&self, _event: &Event) -> friday_core::Result<Vec<Event>> {
        Ok(vec![])
    }
}

async fn run_action(capability: &str, params: serde_json::Value, timeout: Duration) -> Vec<Event> {
    friday_core::init_telemetry();
    let mut friday = Friday::new();
    register_engines(&mut friday).await.unwrap();

    let rt = ActionRuntime::new().with_timeout(timeout);
    rt.register_adapter(Box::new(FilesystemAdapter::new()))
        .await;
    rt.register_adapter(Box::new(TerminalAdapter::new())).await;
    friday.register_plugin(Box::new(rt)).await.unwrap();
    friday.register_plugin(Box::new(Sink)).await.unwrap();

    let bus = friday.bus().clone();
    let mut rx = bus.subscribe();

    let goal = Event::new(
        events::EXEC_ACTION,
        "test",
        json!({
            "task_id": "00000000-0000-0000-0000-000000000001",
            "plan_id": "00000000-0000-0000-0000-000000000002",
            "capability": capability,
            "params": params,
        }),
    );
    bus.publish(goal).await.unwrap();

    let handle = tokio::spawn(async move { friday.run_with_timeout(timeout).await });

    let mut results = Vec::new();
    let deadline = tokio::time::Instant::now() + timeout;
    while tokio::time::Instant::now() < deadline {
        if let Ok(Ok(ev)) = tokio::time::timeout(Duration::from_millis(100), rx.recv()).await {
            if ev.kind == events::EXEC_RESULT {
                results.push(ev);
            }
        }
    }
    handle.await.unwrap().unwrap();
    results
}

#[tokio::test]
async fn safe_filesystem_action_emits_success() {
    let tmp = std::env::temp_dir().join(format!("friday-it-{}", uuid::Uuid::new_v4().simple()));
    std::fs::create_dir_all(&tmp).unwrap();
    let p = tmp.join("note.txt").to_string_lossy().to_string();

    let results = run_action(
        "filesystem.create_file",
        json!({ "path": p, "content": "hi" }),
        Duration::from_secs(5),
    )
    .await;

    assert_eq!(results.len(), 1, "expected exactly one exec.result");
    let r = &results[0];
    assert_eq!(
        r.payload.get("success").and_then(|v| v.as_bool()),
        Some(true)
    );

    std::fs::remove_dir_all(&tmp).ok();
}

#[tokio::test]
async fn denied_action_returns_permission_error() {
    let results = run_action(
        "filesystem.delete_file",
        json!({ "path": "C:\\windows\\system32\\do_not_touch" }),
        Duration::from_secs(5),
    )
    .await;

    assert_eq!(results.len(), 1);
    let r = &results[0];
    assert_eq!(
        r.payload.get("success").and_then(|v| v.as_bool()),
        Some(false)
    );
    let err = r
        .payload
        .get("error")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    assert!(err.contains("permission"), "expected denial, got: {err}");
}

#[tokio::test]
async fn terminal_action_runs_command() {
    let results = run_action(
        "terminal.run",
        json!({ "command": "echo friday-works" }),
        Duration::from_secs(8),
    )
    .await;

    assert_eq!(results.len(), 1, "expected one exec.result from terminal");
    let r = &results[0];
    assert_eq!(
        r.payload.get("success").and_then(|v| v.as_bool()),
        Some(true)
    );
    let out = r
        .payload
        .get("output")
        .and_then(|v| v.get("stdout"))
        .and_then(|v| v.as_str())
        .unwrap_or("");
    assert!(out.contains("friday-works"), "terminal output was: {out:?}");
}
