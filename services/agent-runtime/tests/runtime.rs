//! Agent Runtime integration tests: the full spawn → run → mailbox lifecycle,
//! cancellation, hard timeouts, budget enforcement, bounded concurrency, and
//! result merge — all against deterministic closure drivers.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use friday_agent_runtime::{
    AgentAbort, AgentContext, AgentResult, AgentRuntime, AgentSpec, AgentState, AgentStep,
    AsyncFnDriver, CancellationToken, FnDriver,
};

fn succeeded(id: &str, summary: &str, steps: Vec<AgentStep>) -> AgentResult {
    AgentResult::succeeded(id.to_string(), summary, steps)
}

#[tokio::test]
async fn spawn_completes_and_reports_state() {
    let runtime = AgentRuntime::new();
    let driver = Arc::new(FnDriver::new(|_ctx| {
        succeeded(
            "worker-a",
            "checked git status",
            vec![AgentStep::ok(1, "git.status", 2)],
        )
    }));

    let spec = AgentSpec::new("worker-a", "check the git status").with_allowed_skills(&["git"]);
    let handle = runtime.spawn(spec, driver);

    // The handle observes a valid lifecycle state immediately (Pending,
    // Running, or — for an instant driver — already Succeeded).
    assert!(handle.state().is_success() || !handle.state().is_terminal());

    let result = handle.await_result().await;
    assert_eq!(result.state, AgentState::Succeeded);
    assert_eq!(result.summary, "checked git status");
    assert_eq!(result.steps.len(), 1);
}

#[tokio::test]
async fn cancellation_stops_a_running_worker() {
    let runtime = AgentRuntime::new();
    let driver = Arc::new(AsyncFnDriver::new(|ctx: AgentContext| {
        Box::pin(async move {
            ctx.cancellation.cancelled().await;
            AgentResult::aborted("slow", AgentAbort::Cancelled, "cancelled", vec![], 0)
        })
    }));

    let handle = runtime.spawn(AgentSpec::new("slow", "wait forever"), driver);
    handle.cancel();
    let result = handle.await_result().await;
    assert_eq!(result.state, AgentState::Cancelled);
    assert_eq!(result.error.as_deref(), Some("cancelled"));
}

#[tokio::test]
async fn runtime_hard_kills_a_worker_past_its_deadline() {
    let runtime = AgentRuntime::new().with_default_time_limit(Duration::from_millis(80));
    let driver = Arc::new(AsyncFnDriver::new(|_ctx: AgentContext| {
        Box::pin(async move {
            // Sleep far beyond the deadline without ever checking the context.
            tokio::time::sleep(Duration::from_secs(30)).await;
            succeeded("hang", "never reached", vec![])
        })
    }));

    let handle = runtime.spawn(AgentSpec::new("hang", "sleep"), driver);
    let result = handle.await_result().await;
    assert_eq!(result.state, AgentState::TimedOut);
    assert_eq!(result.error.as_deref(), Some("timed_out"));
}

#[tokio::test]
async fn driver_observes_budget_exhaustion() {
    let runtime = AgentRuntime::new();
    let driver = Arc::new(FnDriver::new(|ctx: AgentContext| {
        let mut steps = Vec::new();
        let mut idx = 1u32;
        loop {
            match ctx.check() {
                Ok(()) => {
                    ctx.record_step();
                    steps.push(AgentStep::ok(idx, format!("step {idx}"), 1));
                    idx += 1;
                }
                Err(AgentAbort::BudgetExhausted) => {
                    return AgentResult::aborted(
                        "stepper",
                        AgentAbort::BudgetExhausted,
                        "hit the step budget",
                        steps,
                        0,
                    );
                }
                Err(reason) => {
                    return AgentResult::aborted("stepper", reason, "aborted", steps, 0);
                }
            }
        }
    }));

    let spec = AgentSpec::new("stepper", "work").with_max_steps(3);
    let handle = runtime.spawn(spec, driver);
    let result = handle.await_result().await;
    assert_eq!(result.state, AgentState::BudgetExhausted);
    assert_eq!(result.steps.len(), 3, "exactly the budgeted steps ran");
}

