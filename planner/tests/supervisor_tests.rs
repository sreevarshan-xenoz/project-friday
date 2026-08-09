//! Integration tests for the M6 GoalSupervisor orchestration kernel.
//!
//! These drive the supervisor directly by feeding it pipeline events and
//! simulating `exec.result` outcomes — no real action runtime required. They
//! prove: autonomous completion, high-risk pause/resume, retention, and
//! event-sourced persistence replay.

use std::sync::Arc;

use friday_core::event::Event;
use friday_core::plugin::Plugin;
use friday_planner::events;
use friday_planner::execution::capability_registry::{CapabilityRegistry, CapabilitySpec, Risk};
use friday_planner::execution::persistence::GoalStore;
use friday_planner::execution::reflection::RuleReflector;
use friday_planner::execution::supervisor::{GoalSupervisor, PolicyAuthority};
use friday_planner::execution::GoalRuntime;
use friday_planner::planning::TaskGraph;
use serde_json::json;
use uuid::Uuid;

fn plan_event(graph: &TaskGraph, trace: Uuid) -> Event {
    events::event(
        events::PLAN_CREATED,
        "test",
        serde_json::to_value(graph).unwrap(),
        trace,
    )
}

fn result_event(plan_id: Uuid, task_id: Uuid, success: bool, trace: Uuid) -> Event {
    events::event(
        events::EXEC_RESULT,
        "test",
        json!({
            "plan_id": plan_id.to_string(),
            "task_id": task_id.to_string(),
            "success": success,
            "output": {"ok": success},
            "error": if success { None } else { Some("boom") },
        }),
        trace,
    )
}

fn resume_event(goal_id: Uuid, trace: Uuid) -> Event {
    events::event(
        events::GOAL_RESUME,
        "test",
        json!({ "goal_id": goal_id.to_string() }),
        trace,
    )
}

struct AllowAll;
impl PolicyAuthority for AllowAll {
    fn allows(&self, _t: &str, _a: &str) -> bool {
        true
    }
}

fn make_supervisor(store: Arc<GoalStore>) -> (GoalSupervisor, Arc<CapabilityRegistry>) {
    let reg = Arc::new(CapabilityRegistry::new());
    reg.register(CapabilitySpec::native(
        "test.ok",
        "ok",
        "safe task",
        "test",
        "ok",
        Risk::Safe,
    ));
    reg.register(CapabilitySpec::native(
        "test.danger",
        "danger",
        "high-risk task",
        "test",
        "danger",
        Risk::High,
    ));
    let sup = GoalSupervisor::with_reflector_and_pool(
        reg.clone(),
        Arc::new(AllowAll),
        store,
        Arc::new(RuleReflector::new(1)),
        4,
    );
    (sup, reg)
}

#[tokio::test]
async fn autonomous_goal_completes_without_human_input() {
    let dir = std::env::temp_dir().join(format!("friday_test_{}.redb", Uuid::new_v4()));
    let _ = std::fs::remove_file(&dir);
    let store = Arc::new(GoalStore::open(&dir).unwrap());
    let (sup, _reg) = make_supervisor(store.clone());

    let trace = Uuid::new_v4();
    let mut g = TaskGraph::new("do the thing");
    let a = g.add_task("step a", "test.ok", json!({}), &[]);
    let b = g.add_task("step b", "test.ok", json!({}), &[a]);

    let mut events_out = sup.handle_event(&plan_event(&g, trace)).await.unwrap();
    // Expect goal.started + task.started(a) + exec.action(a)
    let started_a = events_out
        .iter()
        .any(|e| e.kind == events::TASK_STARTED && e.payload["task_id"] == a.to_string());
    assert!(started_a, "task a should start immediately");

    // Capture the goal id from the emitted GOAL_STARTED event.
    let goal_id = events_out
        .iter()
        .find(|e| e.kind == events::GOAL_STARTED)
        .and_then(|e| {
            e.payload["goal_id"]
                .as_str()
                .and_then(|s| Uuid::parse_str(s).ok())
        })
        .unwrap();
    let plan_id = g.plan_id;

    // Complete task a.
    let mut more = sup
        .handle_event(&result_event(plan_id, a, true, trace))
        .await
        .unwrap();
    events_out.append(&mut more);
    // Completing a should start b.
    let started_b = events_out
        .iter()
        .any(|e| e.kind == events::TASK_STARTED && e.payload["task_id"] == b.to_string());
    assert!(started_b, "task b should start after a completes");

    // Complete task b -> goal completed.
    let more = sup
        .handle_event(&result_event(plan_id, b, true, trace))
        .await
        .unwrap();
    events_out.extend(more);
    let completed = events_out.iter().any(|e| e.kind == events::GOAL_COMPLETED);
    assert!(completed, "goal should complete autonomously");

    // Persistence: snapshot should exist and replay to a Completed goal.
    let rt: GoalRuntime = store.load(goal_id).unwrap().unwrap();
    assert_eq!(rt.state, friday_planner::planning::GoalState::Completed);
    let _ = std::fs::remove_file(&dir);
}

