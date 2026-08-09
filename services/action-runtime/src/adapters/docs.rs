//! Documentation capability adapter: search + read local docs.
//!
//! `docs.search` walks a root directory and matches the query against file
//! names and leading content of doc-shaped files (`.md`, `.txt`, READMEs,
//! `.rs`). `docs.read` returns a bounded read of one file. Both are read-only
//! and classified `Risk::Safe` — this is the \"find local documentation\" muscle
//! behind the `docs` skill (self-hosted knowledge: READMEs, ADRs, specs).
//!
//! **Trust note:** `root`/`path` are host-supplied params (the planner/spec
//! decides what to read) and are used verbatim — the adapter has no sandbox.
//! That is the same trust model as `filesystem.read_file`: capability
//! permissions are the boundary. A deployment needing confinement should
//! layer allowed-roots in the policy layer (the MCP filesystem server pattern).

use async_trait::async_trait;
use serde_json::json;
use std::path::{Path, PathBuf};

use crate::adapters::ActionAdapter;
use crate::request::{ActionRequest, ActionResult};

/// Directories never walked (build artifacts, vendored code, VCS metadata).
const SKIP_DIRS: &[&str] = &[
    "target",
    "node_modules",
    ".git",
    "dist",
    "build",
    ".cache",
    ".venv",
    "vendor",
];

/// Maximum number of files scanned in one search (defense in depth).
const MAX_SCAN_FILES: usize = 4_000;

/// Extension + filename heuristic for \"documentation-shaped\" files.
fn is_doc_file(path: &Path) -> bool {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    if name == "readme" || name == "readme.md" || name == "readme.txt" {
        return true;
    }
    matches!(
        path.extension()
            .and_then(|e| e.to_str())
            .map(str::to_lowercase)
            .as_deref(),
        Some("md") | Some("txt") | Some("rst") | Some("adoc")
    )
}

pub struct DocsAdapter;

impl Default for DocsAdapter {
    fn default() -> Self {
        Self::new()
    }
}

impl DocsAdapter {
    pub fn new() -> Self {
        Self
    }

    fn all_actions() -> &'static [&'static str] {
        &["search", "read"]
    }

    fn search(&self, req: &ActionRequest) -> ActionResult {
        let root = match req.param_str("root") {
            Ok(p) => p,
            Err(e) => return ActionResult::error(e),
        };
        let query = req
            .parameters
            .get("query")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_lowercase();
        let limit = req
            .parameters
            .get("limit")
            .and_then(|v| v.as_u64())
            .unwrap_or(8)
            .min(50) as usize;
        let root_path = Path::new(&root);
        if !root_path.is_dir() {
            return ActionResult::error(format!("root is not a directory: {root}"));
        }

        let mut hits = Vec::new();
        let mut scanned = 0usize;
        walk(root_path, &mut |path: &PathBuf| {
            scanned += 1;
            if scanned > MAX_SCAN_FILES || hits.len() >= limit {
                return false;
            }
            if !is_doc_file(path) {
                return true;
            }
            let relative = path
                .strip_prefix(root_path)
                .unwrap_or(path)
                .to_string_lossy()
                .to_string();
            let name_lower = path
                .file_name()
                .map(|n| n.to_string_lossy().to_lowercase())
                .unwrap_or_default();
            let snippet = if query.is_empty() || name_lower.contains(&query) {
                // Filename match — no need to read the file.
                String::new()
            } else {
                first_snippet(path, &query)
            };
            let matched = query.is_empty() || name_lower.contains(&query) || !snippet.is_empty();
            if matched {
                hits.push(json!({ "path": relative, "snippet": snippet }));
            }
            true
        });
        ActionResult::success(json!({
            "root": root,
            "query": query,
            "scanned": scanned,
            "hits": hits.into_iter().take(limit).collect::<Vec<_>>(),
        }))
    }

    fn read(&self, req: &ActionRequest) -> ActionResult {
        let path = match req.param_str("path") {
            Ok(p) => p,
            Err(e) => return ActionResult::error(e),
        };
        let max_chars = req
            .parameters
            .get("max_chars")
            .and_then(|v| v.as_u64())
            .unwrap_or(40_000)
            .min(200_000) as usize;
        let p = Path::new(&path);
        if !p.is_file() {
            return ActionResult::error(format!("not a file: {path}"));
        }
        match std::fs::read_to_string(p) {
            Ok(content) => {
                let truncated: String = content.chars().take(max_chars).collect();
                ActionResult::success(json!({
                    "path": path,
                    "content": truncated,
                    "chars": truncated.len(),
                    "truncated": truncated.len() < content.len(),
                }))
            }
            Err(e) => ActionResult::error(format!("read failed: {e}")),
        }
    }
}

