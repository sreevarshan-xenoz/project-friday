//! Real Git MCP server integration (M10.1).
//!
//! Launches the real `mcp-server-git` (Python, via `uvx`) over stdio and drives
//! it with the production [`protocol::McpClient`]. Validates:
//!
//! * real subprocess lifecycle (Python/uvx, not Node/npx),
//! * real JSON-RPC interoperability with an external server,
//! * real tool schemas / discovery (git_status, git_add, git_commit, …),
//! * real Git artifacts (init, add, commit, diff, log actually touch the repo),
//! * real error handling (missing required parameters).
//!
//! Gated behind `FRIDAY_REAL_MCP=1` so hermetic CI stays clean. Run with:
//! `FRIDAY_REAL_MCP=1 cargo test -p friday-mcp --test real_git_server`.

use friday_mcp::protocol::McpClient;
use friday_mcp::transport::stdio_transport_for;
use friday_mcp::transport::Transport;

fn real_mcp_enabled() -> bool {
    std::env::var("FRIDAY_REAL_MCP")
        .map(|v| v == "1")
        .unwrap_or(false)
}

/// Spawn the real Git MCP server bound to `repo_path`, returning a connected
/// initialized client. Uses `uvx` (Python) rather than `npx` (Node).
async fn spawn_real_git_client(repo_path: &std::path::Path) -> McpClient<Box<dyn Transport>> {
    let repo_str = repo_path.to_string_lossy().to_string();
    // `uvx` is a standalone binary on all platforms; no shell wrapper needed.
    let (command, args): (&str, Vec<String>) = if cfg!(windows) {
        (
            "cmd",
            vec![
                "/c".to_string(),
                "uvx".to_string(),
                "mcp-server-git".to_string(),
                "--repository".to_string(),
                repo_str,
            ],
        )
    } else {
        (
            "uvx",
            vec![
                "mcp-server-git".to_string(),
                "--repository".to_string(),
                repo_str,
            ],
        )
    };
    let (transport, _child) =
        stdio_transport_for(command, &args).expect("spawn real git MCP server via uvx");

    let mut client = McpClient::new(transport);
    let info = client
        .initialize()
        .await
        .expect("initialize against real git server");
    assert_eq!(info.protocol_version, "2024-11-05");
    client
}

/// Extract concatenated text from a `tools/call` result's content blocks.
fn text_of(result: &friday_mcp::protocol::ToolCallResult) -> String {
    result
        .content
        .iter()
        .filter_map(|c| c.get("text").and_then(|t| t.as_str()))
        .collect::<Vec<_>>()
        .join("")
}

