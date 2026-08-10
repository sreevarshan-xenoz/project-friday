//! Filesystem capability adapter (M4.3).
//!
//! Implements the platform's file actions: create, read, write, delete, rename,
//! copy, move, and list. The permission engine decides whether dangerous
//! actions (e.g. `delete_file`) are allowed; this adapter only performs them
//! when the runtime dispatches the request.

use async_trait::async_trait;
use serde_json::json;
use std::path::Path;

use crate::adapters::ActionAdapter;
use crate::request::{ActionRequest, ActionResult};

pub struct FilesystemAdapter;

impl Default for FilesystemAdapter {
    fn default() -> Self {
        Self::new()
    }
}

impl FilesystemAdapter {
    pub fn new() -> Self {
        Self
    }

    fn all_actions() -> &'static [&'static str] {
        &[
            "create_file",
            "read_file",
            "write_file",
            "delete_file",
            "rename",
            "copy",
            "move",
            "list_directory",
            "tree",
            "search",
        ]
    }
}

#[async_trait]
impl ActionAdapter for FilesystemAdapter {
    fn tool(&self) -> &str {
        "filesystem"
    }

    fn actions(&self) -> Vec<&str> {
        Self::all_actions().to_vec()
    }

    async fn execute(&self, req: ActionRequest) -> ActionResult {
        let result = match req.action.as_str() {
            "create_file" => self.create_file(&req),
            "read_file" => self.read_file(&req),
            "write_file" => self.write_file(&req),
            "delete_file" => self.delete_file(&req),
            "rename" => self.rename(&req),
            "copy" => self.copy(&req),
            "move" => self.move_file(&req),
            "list_directory" => self.list_directory(&req),
            "tree" => self.tree(&req),
            "search" => self.search(&req),
            other => ActionResult::error(format!("unknown filesystem action: {other}")),
        };
        result
    }
}

impl FilesystemAdapter {
    fn create_file(&self, req: &ActionRequest) -> ActionResult {
        let path = match req.param_str("path") {
            Ok(p) => p,
            Err(e) => return ActionResult::error(e),
        };
        let p = Path::new(&path);
        if let Some(parent) = p.parent() {
            if !parent.as_os_str().is_empty() && !parent.exists() {
                if let Err(e) = std::fs::create_dir_all(parent) {
                    return ActionResult::error(format!("create_dir_all failed: {e}"));
                }
            }
        }
        if p.exists() {
            return ActionResult::error("file already exists");
        }
        let content = req
            .parameters
            .get("content")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        match std::fs::write(p, content) {
            Ok(()) => ActionResult::success(json!({ "path": path })),
            Err(e) => ActionResult::error(format!("write failed: {e}")),
        }
    }

    fn read_file(&self, req: &ActionRequest) -> ActionResult {
        let path = match req.param_str("path") {
            Ok(p) => p,
            Err(e) => return ActionResult::error(e),
        };
        match std::fs::read_to_string(Path::new(&path)) {
            Ok(content) => ActionResult::success(json!({ "path": path, "content": content })),
            Err(e) => ActionResult::error(format!("read failed: {e}")),
        }
    }

    fn write_file(&self, req: &ActionRequest) -> ActionResult {
        let path = match req.param_str("path") {
            Ok(p) => p,
            Err(e) => return ActionResult::error(e),
        };
        let content = match req.parameters.get("content").and_then(|v| v.as_str()) {
            Some(c) => c.to_string(),
            None => return ActionResult::error("missing_string_parameter"),
        };
        if let Some(parent) = Path::new(&path).parent() {
            if !parent.as_os_str().is_empty() && !parent.exists() {
                if let Err(e) = std::fs::create_dir_all(parent) {
                    return ActionResult::error(format!("create_dir_all failed: {e}"));
                }
            }
        }
        match std::fs::write(Path::new(&path), content) {
            Ok(()) => ActionResult::success(json!({ "path": path })),
            Err(e) => ActionResult::error(format!("write failed: {e}")),
        }
    }

    fn delete_file(&self, req: &ActionRequest) -> ActionResult {
        let path = match req.param_str("path") {
            Ok(p) => p,
            Err(e) => return ActionResult::error(e),
        };
        match std::fs::remove_file(Path::new(&path)) {
            Ok(()) => ActionResult::success(json!({ "path": path })),
            Err(e) => ActionResult::error(format!("delete failed: {e}")),
        }
    }

    fn rename(&self, req: &ActionRequest) -> ActionResult {
        self.move_file(req)
    }

