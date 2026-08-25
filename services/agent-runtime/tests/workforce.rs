//! Hermetic acceptance tests for the [`Workforce`] (MasterPlan #16): a
//! persistent worker is registered, assigned a task, **picked up across runs**
//! (a fresh `Workforce` over the same store), and the task runs through the
//! real `SkillAgentDriver` + the real kernel (no LLM, no network, no bus).
//! The worker's actions operate in its isolated persisted workspace.
//!
//! Acceptance under test: **"A worker can be assigned a task, picked up
//! across runs, report progress/blockers, and its workspace persists."**

use std::path::{Path, PathBuf};
use std::sync::Arc;

use friday_actions::{AgentKernel, DocsAdapter, SkillAgentDriver};
use friday_agent_runtime::{
    AgentContext, AgentDriver, AgentResult, AgentRole, FnDriver, InMemoryWorkforceStore,
    JsonWorkforceStore, SecurityScope, TaskOutcome, TaskStatus, WorkerStatus, Workforce,
    WorkforceStore,
};
use friday_planner::execution::capability_registry::CapabilityRegistry as PlannerCapabilityRegistry;
use friday_planner::execution::native_caps::register_native_builtins;
use friday_skills::{CapabilityResolver, SkillApprovals, SkillLoader, SkillRegistry, SkillSource};

/// Resolves capability ids against the real planner registry (the same view
/// `SkillAgentDriver` uses).
struct PlannerResolver(Arc<PlannerCapabilityRegistry>);

impl CapabilityResolver for PlannerResolver {
    fn exists(&self, capability: &str) -> bool {
        self.0.resolve(capability).is_some()
    }
}

fn write_skill(dir: &Path, id: &str, json: &str) {
    let sub = dir.join(id);
    std::fs::create_dir_all(&sub).unwrap();
    std::fs::write(sub.join("skill.json"), json).unwrap();
}

fn docs_manifest() -> &'static str {
    r#"{
        "id": "docs", "name": "Documentation", "version": "1.0.0",
        "description": "Search and read local documentation",
        "category": "docs",
        "capabilities": ["docs.search", "docs.read"],
        "actions": ["find documentation", "search the docs", "read a spec"],
        "permissions": [],
        "readonly": true,
        "platform": "Any", "min_core_version": "0.0.0", "enabled": true
    }"#
}

struct Fixture {
    _skills_dir: TempDir,
    driver: Arc<dyn AgentDriver>,
}

async fn fixture() -> Fixture {
    let skills_dir = TempDir::new();
    write_skill(skills_dir.path(), "docs", docs_manifest());

    let planner = Arc::new(PlannerCapabilityRegistry::new());
    register_native_builtins(&planner);

    let skills = Arc::new(SkillRegistry::new());
    let resolver: Arc<dyn CapabilityResolver> = Arc::new(PlannerResolver(Arc::clone(&planner)));
    let report = SkillLoader::new(Arc::clone(&skills), resolver)
        .load_dir(skills_dir.path(), SkillSource::User)
        .expect("load skills");
    assert!(report.ok(), "load failures: {:?}", report.failed);

    let kernel = Arc::new(AgentKernel::new());
    kernel.register_adapter(Box::new(DocsAdapter::new())).await;

    let driver: Arc<dyn AgentDriver> = Arc::new(SkillAgentDriver::new(
        Arc::clone(&skills),
        Arc::new(SkillApprovals::new()),
        Arc::clone(&planner),
        Arc::clone(&kernel),
    ));
    Fixture {
        _skills_dir: skills_dir,
        driver,
    }
}