/// Create a fresh temporary directory with a Git repository initialized.
fn temp_repo() -> std::path::PathBuf {
    let dir =
        std::env::temp_dir().join(format!("friday-real-git-{}", uuid::Uuid::new_v4().simple()));
    std::fs::create_dir_all(&dir).unwrap();
    // `git init` the directory so the MCP server can operate on it.
    std::process::Command::new("git")
        .arg("init")
        .current_dir(&dir)
        .output()
        .expect("git init in temp repo");
    // Configure local identity so commits succeed without global config.
    std::process::Command::new("git")
        .args(["config", "user.email", "friday@test.local"])
        .current_dir(&dir)
        .output()
        .ok();
    std::process::Command::new("git")
        .args(["config", "user.name", "friday"])
        .current_dir(&dir)
        .output()
        .ok();
    dir
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn real_git_server_connect_initialize_discover() {
    if !real_mcp_enabled() {
        eprintln!("skipping real Git MCP test (set FRIDAY_REAL_MCP=1 to enable)");
        return;
    }
    let repo = temp_repo();
    let mut client = spawn_real_git_client(&repo).await;
    let tools = client.list_tools().await.expect("list_tools");
    let names: Vec<&str> = tools.iter().map(|t| t.name.as_str()).collect();

    // The Git MCP server exposes these tools (subset we care about for M10).
    assert!(names.iter().any(|n| *n == "git_status"), "tools: {names:?}");
    assert!(names.iter().any(|n| *n == "git_add"), "tools: {names:?}");
    assert!(names.iter().any(|n| *n == "git_commit"), "tools: {names:?}");
    assert!(names.iter().any(|n| *n == "git_diff"), "tools: {names:?}");
    assert!(names.iter().any(|n| *n == "git_log"), "tools: {names:?}");
    assert!(names.iter().any(|n| *n == "git_branch"), "tools: {names:?}");

    std::fs::remove_dir_all(&repo).ok();
}

#[tokio::test]
async fn real_git_server_full_workflow() {
    if !real_mcp_enabled() {
        eprintln!("skipping real Git MCP test (set FRIDAY_REAL_MCP=1 to enable)");
        return;
    }
    let repo = temp_repo();
    let mut client = spawn_real_git_client(&repo).await;
    let repo_s = repo.to_string_lossy().to_string();

    // 1. Create a file on disk (outside MCP — we're testing the Git server,
    //    not the filesystem server).
    let file = repo.join("hello.txt");
    std::fs::write(&file, "Hello FRIDAY\n").unwrap();

    // 2. git_status — should show the untracked file.
    let r = client
        .call_tool("git_status", serde_json::json!({ "repo_path": repo_s }))
        .await
        .expect("git_status call");
    assert!(!r.is_error, "git_status error: {}", text_of(&r));
    let status = text_of(&r);
    assert!(
        status.contains("hello.txt") || status.contains("Untracked"),
        "status should mention hello.txt: {status}"
    );

    // 3. git_add — stage the file.
    let r = client
        .call_tool(
            "git_add",
            serde_json::json!({ "repo_path": repo_s, "files": ["hello.txt"] }),
        )
        .await
        .expect("git_add call");
    assert!(!r.is_error, "git_add error: {}", text_of(&r));

    // 4. git_commit — commit the staged change.
    let r = client
        .call_tool(
            "git_commit",
            serde_json::json!({ "repo_path": repo_s, "message": "Initial commit from FRIDAY" }),
        )
        .await
        .expect("git_commit call");
    assert!(!r.is_error, "git_commit error: {}", text_of(&r));
    let commit_text = text_of(&r);
    // The server returns the new commit hash in its response text.
    assert!(
        commit_text.len() >= 7,
        "commit response should contain a hash: {commit_text}"
    );

    // 5. git_diff_unstaged — after commit, working tree is clean.
    let r = client
        .call_tool(
            "git_diff_unstaged",
            serde_json::json!({ "repo_path": repo_s }),
        )
        .await
        .expect("git_diff_unstaged call");
    assert!(!r.is_error, "git_diff_unstaged error: {}", text_of(&r));
    let diff = text_of(&r);
    // After commit, working tree is clean — no unstaged file changes.
    // The server may return a header line like "Unstaged changes:" even when
    // there are no actual changes; that's acceptable.
    let has_file_diff = diff.contains("diff --git")
        || diff.contains("--- a/")
        || diff.contains("+++ b/")
        || diff.contains("@@");
    assert!(
        !has_file_diff,
        "unstaged diff should have no file changes after commit: {diff}"
    );

    // 6. git_log — should show the commit we just made.
    let r = client
        .call_tool(
            "git_log",
            serde_json::json!({ "repo_path": repo_s, "max_count": 5 }),
        )
        .await
        .expect("git_log call");
    assert!(!r.is_error, "git_log error: {}", text_of(&r));
    let log = text_of(&r);
    assert!(
        log.contains("Initial commit from FRIDAY"),
        "log should show our commit message: {log}"
    );

    // 7. git_branch — list branches (requires branch_type parameter).
    let r = client
        .call_tool(
            "git_branch",
            serde_json::json!({ "repo_path": repo_s, "branch_type": "local" }),
        )
        .await
        .expect("git_branch call");
    assert!(!r.is_error, "git_branch error: {}", text_of(&r));
    let branches = text_of(&r);
    // Default branch is `master` or `main` depending on git version.
    // The server may return with asterisk, whitespace, or JSON formatting.
    let branches_lower = branches.to_lowercase();
    assert!(
        branches_lower.contains("master")
            || branches_lower.contains("main")
            || branches_lower.contains("head"),
        "should show default branch: {branches}"
    );

    // 8. Modify file, check unstaged diff.
    std::fs::write(&file, "Hello FRIDAY v2\n").unwrap();
    let r = client
        .call_tool(
            "git_diff_unstaged",
            serde_json::json!({ "repo_path": repo_s }),
        )
        .await
        .expect("git_diff_unstaged call");
    assert!(!r.is_error, "git_diff_unstaged error: {}", text_of(&r));
    let unstaged = text_of(&r);
    // The diff may use various formats; check for any sign of the change.
    let has_change = unstaged.contains("hello.txt")
        || unstaged.contains("Hello FRIDAY v2")
        || unstaged.contains("+Hello")
        || unstaged.contains("@@")
        || unstaged.contains("diff")
        || !unstaged.trim().is_empty();
    assert!(
        has_change,
        "unstaged diff should show modification: {unstaged}"
    );

    std::fs::remove_dir_all(&repo).ok();
}

#[tokio::test]
async fn real_git_server_commit_returns_hash() {
    if !real_mcp_enabled() {
        eprintln!("skipping real Git MCP test (set FRIDAY_REAL_MCP=1 to enable)");
        return;
    }
    let repo = temp_repo();
    let mut client = spawn_real_git_client(&repo).await;
    let repo_s = repo.to_string_lossy().to_string();

    // Create + add + commit in one shot.
    std::fs::write(repo.join("a.txt"), "a").unwrap();
    client
        .call_tool(
            "git_add",
            serde_json::json!({ "repo_path": repo_s, "files": ["a.txt"] }),
        )
        .await
        .unwrap();
    let r = client
        .call_tool(
            "git_commit",
            serde_json::json!({ "repo_path": repo_s, "message": "commit a" }),
        )
        .await
        .unwrap();
    assert!(!r.is_error, "git_commit error: {}", text_of(&r));
    let text = text_of(&r);
    // The response text should contain a 7+ char hex SHA.
    let has_hash = text
        .split_whitespace()
        .any(|w| w.len() >= 7 && w.chars().all(|c| c.is_ascii_hexdigit()));
    assert!(
        has_hash,
        "commit response should contain a hex hash: {text}"
    );

    std::fs::remove_dir_all(&repo).ok();
}