    fn copy(&self, req: &ActionRequest) -> ActionResult {
        let from = match req.param_str("from") {
            Ok(p) => p,
            Err(e) => return ActionResult::error(e),
        };
        let to = match req.param_str("to") {
            Ok(p) => p,
            Err(e) => return ActionResult::error(e),
        };
        match std::fs::copy(Path::new(&from), Path::new(&to)) {
            Ok(bytes) => ActionResult::success(json!({ "from": from, "to": to, "bytes": bytes })),
            Err(e) => ActionResult::error(format!("copy failed: {e}")),
        }
    }

    fn move_file(&self, req: &ActionRequest) -> ActionResult {
        let from = match req.param_str("from") {
            Ok(p) => p,
            Err(e) => return ActionResult::error(e),
        };
        let to = match req.param_str("to") {
            Ok(p) => p,
            Err(e) => return ActionResult::error(e),
        };
        if let Some(parent) = Path::new(&to).parent() {
            if !parent.as_os_str().is_empty() && !parent.exists() {
                if let Err(e) = std::fs::create_dir_all(parent) {
                    return ActionResult::error(format!("create_dir_all failed: {e}"));
                }
            }
        }
        match std::fs::rename(Path::new(&from), Path::new(&to)) {
            Ok(()) => ActionResult::success(json!({ "from": from, "to": to })),
            Err(e) => ActionResult::error(format!("move failed: {e}")),
        }
    }

    fn list_directory(&self, req: &ActionRequest) -> ActionResult {
        let path = match req.param_str("path") {
            Ok(p) => p,
            Err(e) => return ActionResult::error(e),
        };
        let entries = match std::fs::read_dir(Path::new(&path)) {
            Ok(rd) => rd,
            Err(e) => return ActionResult::error(format!("read_dir failed: {e}")),
        };
        let mut items = Vec::new();
        for entry in entries.flatten() {
            let is_dir = entry.path().is_dir();
            items.push(json!({
                "name": entry.file_name().to_string_lossy().to_string(),
                "is_dir": is_dir,
            }));
        }
        ActionResult::success(json!({ "path": path, "entries": items }))
    }

    /// Recursive directory structure (`tree`). Returns a nested tree of
    /// entries so "show me the whole project structure" produces the actual
    /// hierarchy rather than a flat one-level listing. Bounded by
    /// `max_depth` (default 6) and `max_entries` (default 5000) so an
    /// unbounded walk can never hang or flood the reply.
    fn tree(&self, req: &ActionRequest) -> ActionResult {
        let path = match req.param_str("path") {
            Ok(p) => p,
            Err(e) => return ActionResult::error(e),
        };
        let max_depth = req
            .parameters
            .get("max_depth")
            .and_then(|v| v.as_u64())
            .unwrap_or(6) as usize;
        let max_entries = req
            .parameters
            .get("max_entries")
            .and_then(|v| v.as_u64())
            .unwrap_or(5000) as usize;
        let mut budget = max_entries;
        let mut tree = Vec::new();
        if let Err(e) = Self::walk_tree(Path::new(&path), 0, max_depth, &mut tree, &mut budget) {
            return ActionResult::error(e);
        }
        ActionResult::success(json!({
            "path": path,
            "count": tree.len(),
            "tree": tree,
        }))
    }

    fn walk_tree(
        dir: &Path,
        depth: usize,
        max_depth: usize,
        out: &mut Vec<serde_json::Value>,
        budget: &mut usize,
    ) -> Result<(), String> {
        if *budget == 0 || depth > max_depth {
            return Ok(());
        }
        let rd = std::fs::read_dir(dir).map_err(|e| format!("read_dir failed: {e}"))?;
        for entry in rd.flatten() {
            if *budget == 0 {
                break;
            }
            *budget -= 1;
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().to_string();
            let display = path.to_string_lossy().to_string();
            if path.is_dir() {
                let mut children = Vec::new();
                let mut item = json!({ "name": name, "path": display, "is_dir": true });
                if let Err(e) = Self::walk_tree(&path, depth + 1, max_depth, &mut children, budget)
                {
                    item["error"] = json!(e);
                }
                item["children"] = json!(children);
                out.push(item);
            } else {
                out.push(json!({ "name": name, "path": display, "is_dir": false }));
            }
        }
        Ok(())
    }

