//! Unit tests for the filesystem adapter's individual actions.

use friday_actions::adapters::FilesystemAdapter;
use friday_actions::request::{ActionRequest, ActionResult};
use friday_actions::ActionAdapter;
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

#[tokio::test]
async fn tree_returns_the_nested_structure() {
    let tmp = Tmp::new();
    let a = FilesystemAdapter::new();
    std::fs::create_dir(tmp.dir.join("sub")).unwrap();
    std::fs::create_dir(tmp.dir.join("sub").join("deep")).unwrap();
    std::fs::write(tmp.dir.join("sub").join("deep").join("x.txt"), "").unwrap();
    std::fs::write(tmp.dir.join("root.txt"), "").unwrap();

    let r = run(&a, "tree", json!({ "path": tmp.dir })).await;
    assert!(r.success, "tree failed: {:?}", r.error);
    let tree = r.output.get("tree").and_then(|v| v.as_array()).unwrap();
    assert_eq!(tree.len(), 2, "root has 2 entries: {tree:?}");
    let sub = tree.iter().find(|e| e["name"] == "sub").unwrap();
    assert_eq!(sub["is_dir"], json!(true));
    let children = sub["children"].as_array().unwrap();
    assert_eq!(children.len(), 1);
    assert_eq!(children[0]["name"], "deep");
    assert_eq!(children[0]["children"][0]["name"], "x.txt");
    assert_eq!(children[0]["children"][0]["is_dir"], json!(false));
}

#[tokio::test]
async fn tree_respects_max_depth() {
    let tmp = Tmp::new();
    let a = FilesystemAdapter::new();
    std::fs::create_dir_all(tmp.dir.join("a/b/c")).unwrap();
    std::fs::write(tmp.dir.join("a/b/c/f.txt"), "").unwrap();

    let r = run(&a, "tree", json!({ "path": tmp.dir, "max_depth": 1 })).await;
    assert!(r.success);
    // max_depth=1: `a` (depth 0) lists its child `b` (depth 1), but `b`'s
    // subtree (depth 2) is cut.
    let tree = r.output.get("tree").and_then(|v| v.as_array()).unwrap();
    let a = tree.iter().find(|e| e["name"] == "a").unwrap();
    let b = a["children"].as_array().unwrap();
    assert_eq!(b.len(), 1);
    assert_eq!(b[0]["name"], "b");
    assert_eq!(b[0]["children"].as_array().unwrap().len(), 0);
}

#[tokio::test]
async fn search_finds_files_by_name_and_extension() {
    let tmp = Tmp::new();
    let a = FilesystemAdapter::new();
    std::fs::create_dir_all(tmp.dir.join("src")).unwrap();
    std::fs::write(tmp.dir.join("src").join("lib.rs"), "").unwrap();
    std::fs::write(tmp.dir.join("src").join("main.rs"), "").unwrap();
    std::fs::write(tmp.dir.join("src").join("notes.txt"), "").unwrap();
    std::fs::write(tmp.dir.join("README.md"), "").unwrap();

    let r = run(&a, "search", json!({ "path": tmp.dir, "ext": "rs" })).await;
    assert!(r.success, "search failed: {:?}", r.error);
    let matches = r.output.get("matches").and_then(|v| v.as_array()).unwrap();
    assert_eq!(matches.len(), 2, "both .rs files: {matches:?}");

    let r = run(&a, "search", json!({ "path": tmp.dir, "query": "readme" })).await;
    assert!(r.success);
    let matches = r.output.get("matches").and_then(|v| v.as_array()).unwrap();
    assert_eq!(matches.len(), 1);
    assert_eq!(matches[0]["name"], "README.md");

    let r = run(
        &a,
        "search",
        json!({ "path": tmp.dir, "query": "nonexistent" }),
    )
    .await;
    assert!(r.success);
    let matches = r.output.get("matches").and_then(|v| v.as_array()).unwrap();
    assert!(matches.is_empty());
}

#[tokio::test]
async fn search_finds_files_by_content() {
    let tmp = Tmp::new();
    let a = FilesystemAdapter::new();
    std::fs::create_dir_all(tmp.dir.join("src")).unwrap();
    std::fs::write(
        tmp.dir.join("src").join("lib.rs"),
        "pub fn ExpertRouter() {}",
    )
    .unwrap();
    std::fs::write(tmp.dir.join("src").join("main.rs"), "fn main() {}").unwrap();
    std::fs::write(
        tmp.dir.join("src").join("notes.txt"),
        "mentions ExpertRouter here",
    )
    .unwrap();

    // Content + extension: only the .rs file whose CONTENTS carry the needle —
    // the symbol lives in the file body, not the filename.
    let r = run(
        &a,
        "search",
        json!({ "path": tmp.dir, "ext": "rs", "content": "ExpertRouter" }),
    )
    .await;
    assert!(r.success, "search failed: {:?}", r.error);
    let matches = r.output.get("matches").and_then(|v| v.as_array()).unwrap();
    assert_eq!(matches.len(), 1, "only lib.rs contains it: {matches:?}");
    assert_eq!(matches[0]["name"], "lib.rs");
    assert_eq!(r.output["content"], "expertrouter");

    // Content search is case-insensitive and never matches directories.
    let r = run(
        &a,
        "search",
        json!({ "path": tmp.dir, "content": "expertrouter" }),
    )
    .await;
    assert!(r.success);
    let matches = r.output.get("matches").and_then(|v| v.as_array()).unwrap();
    assert_eq!(
        matches.len(),
        2,
        "both files carrying the text: {matches:?}"
    );

    // A needle absent from all contents returns nothing.
    let r = run(
        &a,
        "search",
        json!({ "path": tmp.dir, "content": "nonexistent-symbol" }),
    )
    .await;
    assert!(r.success);
    let matches = r.output.get("matches").and_then(|v| v.as_array()).unwrap();
    assert!(matches.is_empty());
}
