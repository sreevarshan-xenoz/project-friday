//! Code intelligence capability adapter: understand the current repository.
//!
//! `codeintel.understand` walks a root directory (bounded, skipping build
//! artifacts) and returns a deterministic overview: total file count, files by
//! extension, top-level directory sizes, and — when a `query` is given — file
//! names containing it. This is the \"know this repo\" muscle behind the
//! `codeintel` skill: it answers structure questions without an LLM reading
//! the whole tree. Read-only, classified `Risk::Safe`.
//!
//! **Trust note:** `root` is a host-supplied param used verbatim (no sandbox),
//! matching the platform's `filesystem` trust model — capability permissions
//! are the boundary.

use async_trait::async_trait;
use serde_json::json;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::adapters::ActionAdapter;
use crate::request::{ActionRequest, ActionResult};

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

/// Maximum files counted in one understand pass.
const MAX_SCAN_FILES: usize = 10_000;

/// Maximum depth walked (protects against pathological nesting).
const MAX_DEPTH: usize = 12;

pub struct CodeIntelAdapter;

impl Default for CodeIntelAdapter {
    fn default() -> Self {
        Self::new()
    }
}

impl CodeIntelAdapter {
    pub fn new() -> Self {
        Self
    }

    fn all_actions() -> &'static [&'static str] {
        &["understand"]
    }

    fn understand(&self, req: &ActionRequest) -> ActionResult {
        let root = match req.param_str("root") {
            Ok(p) => p,
            Err(e) => return ActionResult::error(e),
        };
        let query = req
            .parameters
            .get("query")
            .and_then(|v| v.as_str())
            .map(|s| s.to_lowercase())
            .unwrap_or_default();
        let root_path = Path::new(&root);
        if !root_path.is_dir() {
            return ActionResult::error(format!("root is not a directory: {root}"));
        }

        let mut total_files = 0usize;
        let mut by_ext: BTreeMap<String, usize> = BTreeMap::new();
        let mut by_top_dir: BTreeMap<String, usize> = BTreeMap::new();
        let mut query_matches: Vec<String> = Vec::new();
        let mut scanned = 0usize;

        walk_depth(root_path, 0, &mut |path: &PathBuf| {
            scanned += 1;
            if scanned > MAX_SCAN_FILES {
                return false;
            }
            total_files += 1;
            let ext = path
                .extension()
                .and_then(|e| e.to_str())
                .map(|e| e.to_lowercase())
                .unwrap_or_else(|| "(none)".to_string());
            *by_ext.entry(ext).or_insert(0) += 1;
            if let Ok(rel) = path.strip_prefix(root_path) {
                let top = rel
                    .components()
                    .next()
                    .map(|c| c.as_os_str().to_string_lossy().to_string())
                    .unwrap_or_else(|| ".".to_string());
                *by_top_dir.entry(top).or_insert(0) += 1;
                let name_lower = path
                    .file_name()
                    .map(|n| n.to_string_lossy().to_lowercase())
                    .unwrap_or_default();
                if !query.is_empty() && name_lower.contains(&query) {
                    query_matches.push(rel.to_string_lossy().to_string());
                }
            }
            true
        });

        let top_exts: Vec<serde_json::Value> = by_ext
            .iter()
            .rev() // BTreeMap rev: not by count — sort below instead
            .map(|(ext, count)| json!({ "extension": ext, "files": count }))
            .collect();
        let mut top_exts = top_exts;
        top_exts.sort_by(|a, b| b["files"].as_u64().cmp(&a["files"].as_u64()));
        top_exts.truncate(10);

        let top_dirs: Vec<serde_json::Value> = by_top_dir
            .iter()
            .map(|(dir, count)| json!({ "dir": dir, "files": count }))
            .collect();

        let mut lines = vec![
            format!("Repository at {root}: {total_files} files scanned"),
            format!(
                "Top directories: {}",
                by_top_dir
                    .iter()
                    .take(6)
                    .map(|(d, c)| format!("{d}({c})"))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            format!(
                "Top extensions: {}",
                top_exts
                    .iter()
                    .take(5)
                    .map(|e| format!("{}({})", e["extension"], e["files"]))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        ];
        if !query.is_empty() {
            lines.push(format!("Files matching '{query}': {}", query_matches.len()));
            for m in query_matches.iter().take(10) {
                lines.push(format!("  {m}"));
            }
        }

        ActionResult::success(json!({
            "root": root,
            "total_files": total_files,
            "by_extension": top_exts,
            "by_top_dir": top_dirs,
            "query": query,
            "query_matches": query_matches.into_iter().take(50).collect::<Vec<_>>(),
            "summary": lines.join("\n"),
        }))
    }
}

fn walk_depth(root: &Path, depth: usize, visit: &mut impl FnMut(&PathBuf) -> bool) {
    if depth > MAX_DEPTH {
        return;
    }
    let Ok(rd) = std::fs::read_dir(root) else {
        return;
    };
    for entry in rd.flatten() {
        let path = entry.path();
        if path.is_dir() {
            let name = entry.file_name().to_string_lossy().to_string();
            if SKIP_DIRS.contains(&name.as_str()) {
                continue;
            }
            walk_depth(&path, depth + 1, visit);
        } else if !visit(&path) {
            return;
        }
    }
}

#[async_trait]
impl ActionAdapter for CodeIntelAdapter {
    fn tool(&self) -> &str {
        "codeintel"
    }

    fn actions(&self) -> Vec<&str> {
        Self::all_actions().to_vec()
    }

    async fn execute(&self, req: ActionRequest) -> ActionResult {
        match req.action.as_str() {
            "understand" => self.understand(&req),
            other => ActionResult::error(format!("unknown codeintel action: {other}")),
        }
    }
}

/// Capability record published to the Capability Intelligence registry.
pub fn capabilities() -> Vec<friday_capabilities::model::Capability> {
    friday_capabilities::builtins::codeintel()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_tree() -> tempfile_lite::TempDir {
        tempfile_lite::TempDir::new().expect("temp dir")
    }

    #[tokio::test]
    async fn understand_reports_structure_and_matches() {
        let dir = temp_tree();
        let root = dir.path();
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::create_dir_all(root.join("docs")).unwrap();
        std::fs::create_dir_all(root.join("target")).unwrap();
        std::fs::write(root.join("src/main.rs"), "fn main() {}").unwrap();
        std::fs::write(root.join("src/lib.rs"), "").unwrap();
        std::fs::write(root.join("docs/README.md"), "# readme").unwrap();
        std::fs::write(root.join("Cargo.toml"), "[package]").unwrap();
        std::fs::write(root.join("target/x.rs"), "skip me").unwrap();

        let adapter = CodeIntelAdapter::new();
        let req = ActionRequest::new(
            "codeintel",
            "understand",
            json!({ "root": root.to_string_lossy(), "query": "main" }),
        );
        let res = adapter.execute(req).await;
        assert!(res.success, "understand failed: {:?}", res.error);
        let out = &res.output;
        assert_eq!(out["total_files"], 4, "target must be skipped");
        let matches = out["query_matches"].as_array().unwrap();
        assert_eq!(matches.len(), 1);
        assert!(
            matches[0].as_str().unwrap().ends_with("main.rs"),
            "match: {:?}",
            matches[0]
        );
        let summary = out["summary"].as_str().unwrap();
        assert!(summary.contains("main.rs"), "summary: {summary}");
    }

    #[tokio::test]
    async fn understand_rejects_missing_root() {
        let adapter = CodeIntelAdapter::new();
        let req = ActionRequest::new(
            "codeintel",
            "understand",
            json!({ "root": "/definitely/not/a/real/path-xyz" }),
        );
        let res = adapter.execute(req).await;
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
                "friday-codeintel-test-{}",
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
