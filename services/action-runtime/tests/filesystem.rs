//! Unit tests for the filesystem adapter's individual actions.

use friday_actions::adapters::FilesystemAdapter;
use friday_actions::ActionAdapter;
use friday_actions::request::{ActionRequest, ActionResult};
use serde_json::json;
use std::path::PathBuf;
use uuid::Uuid;

/// A unique temporary directory that cleans itself up on drop.
struct Tmp {
    dir: PathBuf,
}

impl Tmp {
    fn new() -> Self {
        let dir = std::env::temp_dir().join(format!("friday-test-{}", Uuid::new_v4().simple()));
        std::fs::create_dir_all(&dir).unwrap();
        Self { dir }
    }
}

impl Drop for Tmp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

async fn run(adapter: &FilesystemAdapter, action: &str, params: serde_json::Value) -> ActionResult {
    adapter
        .execute(ActionRequest::new("filesystem", action, params))
        .await
}

#[tokio::test]
async fn create_and_read_roundtrip() {
    let tmp = Tmp::new();
    let path = tmp.dir.join("a").join("b.txt");
    let p = path.to_string_lossy().to_string();

    let r = run(
        &FilesystemAdapter::new(),
        "create_file",
        json!({ "path": p, "content": "hello" }),
    )
    .await;
    assert!(r.success, "create failed: {:?}", r.error);

    let r = run(&FilesystemAdapter::new(), "read_file", json!({ "path": p })).await;
    assert!(r.success);
    assert_eq!(
        r.output.get("content").and_then(|v| v.as_str()),
        Some("hello")
    );
}

#[tokio::test]
async fn write_overwrites_existing() {
    let tmp = Tmp::new();
    let p = tmp.dir.join("f.txt").to_string_lossy().to_string();
    let a = FilesystemAdapter::new();

    assert!(
        run(&a, "write_file", json!({ "path": p, "content": "one" }))
            .await
            .success
    );
    let r = run(&a, "write_file", json!({ "path": p, "content": "two" })).await;
    assert!(r.success);
    let r = run(&a, "read_file", json!({ "path": p })).await;
    assert_eq!(
        r.output.get("content").and_then(|v| v.as_str()),
        Some("two")
    );
}

#[tokio::test]
async fn list_directory_returns_entries() {
    let tmp = Tmp::new();
    let p = tmp.dir.to_string_lossy().to_string();
    let a = FilesystemAdapter::new();
    run(
        &a,
        "create_file",
        json!({ "path": tmp.dir.join("x.txt"), "content": "" }),
    )
    .await;
    std::fs::create_dir(tmp.dir.join("sub")).unwrap();

    let r = run(&a, "list_directory", json!({ "path": p })).await;
    assert!(r.success);
    let entries = r.output.get("entries").and_then(|v| v.as_array()).unwrap();
    assert_eq!(entries.len(), 2);
}

#[tokio::test]
async fn copy_and_move() {
    let tmp = Tmp::new();
    let a = FilesystemAdapter::new();
    let src = tmp.dir.join("src.txt").to_string_lossy().to_string();
    let dst = tmp.dir.join("dst.txt").to_string_lossy().to_string();
    run(&a, "write_file", json!({ "path": src, "content": "data" })).await;

    assert!(
        run(&a, "copy", json!({ "from": src, "to": dst }))
            .await
            .success
    );
    let moved = tmp.dir.join("moved.txt").to_string_lossy().to_string();
    let r = run(&a, "move", json!({ "from": dst, "to": moved })).await;
    assert!(r.success);
    assert!(!std::path::Path::new(&dst).exists());
    assert!(std::path::Path::new(&moved).exists());
}

#[tokio::test]
async fn unknown_action_errors() {
    let r = run(&FilesystemAdapter::new(), "frobnicate", json!({})).await;
    assert!(!r.success);
}
