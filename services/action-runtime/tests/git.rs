//! Unit tests for the Git capability adapter.
//!
//! These exercise real local git operations (init/status/add/commit/branch/
//! diff). They skip silently when `git` is not installed, so the suite stays
//! green on machines without Git while still validating the adapter's mapping
//! on machines that have it.

use friday_actions::adapters::GitAdapter;
use friday_actions::request::{ActionRequest, ActionResult};
use friday_actions::ActionAdapter;
use serde_json::json;
use uuid::Uuid;

fn git_available() -> bool {
    std::process::Command::new("git")
        .args(["--version"])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

async fn run(action: &str, params: serde_json::Value) -> ActionResult {
    GitAdapter::new()
        .execute(ActionRequest::new("git", action, params))
        .await
}

#[tokio::test]
async fn init_and_status_clean() {
    if !git_available() {
        return;
    }
    let dir = std::env::temp_dir().join(format!("friday-git-{}", Uuid::new_v4().simple()));
    std::fs::create_dir_all(&dir).unwrap();

    let r = run("init", json!({ "path": dir })).await;
    assert!(r.success, "init failed: {:?}", r.error);

    let r = run("status", json!({ "repo": dir })).await;
    assert!(r.success);
    // A fresh repo reports no changes in porcelain output.
    let out = r
        .output
        .get("stdout")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    assert!(out.trim().is_empty(), "expected clean status, got: {out:?}");

    let _ = dir;
}

#[tokio::test]
async fn add_commit_and_branch() {
    if !git_available() {
        return;
    }
    let dir = std::env::temp_dir().join(format!("friday-git-{}", Uuid::new_v4().simple()));
    std::fs::create_dir_all(&dir).unwrap();

    run("init", json!({ "path": dir.clone() })).await;
    std::fs::write(dir.join("hello.txt"), "world").unwrap();

    let r = run("add", json!({ "repo": dir, "paths": ["hello.txt"] })).await;
    assert!(r.success, "add failed: {:?}", r.error);

    let r = run("commit", json!({ "repo": dir, "message": "first commit" })).await;
    assert!(r.success, "commit failed: {:?}", r.error);

    let r = run("branch", json!({ "repo": dir, "name": "feature" })).await;
    assert!(r.success, "branch failed: {:?}", r.error);

    // Listing branches should now include the new one.
    let r = run("branch", json!({ "repo": dir })).await;
    assert!(r.success);
    let out = r
        .output
        .get("stdout")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    assert!(
        out.contains("feature"),
        "expected feature branch, got: {out:?}"
    );

    let _ = dir;
}

#[tokio::test]
async fn unknown_action_errors() {
    let r = GitAdapter::new()
        .execute(ActionRequest::new("git", "frobnicate", json!({})))
        .await;
    assert!(!r.success);
}