#[tokio::test]
async fn high_risk_task_pauses_then_resumes() {
    let dir = std::env::temp_dir().join(format!("friday_test_{}.redb", Uuid::new_v4()));
    let _ = std::fs::remove_file(&dir);
    let store = Arc::new(GoalStore::open(&dir).unwrap());
    let (sup, _reg) = make_supervisor(store.clone());

    let trace = Uuid::new_v4();
    let mut g = TaskGraph::new("dangerous goal");
    let d = g.add_task("danger", "test.danger", json!({}), &[]);

    let mut out = sup.handle_event(&plan_event(&g, trace)).await.unwrap();
    let paused = out.iter().any(|e| e.kind == events::GOAL_PAUSED)
        && out.iter().any(|e| e.kind == events::PERMISSION_PAUSED);
    assert!(paused, "high-risk task must pause the goal");

    // No task should have started (paused before dispatch).
    assert!(
        !out.iter().any(|e| e.kind == events::TASK_STARTED),
        "no task starts while paused"
    );

    // Capture the goal_id from the paused event.
    let goal_id = out
        .iter()
        .find(|e| e.kind == events::GOAL_PAUSED)
        .and_then(|e| {
            e.payload["goal_id"]
                .as_str()
                .and_then(|s| Uuid::parse_str(s).ok())
        })
        .unwrap();

    // Resume -> the danger task should now start and dispatch.
    let mut more = sup
        .handle_event(&resume_event(goal_id, trace))
        .await
        .unwrap();
    out.append(&mut more);
    let started = out
        .iter()
        .any(|e| e.kind == events::TASK_STARTED && e.payload["task_id"] == d.to_string());
    assert!(started, "resumed goal should start the danger task");

    // Complete it -> goal completes.
    let more = sup
        .handle_event(&result_event(g.plan_id, d, true, trace))
        .await
        .unwrap();
    out.extend(more);
    assert!(
        out.iter().any(|e| e.kind == events::GOAL_COMPLETED),
        "goal should complete after resuming high-risk task"
    );
    let _ = std::fs::remove_file(&dir);
}

