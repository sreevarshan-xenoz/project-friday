//! Integration tests for the M4.4.5 kernel: execution context inheritance, the
//! Resource/Session/Lifecycle managers, and cancellation + metrics wired through
//! the Action Runtime.

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use friday_actions::adapters::ActionAdapter;
use friday_actions::kernel::{ExecutionContext, LifecycleState, ResourceId, ResourceManager};
use friday_actions::request::{ActionRequest, ActionResult};
use friday_actions::runtime::ActionRuntime;
use serde_json::json;
use uuid::Uuid;

/// Sleeps long enough that a cancellation can win the race.
struct SlowAdapter;

#[async_trait]
impl ActionAdapter for SlowAdapter {
    fn tool(&self) -> &str {
        "slow"
    }
    fn actions(&self) -> Vec<&str> {
        vec!["run"]
    }
    async fn execute(&self, _req: ActionRequest) -> ActionResult {
        tokio::time::sleep(Duration::from_secs(30)).await;
        ActionResult::success(json!({ "done": true }))
    }
}

/// Returns immediately.
struct FastAdapter;

#[async_trait]
impl ActionAdapter for FastAdapter {
    fn tool(&self) -> &str {
        "fast"
    }
    fn actions(&self) -> Vec<&str> {
        vec!["run"]
    }
    async fn execute(&self, _req: ActionRequest) -> ActionResult {
        ActionResult::success(json!({ "ok": true }))
    }
}

#[test]
fn context_child_inherits_and_points_to_parent() {
    let parent = ExecutionContext::builder()
        .session_id(Uuid::new_v4())
        .build();
    let child = parent.child(Uuid::new_v4());

    assert_eq!(child.session_id, parent.session_id);
    assert_eq!(child.parent_execution, Some(parent.execution_id));
    assert_eq!(child.trace_id, parent.trace_id);
}

#[test]
fn context_merge_combines_branches() {
    let parent = ExecutionContext::builder().build();
    let mut a = parent.child(Uuid::new_v4());
    a.variables.insert("a".into(), json!(1));
    let mut b = parent.child(Uuid::new_v4());
    b.variables.insert("b".into(), json!(2));

    let merged = ExecutionContext::merge(&parent, &[a, b]);
    assert_eq!(merged.variables.get("a"), Some(&json!(1)));
    assert_eq!(merged.variables.get("b"), Some(&json!(2)));
    assert_eq!(merged.session_id, parent.session_id);
}

#[tokio::test]
async fn cancellation_propagates_to_running_action() {
    let rt = Arc::new(ActionRuntime::new());
    rt.register_adapter(Box::new(SlowAdapter)).await;

    let ctx = ExecutionContext::builder().build();
    let token = ctx.cancellation_token.clone();

    let req = ActionRequest::new("slow", "run", json!({}));
    let rt2 = rt.clone();
    let handle = tokio::spawn(async move { rt2.run_action(req, ctx).await });

    // Cancel almost immediately; the 30s adapter must never win.
    tokio::time::sleep(Duration::from_millis(50)).await;
    token.cancel();

    let result = tokio::time::timeout(Duration::from_secs(5), handle)
        .await
        .expect("run_action hung")
        .expect("task panicked");
    assert!(!result.success);
    assert_eq!(result.error.as_deref(), Some("cancelled"));

    // Lifecycle should reflect cancellation.
    let state = rt
        .kernel()
        .lifecycle
        .state(rt.kernel().metrics.recent(1)[0].execution_id);
    assert_eq!(state, Some(LifecycleState::Cancelled));
    // A metric sample was recorded.
    assert!(rt.kernel().metrics.count() >= 1);
}

#[tokio::test]
async fn successful_action_records_metrics_and_completes() {
    let rt = Arc::new(ActionRuntime::new());
    rt.register_adapter(Box::new(FastAdapter)).await;

    let ctx = ExecutionContext::builder().build();
    let execution_id = ctx.execution_id;
    let req = ActionRequest::new("fast", "run", json!({}));
    let result = rt.run_action(req, ctx).await;

    assert!(result.success);
    assert_eq!(
        rt.kernel().lifecycle.state(execution_id),
        Some(LifecycleState::Completed)
    );
    let sample = rt.kernel().metrics.by_plugin("fast");
    assert_eq!(sample.len(), 1);
    assert!(sample[0].success);
}

#[tokio::test]
async fn resource_manager_owns_and_releases() {
    use async_trait::async_trait;
    use friday_core::Result;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;

    struct Flagged {
        id: ResourceId,
        acquired: Arc<AtomicBool>,
        released: Arc<AtomicBool>,
    }
    #[async_trait]
    impl friday_actions::kernel::ManagedResource for Flagged {
        fn id(&self) -> ResourceId {
            self.id
        }
        async fn acquire(&self) -> Result<()> {
            self.acquired.store(true, Ordering::SeqCst);
            Ok(())
        }
        async fn release(&self) -> Result<()> {
            self.released.store(true, Ordering::SeqCst);
            Ok(())
        }
        fn last_heartbeat(&self) -> chrono::DateTime<chrono::Utc> {
            chrono::Utc::now()
        }
        async fn shutdown(&self) -> Result<()> {
            Ok(())
        }
    }

    let acquired = Arc::new(AtomicBool::new(false));
    let released = Arc::new(AtomicBool::new(false));
    let mgr = ResourceManager::new();
    let id = mgr
        .register(Box::new(Flagged {
            id: ResourceId::new("TerminalSession", 0),
            acquired: acquired.clone(),
            released: released.clone(),
        }))
        .await
        .unwrap();
    assert!(acquired.load(Ordering::SeqCst));
    mgr.shutdown_all().await.unwrap();
    assert!(released.load(Ordering::SeqCst));
    assert_eq!(mgr.count(), 0);
    let _ = id;
}