#[tokio::test]
async fn persistent_worker_runs_a_real_docs_task_across_instances() {
    let fx = fixture().await;
    let docs_root = TempDir::new();
    std::fs::write(docs_root.path().join("README.md"), "# FRIDAY docs").unwrap();

    let root = TempDir::new();
    let store_root = root.path().join("store");
    let workspaces = root.path().join("workspaces");
    let shared: Arc<dyn WorkforceStore> = Arc::new(JsonWorkforceStore::new(&store_root));

    // Instance A: register the persistent worker + assign a real task.
    let (worker_id, task_id) = {
        let wf = Workforce::new(Arc::clone(&shared), Arc::clone(&fx.driver), &workspaces);
        let worker = wf
            .register(AgentRole::Researcher, "reads the project docs")
            .await
            .unwrap();
        assert_eq!(worker.status, WorkerStatus::Idle);
        assert!(
            worker.workspace.exists(),
            "the workspace directory must exist at registration"
        );

        let task = wf
            .create_task(
                "read the docs",
                "find the documentation",
                AgentRole::Researcher,
            )
            .await
            .unwrap();
        wf.assign(&worker.id, &task.id).await.unwrap();
        (worker.id, task.id)
    };

    // Instance B (fresh Workforce, same store — "picked up across runs"):
    // report progress, then run the assigned task through the real driver.
    {
        let wf = Workforce::new(Arc::clone(&shared), Arc::clone(&fx.driver), &workspaces);
        let workers = wf.workers().await.unwrap();
        assert_eq!(workers.len(), 1);
        assert_eq!(workers[0].status, WorkerStatus::Assigned);
        assert!(
            workers[0].workspace.exists(),
            "the workspace persists across instances"
        );

        let task = wf
            .task(&task_id)
            .await
            .unwrap()
            .expect("the assigned task persists");
        assert_eq!(task.status, TaskStatus::Assigned);

        // Attach the capability params the real docs.search needs, exactly
        // like the director acceptance does.
        let mut task = task;
        task.params.insert(
            "docs.search".to_string(),
            serde_json::json!({ "root": docs_root.path().to_string_lossy() }),
        );
        // Update through the store directly: params live on the board record.
        shared.save_task(&task).await.unwrap();

        wf.report_progress(&worker_id, "indexing the docs root")
            .await
            .unwrap();

        let outcome = wf.run(&worker_id).await.unwrap();
        let TaskOutcome::Done { task, result } = outcome else {
            panic!("the real worker must complete the task");
        };
        assert_eq!(task.status, TaskStatus::Done);
        assert_eq!(task.progress.len(), 1, "progress note persisted");
        assert!(
            result
                .steps
                .iter()
                .any(|s| s.description == "docs.search" && s.outcome == "ok"),
            "the worker must have run a real docs.search: {:?}",
            result.steps
        );
    }

    // Instance C: the done task, its persisted result, and the worker's
    // still-present workspace are all visible.
    {
        let wf = Workforce::new(Arc::clone(&shared), Arc::clone(&fx.driver), &workspaces);
        let task = wf
            .task(&task_id)
            .await
            .unwrap()
            .expect("the done task persists");
        assert_eq!(task.status, TaskStatus::Done);
        assert!(task.result.is_some(), "the result persists with the task");

        let workers = wf.workers().await.unwrap();
        assert_eq!(workers[0].status, WorkerStatus::Idle);
        assert!(workers[0].current_task.is_none());
        assert!(
            workers[0].workspace.exists(),
            "the worker's workspace persists"
        );
    }
}

#[tokio::test]
async fn in_memory_workforce_completes_the_assign_run_verify_flow() {
    let fx = fixture().await;
    let docs_root = TempDir::new();
    std::fs::write(docs_root.path().join("README.md"), "# FRIDAY docs").unwrap();

    let store: Arc<dyn WorkforceStore> = Arc::new(InMemoryWorkforceStore::new());
    let wf = Workforce::new(
        store,
        Arc::clone(&fx.driver),
        TempDir::new().path().to_path_buf(),
    );

    let worker = wf
        .register(AgentRole::Researcher, "reads docs")
        .await
        .unwrap();
    let mut task = wf
        .create_task(
            "read the docs",
            "find the documentation",
            AgentRole::Researcher,
        )
        .await
        .unwrap();
    task.params.insert(
        "docs.search".to_string(),
        serde_json::json!({ "root": docs_root.path().to_string_lossy() }),
    );
    wf.store().save_task(&task).await.unwrap();
    wf.assign(&worker.id, &task.id).await.unwrap();

    let outcome = wf.run(&worker.id).await.unwrap();
    let TaskOutcome::Done { task, result } = outcome else {
        panic!("expected Done");
    };
    assert_eq!(task.status, TaskStatus::Done);
    assert!(result
        .steps
        .iter()
        .any(|s| s.description == "docs.search" && s.outcome == "ok"));
    assert_eq!(
        wf.worker(&worker.id).await.unwrap().unwrap().status,
        WorkerStatus::Idle
    );
}

#[tokio::test]
async fn workforce_run_inherits_the_parent_security_scope() {
    let seen: Arc<std::sync::Mutex<Vec<Option<SecurityScope>>>> =
        Arc::new(std::sync::Mutex::new(Vec::new()));
    let seen_driver: Arc<std::sync::Mutex<Vec<Option<SecurityScope>>>> = Arc::clone(&seen);
    let driver: Arc<dyn AgentDriver> = Arc::new(FnDriver::new(move |ctx: AgentContext| {
        seen_driver
            .lock()
            .expect("record lock")
            .push(ctx.spec.security_scope.clone());
        AgentResult::succeeded(&ctx.spec.id, "scope ok", vec![])
    }));

    let store: Arc<dyn WorkforceStore> = Arc::new(InMemoryWorkforceStore::new());
    let wf = Workforce::new(
        store,
        Arc::clone(&driver),
        TempDir::new().path().to_path_buf(),
    )
    .with_security_scope(SecurityScope::new(&["web"], &["provider/openrouter"]));

    let worker = wf
        .register(AgentRole::Researcher, "reads docs")
        .await
        .unwrap();
    let task = wf
        .create_task("read", "find the documentation", AgentRole::Researcher)
        .await
        .unwrap();
    wf.assign(&worker.id, &task.id).await.unwrap();

    let outcome = wf.run(&worker.id).await.unwrap();
    let TaskOutcome::Done { result, .. } = outcome else {
        panic!("expected Done");
    };
    assert_eq!(result.summary, "scope ok");

    let recorded = seen.lock().expect("record lock");
    assert_eq!(recorded.len(), 1);
    let scope = recorded[0].clone().expect("security scope attached");
    assert_eq!(scope.egress, vec!["web".to_string()]);
    assert_eq!(scope.secrets, vec!["provider/openrouter".to_string()]);
}

