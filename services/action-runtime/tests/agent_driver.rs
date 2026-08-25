//! Hermetic tests for [`SkillAgentDriver`]: a worker goal is planned by the
//! skill engine scoped to the agent's allowed skills, then executed through
//! the real kernel + adapters. No LLM, no network, no bus — fully
//! deterministic proof of the "planner orchestrates, per-agent allowed skills"
//! contract.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use friday_actions::{AgentKernel, CodeIntelAdapter, DocsAdapter, GitAdapter, SkillAgentDriver};
use friday_agent_runtime::{AgentRuntime, AgentSpec};
use friday_planner::execution::capability_registry::CapabilityRegistry as PlannerCapabilityRegistry;
use friday_planner::execution::native_caps::register_native_builtins;
use friday_skills::{CapabilityResolver, SkillApprovals, SkillLoader, SkillRegistry, SkillSource};

/// Resolves capability ids against the real planner registry (the same view
/// `friday-context`'s `PlannerRegistryResolver` provides).
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

fn git_manifest() -> &'static str {
    r#"{
        "id": "git", "name": "Git", "version": "1.0.0",
        "description": "Version control with git",
        "category": "developer",
        "capabilities": ["git.status"],
        "actions": ["check git status", "view git status"],
        "permissions": ["git.status"],
        "readonly": false,
        "platform": "Any", "min_core_version": "0.0.0", "enabled": true
    }"#
}

struct Fixture {
    /// Held only to keep the temp dir alive for the fixture's lifetime.
    #[allow(dead_code)]
    skills_dir: TempDir,
    planner: Arc<PlannerCapabilityRegistry>,
    skills: Arc<SkillRegistry>,
    approvals: Arc<SkillApprovals>,
    kernel: Arc<AgentKernel>,
}

async fn fixture() -> Fixture {
    let skills_dir = TempDir::new();
    write_skill(skills_dir.path(), "docs", docs_manifest());
    write_skill(skills_dir.path(), "git", git_manifest());

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
    kernel
        .register_adapter(Box::new(CodeIntelAdapter::new()))
        .await;
    kernel.register_adapter(Box::new(GitAdapter::new())).await;

    Fixture {
        skills_dir,
        planner,
        skills,
        approvals: Arc::new(SkillApprovals::new()),
        kernel,
    }
}

fn driver(fx: &Fixture) -> SkillAgentDriver {
    SkillAgentDriver::new(
        Arc::clone(&fx.skills),
        Arc::clone(&fx.approvals),
        Arc::clone(&fx.planner),
        Arc::clone(&fx.kernel),
    )
}

#[tokio::test]
async fn readonly_agent_runs_its_auto_approved_capabilities() {
    let fx = fixture().await;
    let docs_root = TempDir::new();
    std::fs::write(docs_root.path().join("README.md"), "# FRIDAY docs").unwrap();

    let runtime = AgentRuntime::new();
    let spec = AgentSpec::new("research", "find the documentation")
        .with_allowed_skills(&["docs"])
        .with_param(
            "docs.search",
            serde_json::json!({ "root": docs_root.path().to_string_lossy() }),
        );
    let handle = runtime.spawn(spec, Arc::new(driver(&fx)));
    let result = handle.await_result().await;

    assert!(result.state.is_success(), "{result:?}");
    assert!(result.summary.contains("'docs'"), "{}", result.summary);
    let steps = &result.steps;
    assert!(
        steps
            .iter()
            .any(|s| s.description == "docs.search" && s.outcome == "ok"),
        "docs.search must have executed: {steps:?}"
    );
    assert!(
        steps.iter().all(|s| s.description.starts_with("docs.")),
        "a scoped agent must never touch non-docs capabilities: {steps:?}"
    );
}

#[tokio::test]
async fn approved_skill_runs_and_out_of_scope_skills_are_unreachable() {
    let fx = fixture().await;
    // A real git repo so the GitAdapter has something to inspect.
    let repo = TempDir::new();
    std::process::Command::new("git")
        .args(["init"])
        .current_dir(repo.path())
        .output()
        .expect("git init");
    std::fs::write(repo.path().join("hello.txt"), "hi").unwrap();

    fx.approvals.approve("git");

    let runtime = AgentRuntime::new();
    // The goal talks about *documentation*, but the agent's scope is [git] —
    // the driver must pick git (the only candidate) and never reach docs.
    let spec = AgentSpec::new("inspect", "find the documentation and check the git status")
        .with_allowed_skills(&["git"])
        .with_param(
            "git.status",
            serde_json::json!({ "path": repo.path().to_string_lossy() }),
        );
    let handle = runtime.spawn(spec, Arc::new(driver(&fx)));
    let result = handle.await_result().await;

    assert!(result.state.is_success(), "{result:?}");
    assert!(result.summary.contains("'git'"), "{}", result.summary);
    assert!(
        result
            .steps
            .iter()
            .any(|s| s.description == "git.status" && s.outcome == "ok"),
        "git.status must have executed: {:?}",
        result.steps
    );
    assert!(
        result
            .steps
            .iter()
            .all(|s| s.description.starts_with("git.")),
        "out-of-scope docs capabilities must be unreachable: {:?}",
        result.steps
    );
}

#[tokio::test]
async fn unapproved_non_readonly_agent_fails_with_no_capability_executed() {
    let fx = fixture().await;
    // git is not readonly and not approved → nothing may run; the agent must
    // fail honestly instead of silently executing.
    let runtime = AgentRuntime::new();
    let spec = AgentSpec::new("inspect", "check the git status").with_allowed_skills(&["git"]);
    let handle = runtime.spawn(spec, Arc::new(driver(&fx)));
    let result = handle.await_result().await;

    assert_eq!(result.state, friday_agent_runtime::AgentState::Failed);
    assert_eq!(result.error.as_deref(), Some("no_capability_executed"));
    assert!(result
        .steps
        .iter()
        .all(|s| s.outcome == "requires-approval"));
}

#[tokio::test]
async fn empty_scope_finds_no_selectable_skill() {
    let fx = fixture().await;
    let runtime = AgentRuntime::new();
    let spec = AgentSpec::new("lonely", "find the documentation").with_allowed_skills(&["web"]);
    let handle = runtime.spawn(spec, Arc::new(driver(&fx)));
    let result = handle.await_result().await;

    assert_eq!(result.state, friday_agent_runtime::AgentState::Failed);
    assert_eq!(result.error.as_deref(), Some("no_selectable_skill"));
}

/// Minimal RAII temp dir for tests.
struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn new() -> Self {
        let mut path = std::env::temp_dir();
        path.push(format!(
            "friday-agent-driver-test-{}",
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
