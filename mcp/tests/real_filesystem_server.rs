//! Real Filesystem MCP server integration (M9.1).
//!
//! This is the first *production* MCP integration: it launches the real
//! `@modelcontextprotocol/server-filesystem` subprocess over stdio and drives it
//! with the production [`protocol::McpClient`]. It validates the things hermetic
//! tests cannot:
//!
//! * real subprocess (stdio) lifecycle,
//! * real JSON-RPC interoperability with an external server,
//! * real tool schemas / discovery,
//! * real filesystem artifacts (write/read/create/list actually touch disk),
//! * real error handling (path outside the allowed directory is rejected).
//!
//! Gated behind `FRIDAY_REAL_MCP=1` so hermetic CI (no Node) stays clean — this
//! matches the PR4 "deterministic hermetic CI" discipline. Run locally with:
//! `FRIDAY_REAL_MCP=1 cargo test -p friday-mcp --test real_filesystem_server`.

use friday_mcp::protocol::McpClient;
use friday_mcp::transport::stdio_transport_for;
use friday_mcp::transport::Transport;

/// Resolve the real server command once. Returns `None` (skips the test) when
/// `FRIDAY_REAL_MCP` is not set, so the suite is a no-op in hermetic CI.
fn real_mcp_enabled() -> bool {
    std::env::var("FRIDAY_REAL_MCP")
        .map(|v| v == "1")
        .unwrap_or(false)
}

/// Spawn the real filesystem server bound to `root`, returning a connected
/// initialized client. Real stdio lifecycle + JSON-RPC handshake.
async fn spawn_real_fs_client(root: &std::path::Path) -> McpClient<Box<dyn Transport>> {
    let root_str = root.to_string_lossy().to_string();
    // `npx --yes` fetches the package on first run; subsequent runs are cached.
    // On Windows `npx` is a `.cmd` script, so route through `cmd /c`; on Unix
    // invoke `npx` directly.
    let (command, args): (&str, Vec<String>) = if cfg!(windows) {
        (
            "cmd",
            vec![
                "/c".to_string(),
                "npx".to_string(),
                "--yes".to_string(),
                "@modelcontextprotocol/server-filesystem".to_string(),
                root_str,
            ],
        )
    } else {
        (
            "npx",
            vec![
                "--yes".to_string(),
                "@modelcontextprotocol/server-filesystem".to_string(),
                root_str,
            ],
        )
    };
    let (transport, _child) =
        stdio_transport_for(command, &args).expect("spawn real filesystem MCP server");

    let mut client = McpClient::new(transport);
    let info = client
        .initialize()
        .await
        .expect("initialize against real server");
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

#[tokio::test]
async fn real_filesystem_server_connect_initialize_discover() {
    if !real_mcp_enabled() {
        eprintln!("skipping real MCP test (set FRIDAY_REAL_MCP=1 to enable)");
        return;
    }
    let root =
        std::env::temp_dir().join(format!("friday-real-fs-{}", uuid::Uuid::new_v4().simple()));
    std::fs::create_dir_all(&root).unwrap();

    let mut client = spawn_real_fs_client(&root).await;
    let tools = client
        .list_tools()
        .await
        .expect("list_tools on real server");
    let names: Vec<&str> = tools.iter().map(|t| t.name.as_str()).collect();

    // Real tool schemas from the secure-filesystem-server.
    assert!(names.contains(&"write_file"), "tools: {names:?}");
    assert!(names.contains(&"read_text_file"), "tools: {names:?}");
    assert!(names.contains(&"create_directory"), "tools: {names:?}");
    assert!(names.contains(&"list_directory"), "tools: {names:?}");
}

#[tokio::test]
async fn real_filesystem_server_write_read_create_list() {
    if !real_mcp_enabled() {
        eprintln!("skipping real MCP test (set FRIDAY_REAL_MCP=1 to enable)");
        return;
    }
    let root =
        std::env::temp_dir().join(format!("friday-real-fs-{}", uuid::Uuid::new_v4().simple()));
    std::fs::create_dir_all(&root).unwrap();

    let mut client = spawn_real_fs_client(&root).await;

    // write_file -> real artifact on disk.
    let report = root.join("report.md");
    let r = client
        .call_tool(
            "write_file",
            serde_json::json!({ "path": report.to_string_lossy(), "content": "# Hello FRIDAY\n" }),
        )
        .await
        .expect("write_file");
    assert!(!r.is_error, "write_file error: {}", text_of(&r));

    // The file actually exists on disk with the written content.
    let on_disk = std::fs::read_to_string(&report).unwrap();
    assert!(on_disk.contains("Hello FRIDAY"), "disk content: {on_disk}");

    // read_text_file -> returns the content back through MCP.
    let r = client
        .call_tool(
            "read_text_file",
            serde_json::json!({ "path": report.to_string_lossy() }),
        )
        .await
        .expect("read_text_file");
    assert!(!r.is_error, "read_text_file error: {}", text_of(&r));
    assert!(
        text_of(&r).contains("Hello FRIDAY"),
        "read back: {}",
        text_of(&r)
    );

    // create_directory -> real directory on disk.
    let sub = root.join("sub");
    let r = client
        .call_tool(
            "create_directory",
            serde_json::json!({ "path": sub.to_string_lossy() }),
        )
        .await
        .expect("create_directory");
    assert!(!r.is_error, "create_directory error: {}", text_of(&r));
    assert!(sub.is_dir(), "directory should exist on disk");

    // list_directory -> reflects the real artifacts.
    let r = client
        .call_tool(
            "list_directory",
            serde_json::json!({ "path": root.to_string_lossy() }),
        )
        .await
        .expect("list_directory");
    assert!(!r.is_error, "list_directory error: {}", text_of(&r));
    let listing = text_of(&r);
    assert!(
        listing.contains("report.md"),
        "listing should show report.md: {listing}"
    );
    assert!(
        listing.contains("sub"),
        "listing should show sub: {listing}"
    );

    std::fs::remove_dir_all(&root).ok();
}

#[tokio::test]
async fn real_filesystem_server_rejects_out_of_sandbox_path() {
    if !real_mcp_enabled() {
        eprintln!("skipping real MCP test (set FRIDAY_REAL_MCP=1 to enable)");
        return;
    }
    let root =
        std::env::temp_dir().join(format!("friday-real-fs-{}", uuid::Uuid::new_v4().simple()));
    std::fs::create_dir_all(&root).unwrap();

    let mut client = spawn_real_fs_client(&root).await;

    // A path outside the allowed directory must be rejected by the server. The
    // secure-filesystem-server reports the violation as a tool error; we assert
    // on the observable outcome — no file is created outside the sandbox.
    let outside =
        std::env::temp_dir().join(format!("friday-outside-{}", uuid::Uuid::new_v4().simple()));
    let r = client
        .call_tool(
            "write_file",
            serde_json::json!({ "path": outside.to_string_lossy(), "content": "x" }),
        )
        .await
        .expect("write_file call returns (not panic)");
    let text = text_of(&r);
    let rejected = r.is_error
        || text.contains("outside")
        || text.contains("allowed")
        || text.contains("Access")
        || text.contains("not allowed");
    assert!(
        rejected,
        "server should reject out-of-sandbox write; got: {text}"
    );
    // The meaningful property: the file was NOT written outside the sandbox.
    assert!(!outside.exists(), "out-of-sandbox file must not be created");

    std::fs::remove_dir_all(&root).ok();
}