/// Minimal RAII temp dir for tests.
struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn new() -> Self {
        let mut path = std::env::temp_dir();
        path.push(format!(
            "friday-workforce-test-{}",
            uuid::Uuid::new_v4().simple()
        ));
        std::fs::create_dir_all(&path).unwrap();
        Self { path }
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

// ── R2-2 F2: hard-crash orphan injection (qualification, no fix) ──────

/// Injects a **hard crash between the durable pre-dispatch checkpoint and
/// the terminal write**: the exact state `Workforce::run` persists before
/// dispatching the driver (worker `Running`, task `InProgress`) is written,
/// then the owning instance is dropped - no driver `Err`, no panic, no
/// terminal state. A fresh `Workforce` is reconstructed over the same
/// persistent root and every public recovery avenue is tried.
///
/// This test QUALIFIES the recovery behavior; it intentionally does not
/// repair it. Observed outcome recorded in QUALIFICATION-CAMPAIGN.md (F2).
#[tokio::test]
async fn f2_hard_crash_orphans_an_in_progress_task_and_no_public_api_recovers_it() {
    let fx = fixture().await;
    let root = TempDir::new();
    let store_root = root.path().join("store");
    let workspaces = root.path().join("workspaces");
    let shared: Arc<dyn WorkforceStore> = Arc::new(JsonWorkforceStore::new(&store_root));

    // Instance A: register + create + assign, then write the exact durable
    // checkpoint run() persists immediately before dispatch.
    let (worker_id, task_id) = {
        let wf = Workforce::new(Arc::clone(&shared), Arc::clone(&fx.driver), &workspaces);
        let worker = wf
            .register(AgentRole::Researcher, "crash-test worker")
            .await
            .unwrap();
        let task = wf
            .create_task("crash test", "will be orphaned", AgentRole::Researcher)
            .await
            .unwrap();
        wf.assign(&worker.id, &task.id).await.unwrap();

        // The crash window - exactly what run() saves before dispatch:
        let mut w = shared.load_worker(&worker.id).await.unwrap().unwrap();
        w.status = WorkerStatus::Running;
        shared.save_worker(&w).await.unwrap();
        let mut t = shared.load_task(&task.id).await.unwrap().unwrap();
        t.status = TaskStatus::InProgress;
        shared.save_task(&t).await.unwrap();
        (worker.id.clone(), task.id.clone())
        // Process disappears here. No Done/Failed/Idle write ever happens.
    };

    // Instance C ("restart"): a fresh Workforce over the same store.
    let wf = Workforce::new(Arc::clone(&shared), Arc::clone(&fx.driver), &workspaces);

    // Persistence itself is GREEN: the orphaned state survived exactly as left.
    let orphan = wf
        .task(&task_id)
        .await
        .unwrap()
        .expect("task survives restart");
    assert_eq!(orphan.status, TaskStatus::InProgress);
    assert_eq!(orphan.assignee.as_deref(), Some(worker_id.as_str()));
    let crashed = wf.worker(&worker_id).await.unwrap().unwrap();
    assert_eq!(crashed.status, WorkerStatus::Running);
    assert_eq!(crashed.current_task.as_deref(), Some(task_id.as_str()));

    // -- Every public recovery avenue, probed in turn --------------------

    // 1. A replacement worker cannot even see the task (pick_up finds Open only).
    let fresh = wf
        .register(AgentRole::Researcher, "replacement worker")
        .await
        .unwrap();
    assert!(
        wf.pick_up(&fresh.id).await.unwrap().is_none(),
        "pick_up sees only Open tasks: the orphan is invisible to the board"
    );

    // 2. Direct assignment is refused (task not Open).
    let err = wf.assign(&fresh.id, &task_id).await.unwrap_err();
    assert!(err.contains("in_progress"), "assign refused: {err}");

    // 3. Blocker resolution does not apply (task not Blocked).
    let err = wf.resolve_blocker(&task_id).await.unwrap_err();
    assert!(
        err.contains("not blocked"),
        "resolve_blocker refused: {err}"
    );

    // 4. The crashed worker cannot suspend / resume / retire its way out.
    let err = wf.suspend(&worker_id).await.unwrap_err();
    assert!(err.contains("not assigned"), "suspend refused: {err}");
    let err = wf.resume(&worker_id).await.unwrap_err();
    assert!(err.contains("not suspended"), "resume refused: {err}");
    let err = wf.retire(&worker_id).await.unwrap_err();
    assert!(err.contains("running"), "retire refused: {err}");

    // OBSERVED OUTCOME: the orphan remains InProgress forever. No stale-
    // ownership detection, no lease/heartbeat, no reaper exists. This is the
    // proven R2 GAP - recorded, not repaired here.
    let after = wf.task(&task_id).await.unwrap().unwrap();
    assert_eq!(
        after.status,
        TaskStatus::InProgress,
        "PROVEN GAP: an orphaned InProgress task has no automatic or manual \
         recovery path through the public API"
    );
}