#[tokio::test]
async fn shared_context_publishes_produced_output() {
    // M6.3: a task declaring `produces` should publish its output into the
    // goal-scoped context so downstream tasks can consume it.
    let dir = std::env::temp_dir().join(format!("friday_test_{}.redb", Uuid::new_v4()));
    let _ = std::fs::remove_file(&dir);
    let store = Arc::new(GoalStore::open(&dir).unwrap());
    let (sup, _reg) = make_supervisor(store.clone());

    let trace = Uuid::new_v4();
    let mut g = TaskGraph::new("publish goal");
    let a = g.add_task("compute", "test.ok", json!({}), &[]);
    if let Some(t) = g.task_mut(a) {
        t.produces = Some("workspace_root".to_string());
    }

    let _ = sup.handle_event(&plan_event(&g, trace)).await.unwrap();
    let more = sup
        .handle_event(&result_event(g.plan_id, a, true, trace))
        .await
        .unwrap();
    let published = more.iter().any(|e| e.kind == events::GOAL_COMPLETED);
    assert!(published, "goal should complete");

    // Snapshot must carry the published context value.
    let goal_id = more
        .iter()
        .find(|e| e.kind == events::GOAL_COMPLETED)
        .and_then(|e| {
            e.payload["goal_id"]
                .as_str()
                .and_then(|s| Uuid::parse_str(s).ok())
        })
        .unwrap();
    let rt: GoalRuntime = store.load(goal_id).unwrap().unwrap();
    assert_eq!(rt.context.len(), 1, "one artifact published");
    let val = rt.context.get("workspace_root").unwrap();
    assert_eq!(val, json!({ "ok": true }), "published value matches output");
    let _ = std::fs::remove_file(&dir);
}

#[tokio::test]
async fn streaming_emits_progress_and_chunks() {
    // M6.4: a result carrying `stream` chunks and `progress` should emit
    // TASK_STREAM and TASK_PROGRESS events for live client rendering.
    let dir = std::env::temp_dir().join(format!("friday_test_{}.redb", Uuid::new_v4()));
    let _ = std::fs::remove_file(&dir);
    let store = Arc::new(GoalStore::open(&dir).unwrap());
    let (sup, _reg) = make_supervisor(store.clone());

    let trace = Uuid::new_v4();
    let mut g = TaskGraph::new("stream goal");
    let a = g.add_task("work", "test.ok", json!({}), &[]);

    let _ = sup.handle_event(&plan_event(&g, trace)).await.unwrap();
    let stream_ev = events::event(
        events::EXEC_RESULT,
        "test",
        json!({
            "plan_id": g.plan_id.to_string(),
            "task_id": a.to_string(),
            "success": true,
            "output": {"ok": true},
            "progress": 0.5,
            "stream": ["Checking serde...", "Compiling planner..."],
        }),
        trace,
    );
    let more = sup.handle_event(&stream_ev).await.unwrap();
    let streamed: Vec<&Event> = more
        .iter()
        .filter(|e| e.kind == events::TASK_STREAM)
        .collect();
    assert_eq!(streamed.len(), 2, "two stream chunks emitted");
    assert!(
        streamed[0].payload["chunk"] == "Checking serde...",
        "first chunk correct"
    );
    assert!(
        more.iter().any(|e| e.kind == events::TASK_PROGRESS
            && e.payload["progress"].as_f64() == Some(0.5)),
        "progress event emitted"
    );
    let _ = std::fs::remove_file(&dir);
}

#[tokio::test]
async fn failure_triggers_retry_then_completes() {
    let dir = std::env::temp_dir().join(format!("friday_test_{}.redb", Uuid::new_v4()));
    let _ = std::fs::remove_file(&dir);
    let store = Arc::new(GoalStore::open(&dir).unwrap());
    let (sup, _reg) = make_supervisor(store.clone());

    let trace = Uuid::new_v4();
    let mut g = TaskGraph::new("flaky goal");
    let a = g.add_task("maybe fails", "test.ok", json!({}), &[]);

    let mut out = sup.handle_event(&plan_event(&g, trace)).await.unwrap();
    // Fail first.
    let mut more = sup
        .handle_event(&result_event(g.plan_id, a, false, trace))
        .await
        .unwrap();
    out.append(&mut more);
    let retried = out.iter().any(|e| e.kind == events::TASK_RETRYING);
    assert!(retried, "failure should trigger a retry");

    // Succeed on retry.
    let more = sup
        .handle_event(&result_event(g.plan_id, a, true, trace))
        .await
        .unwrap();
    out.extend(more);
    assert!(
        out.iter().any(|e| e.kind == events::GOAL_COMPLETED),
        "goal completes after retry succeeds"
    );
    let _ = std::fs::remove_file(&dir);
}