#[tokio::test]
async fn bounded_concurrency_caps_parallel_workers() {
    let runtime = AgentRuntime::new().with_max_concurrency(2);
    let running = Arc::new(AtomicUsize::new(0));
    let peak = Arc::new(AtomicUsize::new(0));
    let running2 = Arc::clone(&running);
    let peak2 = Arc::clone(&peak);

    let driver = Arc::new(AsyncFnDriver::new(move |_ctx: AgentContext| {
        let running2 = Arc::clone(&running2);
        let peak2 = Arc::clone(&peak2);
        Box::pin(async move {
            let now = running2.fetch_add(1, Ordering::SeqCst) + 1;
            peak2.fetch_max(now, Ordering::SeqCst);
            tokio::time::sleep(Duration::from_millis(120)).await;
            running2.fetch_sub(1, Ordering::SeqCst);
            succeeded("w", "done", vec![])
        })
    }));

    let specs: Vec<AgentSpec> = (0..5)
        .map(|i| AgentSpec::new(format!("w{i}"), "work"))
        .collect();
    let handles = runtime.spawn_many(specs, driver);
    let results = runtime.await_all(handles).await;

    assert_eq!(results.len(), 5);
    assert!(results.iter().all(|r| r.state.is_success()));
    assert_eq!(
        peak.load(Ordering::SeqCst),
        2,
        "never more than 2 in parallel"
    );
}

#[tokio::test]
async fn queue_waits_in_pending_until_a_slot_frees() {
    let runtime = AgentRuntime::new().with_max_concurrency(1);
    let started = Arc::new(AtomicUsize::new(0));
    let started2 = Arc::clone(&started);
    let driver = Arc::new(AsyncFnDriver::new(move |_ctx: AgentContext| {
        let started2 = Arc::clone(&started2);
        Box::pin(async move {
            started2.fetch_add(1, Ordering::SeqCst);
            tokio::time::sleep(Duration::from_millis(80)).await;
            succeeded("w", "done", vec![])
        })
    }));

    let specs: Vec<AgentSpec> = (0..3)
        .map(|i| AgentSpec::new(format!("w{i}"), "work"))
        .collect();
    let handles = runtime.spawn_many(specs, driver);
    // Only one slot exists, so exactly one worker may have started by now.
    tokio::time::sleep(Duration::from_millis(30)).await;
    assert_eq!(started.load(Ordering::SeqCst), 1);

    let results = runtime.await_all(handles).await;
    assert!(results.iter().all(|r| r.state.is_success()));
    assert_eq!(started.load(Ordering::SeqCst), 3);
}

#[tokio::test]
async fn merge_aggregates_a_fan_out() {
    let runtime = AgentRuntime::new();
    let driver = Arc::new(FnDriver::new(|_ctx| succeeded("w", "done", vec![])));
    let specs = vec![
        AgentSpec::new("research", "find docs").with_allowed_skills(&["docs"]),
        AgentSpec::new("inspect", "check git").with_allowed_skills(&["git"]),
    ];
    let handles = runtime.spawn_many(specs, driver);
    let results = runtime.await_all(handles).await;
    let report = runtime.merge(results);

    assert_eq!(report.agents.len(), 2);
    assert_eq!(report.succeeded, 2);
    assert_eq!(report.failed, 0);
    assert!(report.all_succeeded());
    assert_eq!(report.summary_lines().len(), 2);
}

#[tokio::test]
async fn cancellation_token_shared_state() {
    let token = CancellationToken::new();
    let child = token.clone();
    token.cancel();
    assert!(child.is_cancelled());
    // A cancelled token resolves immediately.
    child.cancelled().await;
}

#[tokio::test]
async fn preferred_models_and_memory_scope_ride_through_the_spec() {
    use friday_agent_runtime::ModelCapability;
    let runtime = AgentRuntime::new();
    let driver = Arc::new(FnDriver::new(|ctx: AgentContext| {
        let spec = &ctx.spec;
        assert!(spec.preferred_models.contains(&ModelCapability::Coding));
        assert_eq!(spec.memory_scope, vec!["private.coding".to_string()]);
        assert_eq!(ctx.memory_scope(), &["private.coding".to_string()]);
        succeeded(&spec.id, "scope ok", vec![])
    }));
    let spec = AgentSpec::new("coder", "write code")
        .with_allowed_skills(&["git", "filesystem"])
        .with_preferred_models(&[ModelCapability::Coding])
        .with_memory_scope(&["private.coding"])
        .with_param("git.status", serde_json::json!({ "path": "/tmp" }));
    let handle = runtime.spawn(spec, driver);
    let result = handle.await_result().await;
    assert!(result.state.is_success(), "{result:?}");
}
