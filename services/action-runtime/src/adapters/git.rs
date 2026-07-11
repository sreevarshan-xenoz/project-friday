//! Git capability adapter (M4.5.1).
//!
//! Git is exposed as **capabilities**, not shell commands. The adapter maps a
//! normalized `ActionRequest` (`tool = "git"`, `action = "<verb>"`) onto a Git
//! CLI invocation behind the scenes. Because the planner and runtime only ever
//! see the capability names (`git.clone`, `git.commit`, ...), the implementation
//! can later be swapped from CLI to `libgit2` without touching anything above.
//!
//! Supported verbs: `clone`, `init`, `status`, `add`, `commit`, `checkout`,
//! `branch`, `merge`, `diff`, `push`, `pull`. Each resolves to a `git` process
//! with a captured, normalized result.

use async_trait::async_trait;
use serde_json::json;
use std::collections::HashMap;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::adapters::ActionAdapter;
use crate::request::{ActionRequest, ActionResult};

/// Identity used in commits when the caller doesn't supply author config.
const COMMIT_NAME: &str = "friday";
const COMMIT_EMAIL: &str = "friday@local";

pub struct GitAdapter;

impl Default for GitAdapter {
    fn default() -> Self {
        Self::new()
    }
}

impl GitAdapter {
    pub fn new() -> Self {
        Self
    }

    fn all_actions() -> &'static [&'static str] {
        &[
            "clone", "init", "status", "add", "commit", "checkout", "branch", "merge", "diff",
            "push", "pull",
        ]
    }

    fn repo_path(req: &ActionRequest) -> PathBuf {
        req.param_str("repo")
            .or_else(|_| req.param_str("path"))
            .map(PathBuf::from)
            .unwrap_or_else(|_| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")))
    }

    fn optional_str(req: &ActionRequest, key: &str) -> Option<String> {
        req.parameters
            .get(key)
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
    }

    fn string_array(req: &ActionRequest, key: &str) -> Vec<String> {
        req.parameters
            .get(key)
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
                    .collect()
            })
            .unwrap_or_default()
    }
}

#[async_trait]
impl ActionAdapter for GitAdapter {
    fn tool(&self) -> &str {
        "git"
    }

    fn actions(&self) -> Vec<&str> {
        Self::all_actions().to_vec()
    }

    async fn execute(&self, req: ActionRequest) -> ActionResult {
        match req.action.as_str() {
            "clone" => {
                let url = match req.param_str("url") {
                    Ok(u) => u,
                    Err(e) => return ActionResult::error(e),
                };
                let dest = match req.param_str("dest") {
                    Ok(d) => d,
                    Err(_) => url
                        .trim_end_matches('/')
                        .trim_end_matches(".git")
                        .to_string(),
                };
                let args = vec!["clone".to_string(), url, dest];
                run_git(&PathBuf::from("."), &args)
            }
            "init" => {
                let path = Self::repo_path(&req);
                run_git(&path, &["init"])
            }
            "status" => {
                let repo = Self::repo_path(&req);
                run_git(&repo, &["status", "--porcelain"])
            }
            "add" => {
                let repo = Self::repo_path(&req);
                let mut paths = Self::string_array(&req, "paths");
                if paths.is_empty() {
                    paths.push(".".to_string());
                }
                let mut args = vec!["add".to_string()];
                args.extend(paths);
                run_git(&repo, &args)
            }
            "commit" => {
                let repo = Self::repo_path(&req);
                let message = match req.param_str("message") {
                    Ok(m) => m,
                    Err(e) => return ActionResult::error(e),
                };
                run_git(
                    &repo,
                    &[
                        "-c".to_string(),
                        format!("user.name={COMMIT_NAME}"),
                        "-c".to_string(),
                        format!("user.email={COMMIT_EMAIL}"),
                        "commit".to_string(),
                        "-m".to_string(),
                        message,
                    ],
                )
            }
            "checkout" => {
                let repo = Self::repo_path(&req);
                let branch = match req.param_str("ref").or_else(|_| req.param_str("branch")) {
                    Ok(b) => b,
                    Err(e) => return ActionResult::error(e),
                };
                let create = req
                    .parameters
                    .get("create")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                if create {
                    let args = vec!["checkout".to_string(), "-b".to_string(), branch];
                    run_git(&repo, &args)
                } else {
                    let args = vec!["checkout".to_string(), branch];
                    run_git(&repo, &args)
                }
            }
            "branch" => {
                let repo = Self::repo_path(&req);
                match Self::optional_str(&req, "name") {
                    Some(name) => {
                        let args = vec!["branch".to_string(), name];
                        run_git(&repo, &args)
                    }
                    None => run_git(&repo, &["branch", "--list"]),
                }
            }
            "merge" => {
                let repo = Self::repo_path(&req);
                let branch = match req.param_str("ref").or_else(|_| req.param_str("branch")) {
                    Ok(b) => b,
                    Err(e) => return ActionResult::error(e),
                };
                let args = vec!["merge".to_string(), branch];
                run_git(&repo, &args)
            }
            "diff" => {
                let repo = Self::repo_path(&req);
                let extra = Self::string_array(&req, "args");
                let mut args = vec!["diff".to_string()];
                args.extend(extra);
                run_git(&repo, &args)
            }
            "push" => {
                let repo = Self::repo_path(&req);
                let mut args = vec!["push".to_string()];
                if let Some(remote) = Self::optional_str(&req, "remote") {
                    args.push(remote);
                    if let Some(branch) = Self::optional_str(&req, "branch") {
                        args.push(branch);
                    }
                }
                run_git(&repo, &args)
            }
            "pull" => {
                let repo = Self::repo_path(&req);
                let mut args = vec!["pull".to_string()];
                if let Some(remote) = Self::optional_str(&req, "remote") {
                    args.push(remote);
                    if let Some(branch) = Self::optional_str(&req, "branch") {
                        args.push(branch);
                    }
                }
                run_git(&repo, &args)
            }
            other => ActionResult::error(format!("unknown git action: {other}")),
        }
    }
}

/// Run `git` in `cwd` with `args`; normalize stdout/stderr/exit into an
/// [`ActionResult`].
fn run_git(cwd: &Path, args: &[impl AsRef<OsStr>]) -> ActionResult {
    let output = Command::new("git").current_dir(cwd).args(args).output();
    match output {
        Ok(o) => {
            let stdout = String::from_utf8_lossy(&o.stdout).to_string();
            let stderr = String::from_utf8_lossy(&o.stderr).to_string();
            let success = o.status.success();
            ActionResult {
                success,
                output: json!({
                    "stdout": stdout,
                    "stderr": stderr,
                    "exit_code": o.status.code(),
                }),
                error: if success { None } else { Some(stderr.clone()) },
                metadata: HashMap::new(),
            }
        }
        Err(e) => ActionResult::error(format!("failed to spawn git: {e}")),
    }
}
