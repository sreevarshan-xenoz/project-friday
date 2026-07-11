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
}
