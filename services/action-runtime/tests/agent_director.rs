//! Hermetic acceptance tests for the [`AgentDirector`]: the supervisor runs
//! **real** skill-scoped workers through `SkillAgentDriver` + the real kernel
//! (no LLM, no network, no bus), and the goal only completes when every
//! role-typed result passes the director's gate. This is the test that turns
//! the topology from a routing abstraction into an actual FRIDAY capability.
//!
//! Topology under test: Director → Researcher (docs) / Verifier (git) →
//! `verify()` → `DirectorReport::all_verified()`.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use friday_actions::{AgentKernel, DocsAdapter, GitAdapter, SkillAgentDriver};
use friday_agent_runtime::{default_gate, AgentDirector, AgentDriver, AgentRole, AgentSpec};
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
    // Kept alive only so the temp skills dir survives the fixture's lifetime.
    _skills_dir: TempDir,
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
    kernel.register_adapter(Box::new(GitAdapter::new())).await;

    Fixture {
        _skills_dir: skills_dir,
        planner,
        skills,
        approvals: Arc::new(SkillApprovals::new()),
        kernel,
    }
}

fn driver(fx: &Fixture) -> Arc<dyn AgentDriver> {
    Arc::new(SkillAgentDriver::new(
        Arc::clone(&fx.skills),
        Arc::clone(&fx.approvals),
        Arc::clone(&fx.planner),
        Arc::clone(&fx.kernel),
    ))
}

/// A real git repo so the GitAdapter has something to inspect.
fn git_repo() -> TempDir {
    let repo = TempDir::new();
    std::process::Command::new("git")
        .args(["init"])
        .current_dir(repo.path())
        .output()
        .expect("git init");
    std::fs::write(repo.path().join("hello.txt"), "hi").unwrap();
    repo
}

#[tokio::test]
async fn director_collects_verified_results_from_real_skill_workers() {
    let fx = fixture().await;
    let docs_root = TempDir::new();
    std::fs::write(docs_root.path().join("README.md"), "# FRIDAY docs").unwrap();
    let repo = git_repo();
    fx.approvals.approve("git");

    let director = AgentDirector::new(driver(&fx));

    // Researcher: gathers documentation through the real docs adapter.
    let researcher_spec = AgentSpec::new("researcher.1", "find the documentation")
        .with_allowed_skills(&["docs"])
        .with_param(
            "docs.search",
            serde_json::json!({ "root": docs_root.path().to_string_lossy() }),
        );
    let researcher = director.spawn_spec(AgentRole::Researcher, researcher_spec);

    // Verifier: proves the workspace state through a real git.status check.
    let verifier_spec = AgentSpec::new("verifier.1", "check the git status")
        .with_allowed_skills(&["git"])
        .with_param(
            "git.status",
            serde_json::json!({ "path": repo.path().to_string_lossy() }),
        );
    let verifier = director.spawn_spec(AgentRole::Verifier, verifier_spec);

    let report = director
        .verify(vec![researcher, verifier], default_gate())
        .await;

    assert!(
        report.all_verified(),
        "real skill workers must all pass the gate: {}",
        report.summary_lines().join("\n")
    );
    assert_eq!(report.verified(), 2);

    // Both children ran real, role-appropriate capabilities only.
    let researcher_entry = report
        .entries
        .iter()
        .find(|e| e.role == AgentRole::Researcher)
        .unwrap();
    assert!(
        researcher_entry
            .result
            .steps
            .iter()
            .any(|s| s.description == "docs.search" && s.outcome == "ok"),
        "{:?}",
        researcher_entry.result.steps
    );

    let verifier_entry = report
        .entries
        .iter()
        .find(|e| e.role == AgentRole::Verifier)
        .unwrap();
    assert!(
        verifier_entry
            .result
            .steps
            .iter()
            .any(|s| s.description == "git.status" && s.outcome == "ok"),
        "a verifier must have run at least one real check: {:?}",
        verifier_entry.result.steps
    );
}

#[tokio::test]
async fn unverifiable_child_blocks_the_director_run() {
    let fx = fixture().await;
    let docs_root = TempDir::new();
    std::fs::write(docs_root.path().join("README.md"), "# FRIDAY docs").unwrap();
    let repo = git_repo();
    // NOTE: git is NOT approved here — the verifier must fail honestly
    // instead of running an unapproved non-readonly capability.

    let director = AgentDirector::new(driver(&fx));

    let researcher_spec = AgentSpec::new("researcher.1", "find the documentation")
        .with_allowed_skills(&["docs"])
        .with_param(
            "docs.search",
            serde_json::json!({ "root": docs_root.path().to_string_lossy() }),
        );
    let researcher = director.spawn_spec(AgentRole::Researcher, researcher_spec);

    let verifier_spec = AgentSpec::new("verifier.1", "check the git status")
        .with_allowed_skills(&["git"])
        .with_param(
            "git.status",
            serde_json::json!({ "path": repo.path().to_string_lossy() }),
        );
    let verifier = director.spawn_spec(AgentRole::Verifier, verifier_spec);

    let report = director
        .verify(vec![researcher, verifier], default_gate())
        .await;

    assert_eq!(report.verified(), 1, "researcher verified, verifier not");
    assert!(
        !report.all_verified(),
        "an unverifiable child must block the run: {}",
        report.summary_lines().join("\n")
    );
    let verifier_entry = report
        .entries
        .iter()
        .find(|e| e.role == AgentRole::Verifier)
        .unwrap();
    assert!(!verifier_entry.verified);
    assert!(
        verifier_entry
            .verdict
            .as_deref()
            .unwrap_or_default()
            .contains("ended failed"),
        "{:?}",
        verifier_entry.verdict
    );
    assert!(
        verifier_entry
            .result
            .steps
            .iter()
            .all(|s| s.outcome == "requires-approval"),
        "the unapproved verifier must never have executed: {:?}",
        verifier_entry.result.steps
    );
}

/// Minimal RAII temp dir for tests.
struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn new() -> Self {
        let mut path = std::env::temp_dir();
        path.push(format!(
            "friday-agent-director-test-{}",
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
