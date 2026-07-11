//! End-to-end test of the full planner pipeline using a mock executor plugin
//! in place of the real (M4) capability plugins.

use async_trait::async_trait;
use friday_core::event::Event;
use friday_core::plugin::{Plugin, PluginMetadata};
use friday_core::Friday;
use friday_planner::events;
use friday_planner::register_engines;
use serde_json::json;
use std::collections::HashSet;
use std::time::Duration;

/// Stands in for the future Windows/Browser/etc. plugins. It answers
/// `exec.action` events with an `exec.result`, optionally failing one
/// capability to exercise the retry path.
struct MockExecutor {
    meta: PluginMetadata,
    fail_cap: String,
}

#[async_trait]
impl Plugin for MockExecutor {
    fn metadata(&self) -> &PluginMetadata {
        &self.meta
    }

    async fn handle_event(&self, event: &Event) -> friday_core::Result<Vec<Event>> {
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
            .unwrap_or("")
            .to_string();
        let capability = event
            .payload
            .get("capability")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let success = capability != self.fail_cap;
        Ok(vec![events::event(
            events::EXEC_RESULT,
            "mock",
            json!({ "task_id": task_id, "plan_id": plan_id, "success": success, "output": {} }),
            event.trace_id,
        )])
    }
}

async fn run_pipeline(fail_cap: &str) -> (HashSet<String>, usize, usize) {
    friday_core::init_telemetry();
    let mut friday = Friday::new();
    register_engines(&mut friday).await.unwrap();

    let mock = MockExecutor {
        meta: PluginMetadata::builder("mock", "Mock Executor")
            .subscribes(&[events::EXEC_ACTION])
            .emits(&[events::EXEC_RESULT])
            .build(),
        fail_cap: fail_cap.to_string(),
    };
    friday.register_plugin(Box::new(mock)).await.unwrap();

    let bus = friday.bus().clone();
    let mut rx = bus.subscribe();
    bus.publish(Event::new(
        events::GOAL_RAW,
        "user",
        json!({ "text": "Open Chrome and search Tesla" }),
    ))
    .await
    .unwrap();

    let handle = tokio::spawn(async move { friday.run_with_timeout(Duration::from_secs(3)).await });

    let mut seen = HashSet::new();
    let mut task_completed = 0;
    let mut task_failed = 0;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(3);
    while tokio::time::Instant::now() < deadline {
        if let Ok(Ok(ev)) = tokio::time::timeout(Duration::from_millis(100), rx.recv()).await {
            seen.insert(ev.kind.clone());
            if ev.kind == events::TASK_COMPLETED {
                task_completed += 1;
            }
            if ev.kind == events::TASK_FAILED {
                task_failed += 1;
            }
        }
    }
    handle.await.unwrap().unwrap();
    (seen, task_completed, task_failed)
}

#[tokio::test]
async fn browser_search_happy_path() {
    let (seen, task_completed, _) = run_pipeline("").await;

    assert!(
        seen.contains(events::INTENT_CREATED),
        "intent.created missing"
    );
    assert!(seen.contains(events::PLAN_CREATED), "plan.created missing");
    assert!(seen.contains(events::TASK_STARTED), "task.started missing");
    assert!(seen.contains(events::EXEC_ACTION), "exec.action missing");
    assert!(
        seen.contains(events::TASK_COMPLETED),
        "task.completed missing"
    );
    assert!(
        seen.contains(events::VERIFICATION_COMPLETED),
        "verification.completed missing"
    );
    assert_eq!(task_completed, 4, "expected all 4 tasks to complete");
}

#[tokio::test]
async fn browser_search_retries_on_failure() {
    // The final "verify page loaded" task fails; the verifier should retry and
    // ultimately report failure rather than silently assuming success.
    let (seen, _, task_failed) = run_pipeline("page.verify").await;

    assert!(
        seen.contains(events::TASK_RETRY),
        "expected a retry after task failure"
    );
    assert!(
        seen.contains(events::VERIFICATION_FAILED),
        "expected verification.failed on exhausted retries"
    );
    assert!(task_failed >= 1, "expected at least one task.failed");
}