    /// Recursive search (`search`). Collects entries whose names match every
    /// whitespace-separated `query` term (case-insensitive substring), and/or
    /// the extension `ext` (matched case-insensitively against the suffix).
    /// When `content` is set, only *files* whose contents contain that string
    /// (case-insensitive) match — "find the Rust files containing `Foo`"
    /// searches symbols and prose, not just filenames. Directories never match
    /// a content query. The walk is bounded: `max_entries` caps the total
    /// entries visited (default 10000) so a huge tree can never hang the
    /// search. When the cap is hit the matches found so far are returned with
    /// `truncated: true`.
    fn search(&self, req: &ActionRequest) -> ActionResult {
        let path = match req.param_str("path") {
            Ok(p) => p,
            Err(e) => return ActionResult::error(e),
        };
        let query = req
            .parameters
            .get("query")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_lowercase();
        let ext = req
            .parameters
            .get("ext")
            .and_then(|v| v.as_str())
            .map(|e| e.to_lowercase());
        let content = req
            .parameters
            .get("content")
            .and_then(|v| v.as_str())
            .map(|c| c.to_lowercase());
        let max_depth = req
            .parameters
            .get("max_depth")
            .and_then(|v| v.as_u64())
            .unwrap_or(12) as usize;
        let max_entries = req
            .parameters
            .get("max_entries")
            .and_then(|v| v.as_u64())
            .unwrap_or(10_000) as usize;
        let terms: Vec<&str> = query.split_whitespace().collect();
        let mut matches = Vec::new();
        let mut budget = max_entries;
        let mut walk_err: Option<String> = None;
        Self::walk_search(
            Path::new(&path),
            0,
            max_depth,
            &terms,
            ext.as_deref(),
            content.as_deref(),
            &mut matches,
            &mut budget,
            &mut walk_err,
        );
        let truncated = budget == 0;
        let mut resp = json!({
            "path": path,
            "query": query,
            "matches": matches,
            "truncated": truncated,
        });
        if let Some(c) = content {
            resp["content"] = json!(c);
        }
        if let Some(e) = walk_err {
            if matches.is_empty() {
                return ActionResult::error(e);
            }
            resp["note"] = json!(e);
        }
        ActionResult::success(resp)
    }

    /// Read-size cap for a `content` search term — a matching search never
    /// slurps a multi-gigabyte binary.
    const MAX_CONTENT_BYTES: u64 = 8 * 1024 * 1024;

    /// Whether `path` (a regular file under the cap) contains `needle`, read as
    /// UTF-8 text. Non-text or unreadable files never match.
    fn file_contains(path: &Path, needle: &str) -> bool {
        let Ok(meta) = std::fs::metadata(path) else {
            return false;
        };
        if !meta.is_file() || meta.len() > Self::MAX_CONTENT_BYTES {
            return false;
        }
        let Ok(bytes) = std::fs::read(path) else {
            return false;
        };
        match std::str::from_utf8(&bytes) {
            Ok(text) => text.to_lowercase().contains(needle),
            Err(_) => false,
        }
    }

    fn walk_search(
        dir: &Path,
        depth: usize,
        max_depth: usize,
        terms: &[&str],
        ext: Option<&str>,
        content: Option<&str>,
        matches: &mut Vec<serde_json::Value>,
        budget: &mut usize,
        walk_err: &mut Option<String>,
    ) {
        if depth > max_depth {
            return;
        }
        let rd = match std::fs::read_dir(dir) {
            Ok(rd) => rd,
            Err(e) => {
                *walk_err = Some(format!("read_dir failed: {e}"));
                return;
            }
        };
        for entry in rd.flatten() {
            if *budget == 0 {
                return;
            }
            *budget -= 1;
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().to_string();
            let lower = name.to_lowercase();
            let ext_ok = ext.map_or(true, |e| lower.ends_with(&format!(".{e}")));
            let terms_ok =
                terms.is_empty() || terms.iter().all(|t| !t.is_empty() && lower.contains(&**t));
            let content_ok = match content {
                Some(needle) => path.is_file() && Self::file_contains(&path, needle),
                None => true,
            };
            if ext_ok && terms_ok && content_ok {
                matches.push(json!({
                    "name": name,
                    "path": path.to_string_lossy().to_string(),
                    "is_dir": path.is_dir(),
                }));
            }
            if path.is_dir() {
                Self::walk_search(
                    &path,
                    depth + 1,
                    max_depth,
                    terms,
                    ext,
                    content,
                    matches,
                    budget,
                    walk_err,
                );
            }
        }
    }
}

/// Capability record published to the Capability Intelligence registry.
pub fn capabilities() -> Vec<friday_capabilities::model::Capability> {
    friday_capabilities::builtins::filesystem()
}