/// Depth-first walk over a directory tree, skipping `SKIP_DIRS`. `visit` may
/// stop the walk early by returning `false`.
fn walk(root: &Path, visit: &mut impl FnMut(&PathBuf) -> bool) {
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in rd.flatten() {
            let path = entry.path();
            if path.is_dir() {
                let name = entry.file_name().to_string_lossy().to_string();
                if SKIP_DIRS.contains(&name.as_str()) {
                    continue;
                }
                stack.push(path);
            } else if !visit(&path) {
                return;
            }
        }
    }
}

/// Read the first few lines of a doc file and return the first line (or few)
/// containing the query, trimmed to a snippet.
fn first_snippet(path: &Path, query: &str) -> String {
    let Ok(content) = std::fs::read_to_string(path) else {
        return String::new();
    };
    for line in content.lines().take(80) {
        let lower = line.to_lowercase();
        if lower.contains(query) {
            return line.trim().chars().take(200).collect();
        }
    }
    String::new()
}

#[async_trait]
impl ActionAdapter for DocsAdapter {
    fn tool(&self) -> &str {
        "docs"
    }

    fn actions(&self) -> Vec<&str> {
        Self::all_actions().to_vec()
    }

    async fn execute(&self, req: ActionRequest) -> ActionResult {
        match req.action.as_str() {
            "search" => self.search(&req),
            "read" => self.read(&req),
            other => ActionResult::error(format!("unknown docs action: {other}")),
        }
    }
}

/// Capability record published to the Capability Intelligence registry.
pub fn capabilities() -> Vec<friday_capabilities::model::Capability> {
    friday_capabilities::builtins::docs()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_tree() -> tempfile_lite::TempDir {
        tempfile_lite::TempDir::new().expect("temp dir")
    }

    #[tokio::test]
    async fn search_finds_doc_files_by_name_and_content() {
        let dir = temp_tree();
        let root = dir.path();
        std::fs::create_dir_all(root.join("docs")).unwrap();
        std::fs::create_dir_all(root.join("target")).unwrap(); // must be skipped
        std::fs::write(root.join("README.md"), "# FRIDAY\nRust agent platform").unwrap();
        std::fs::write(root.join("docs/adr-0001.md"), "# ADR-0001\nCore freeze").unwrap();
        std::fs::write(root.join("target/junk.md"), "should not appear").unwrap();

        let adapter = DocsAdapter::new();
        let req = ActionRequest::new("docs", "search", json!({ "root": root.to_string_lossy() }));
        let res = adapter.execute(req).await;
        assert!(res.success, "search failed: {:?}", res.error);
        let hits = res.output["hits"].as_array().unwrap().clone();
        let paths: Vec<&str> = hits.iter().filter_map(|h| h["path"].as_str()).collect();
        assert!(paths.contains(&"README.md"), "paths: {paths:?}");
        assert!(
            paths.iter().any(|p| p.ends_with("adr-0001.md")),
            "paths: {paths:?}"
        );
        assert!(!paths.contains(&"target/junk.md"), "target must be skipped");
    }

    #[tokio::test]
    async fn search_matches_query_in_content() {
        let dir = temp_tree();
        let root = dir.path();
        std::fs::write(root.join("guide.md"), "## Setup\nInstall the SDK first.").unwrap();
        std::fs::write(root.join("other.md"), "nothing related at all").unwrap();

        let adapter = DocsAdapter::new();
        let req = ActionRequest::new(
            "docs",
            "search",
            json!({ "root": root.to_string_lossy(), "query": "sdk" }),
        );
        let res = adapter.execute(req).await;
        assert!(res.success);
        let hits = res.output["hits"].as_array().unwrap();
        assert_eq!(hits.len(), 1, "only guide.md mentions sdk");
        assert_eq!(hits[0]["path"], "guide.md");
    }

    #[tokio::test]
    async fn read_truncates_and_rejects_dirs() {
        let dir = temp_tree();
        let root = dir.path();
        let content = "x".repeat(100_000);
        std::fs::write(root.join("big.md"), &content).unwrap();

        let adapter = DocsAdapter::new();
        let req = ActionRequest::new(
            "docs",
            "read",
            json!({ "path": root.join("big.md").to_string_lossy(), "max_chars": 100 }),
        );
        let res = adapter.execute(req).await;
        assert!(res.success);
        assert_eq!(res.output["chars"], 100);
        assert_eq!(res.output["truncated"], true);

        let dir_req = ActionRequest::new("docs", "read", json!({ "path": root.to_string_lossy() }));
        let res = adapter.execute(dir_req).await;
        assert!(!res.success);
    }
}

/// Minimal RAII temp dir for tests (avoids adding a dev-dependency).
#[cfg(test)]
mod tempfile_lite {
    use std::path::PathBuf;

    pub struct TempDir {
        path: PathBuf,
    }

    impl TempDir {
        pub fn new() -> std::io::Result<Self> {
            let mut path = std::env::temp_dir();
            path.push(format!(
                "friday-docs-test-{}",
                uuid::Uuid::new_v4().simple()
            ));
            std::fs::create_dir_all(&path)?;
            Ok(Self { path })
        }

        pub fn path(&self) -> &std::path::Path {
            &self.path
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }
}
