//! End-to-end test of the Action Runtime wired into the FRIDAY core.
//!
//! A plan's `exec.action` events are routed to the AgentKernel, which applies
//! the permission gate, dispatches to a real adapter, records an audit entry,
//! and emits `exec.result`. We assert on the emitted events as well as the
//! audit trail.

use async_trait::async_trait;
use friday_actions::adapters::{FilesystemAdapter, TerminalAdapter};
use friday_actions::permission::PermissionEngine;
use friday_actions::runtime::AgentKernel;
use friday_core::event::Event;
use friday_core::plugin::{Plugin, PluginMetadata};
use friday_core::Friday;
use friday_planner::events;
use friday_planner::register_engines;
use serde_json::json;
use std::time::Duration;

/// Captures `exec.result` events so the test can assert on outcomes.
struct Sink;

#[async_trait]
impl Plugin for Sink {
    fn metadata(&self) -> &PluginMetadata {
        static META: std::sync::OnceLock<PluginMetadata> = std::sync::OnceLock::new();
        META.get_or_init(|| {
            PluginMetadata::builder("sink", "Result Sink")
                .subscribes(&[events::EXEC_RESULT])
                .build()
        })
    }

    async fn handle_event(&self, _event: &Event) -> friday_core::Result<Vec<Event>> {
        Ok(vec![])
    }
}

async fn run_action(capability: &str, params: serde_json::Value, timeout: Duration) -> Vec<Event> {
    friday_core::init_telemetry();
    let mut friday = Friday::new();
    register_engines(&mut friday).await.unwrap();

    let rt = AgentKernel::new().with_timeout(timeout);
    rt.register_adapter(Box::new(FilesystemAdapter::new()))
        .await;
    rt.register_adapter(Box::new(TerminalAdapter::new())).await;
    friday.register_plugin(Box::new(rt)).await.unwrap();
    friday.register_plugin(Box::new(Sink)).await.unwrap();

    let bus = friday.bus().clone();
    let mut rx = bus.subscribe();

    let goal = Event::new(
        events::EXEC_ACTION,
        "test",
        json!({
            "task_id": "00000000-0000-0000-0000-000000000001",
            "plan_id": "00000000-0000-0000-0000-000000000002",
            "capability": capability,
            "params": params,
        }),
    );
    bus.publish(goal).await.unwrap();

    let handle = tokio::spawn(async move { friday.run_with_timeout(timeout).await });

    let mut results = Vec::new();
    let deadline = tokio::time::Instant::now() + timeout;
    while tokio::time::Instant::now() < deadline {
        if let Ok(Ok(ev)) = tokio::time::timeout(Duration::from_millis(100), rx.recv()).await {
            if ev.kind == events::EXEC_RESULT {
                results.push(ev);
            }
        }
    }
    handle.await.unwrap().unwrap();
    results
}

#[tokio::test]
async fn safe_filesystem_action_emits_success() {
    let tmp = std::env::temp_dir().join(format!("friday-it-{}", uuid::Uuid::new_v4().simple()));
    std::fs::create_dir_all(&tmp).unwrap();
    let p = tmp.join("note.txt").to_string_lossy().to_string();

    let results = run_action(
        "filesystem.create_file",
        json!({ "path": p, "content": "hi" }),
        Duration::from_secs(5),
    )
    .await;

    assert_eq!(results.len(), 1, "expected exactly one exec.result");
    let r = &results[0];
    assert_eq!(
        r.payload.get("success").and_then(|v| v.as_bool()),
        Some(true)
    );

    std::fs::remove_dir_all(&tmp).ok();
}

#[tokio::test]
async fn denied_action_returns_permission_error() {
    let results = run_action(
        "filesystem.delete_file",
        json!({ "path": "C:\\windows\\system32\\do_not_touch" }),
        Duration::from_secs(5),
    )
    .await;

    assert_eq!(results.len(), 1);
    let r = &results[0];
    assert_eq!(
        r.payload.get("success").and_then(|v| v.as_bool()),
        Some(false)
    );
    let err = r
        .payload
        .get("error")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    assert!(err.contains("permission"), "expected denial, got: {err}");
}

#[tokio::test]
async fn terminal_action_runs_command() {
    let results = run_action(
        "terminal.run",
        json!({ "command": "echo friday-works" }),
        Duration::from_secs(8),
    )
    .await;

    assert_eq!(results.len(), 1, "expected one exec.result from terminal");
    let r = &results[0];
    assert_eq!(
        r.payload.get("success").and_then(|v| v.as_bool()),
        Some(true)
    );
    let out = r
        .payload
        .get("output")
        .and_then(|v| v.get("stdout"))
        .and_then(|v| v.as_str())
        .unwrap_or("");
    assert!(out.contains("friday-works"), "terminal output was: {out:?}");
}

/// M8: an MCP server is just another `ActionAdapter`. We stand up a hermetic
/// fake MCP server, register it with the kernel under the tool key `mcp:fake`,
/// and dispatch an `exec.action` for `mcp:fake.read_file` through the unified
/// `run_action` path. The supervisor/planner/kernel never branch on
/// `CapabilitySource::Mcp` - routing is purely by tool key, identical to a
/// native adapter.
#[tokio::test]
async fn mcp_server_is_just_another_adapter() {
    use friday_actions::kernel::ExecutionContext;
    use friday_mcp::server::{FakeMcpServer, FakeTransport};

    let (client_side, server_side) = FakeTransport::pair();
    let server = FakeMcpServer::new().with_call_handler(|name, args| {
        serde_json::json!({
            "content": [ { "type": "text", "text": format!("read {name} -> {args}") } ],
            "is_error": false,
        })
    });
    tokio::spawn(server.serve(server_side));

    let kernel = AgentKernel::new();
    let tools = kernel
        .register_mcp_server("fake", Box::new(client_side))
        .await
        .expect("mcp server registers as adapter");
    assert!(
        tools.iter().any(|t| t == "read_file"),
        "discovered tools: {tools:?}"
    );

    let ctx = ExecutionContext::builder()
        .execution_id(uuid::Uuid::new_v4())
        .session_id(uuid::Uuid::new_v4())
        .build();
    let result = kernel
        .run_action(
            friday_actions::request::ActionRequest::new(
                "mcp:fake",
                "read_file",
                serde_json::json!({ "path": "/etc/hosts" }),
            ),
            ctx,
        )
        .await;

    assert!(result.success, "mcp call should succeed: {:?}", result.error);
    let text = result
        .output
        .get("content")
        .and_then(|c| c.get(0))
        .and_then(|c| c.get("text"))
        .and_then(|t| t.as_str())
        .unwrap_or("");
    assert!(text.contains("read_file"), "unexpected output: {text}");
}

/// M8.3: cancellation is unified. A cancelled context makes `run_action` return
/// a cancelled `ActionResult` for the MCP backend exactly as for native backends
/// � the kernel's `tokio::select!` races the in-flight `execute` against the
/// token, so no backend-specific cancellation code exists.
#[tokio::test]
async fn mcp_adapter_respects_cancellation() {
    use friday_actions::kernel::ExecutionContext;
    use friday_mcp::server::{FakeMcpServer, FakeTransport};

    let (client_side, server_side) = FakeTransport::pair();
    // Server that never answers tools/call, so the call stays in flight.
    let server = FakeMcpServer::new();
    tokio::spawn(server.serve(server_side));

    let kernel = AgentKernel::new().with_timeout(std::time::Duration::from_secs(30));
    kernel
        .register_mcp_server("slow", Box::new(client_side))
        .await
        .expect("register");

    let ctx = ExecutionContext::builder()
        .execution_id(uuid::Uuid::new_v4())
        .session_id(uuid::Uuid::new_v4())
        .build();
    // Cancel immediately before dispatching.
    ctx.cancellation_token.cancel();

    let result = kernel
        .run_action(
            friday_actions::request::ActionRequest::new("mcp:slow", "read_file", serde_json::json!({})),
            ctx,
        )
        .await;

    assert!(!result.success);
    let err = result.error.clone().unwrap_or_default();
    assert!(err.contains("cancelled"), "expected cancellation, got: {err}");
}

/// M9.2: the unified permission gate covers MCP with no MCP-specific code. We
/// register the *real* filesystem server as an McpAdapter, then inject a
/// PermissionEngine that denies `mcp:filesystem.write_file`. The MCP write must
/// be blocked at the gate � exactly like a native write would be � proving the
/// runtime owns permission, not the backend.
#[tokio::test]
async fn mcp_write_is_gated_by_unified_permission() {
    if std::env::var("FRIDAY_REAL_MCP").map(|v| v != "1").unwrap_or(true) {
        eprintln!("skipping real MCP test (set FRIDAY_REAL_MCP=1 to enable)");
        return;
    }
    let root = std::env::temp_dir().join(format!("friday-real-perm-{}", uuid::Uuid::new_v4().simple()));
    std::fs::create_dir_all(&root).unwrap();

    let root_str = root.to_string_lossy().to_string();
    let (command, args): (&str, Vec<String>) = if cfg!(windows) {
        (
            "cmd",
            vec!["/c".into(), "npx".into(), "--yes".into(),
                 "@modelcontextprotocol/server-filesystem".into(), root_str],
        )
    } else {
        (
            "npx",
            vec!["--yes".into(), "@modelcontextprotocol/server-filesystem".into(), root_str],
        )
    };
    let (transport, _child) = friday_mcp::transport::stdio_transport_for(command, &args)
        .expect("spawn real filesystem MCP server");

    let mut kernel = AgentKernel::new()
        .with_permission(PermissionEngine::new().deny_substring("mcp:filesystem.write_file"));
    kernel
        .register_mcp_server("filesystem", transport)
        .await
        .expect("register real fs server as adapter");

    let ctx = friday_actions::kernel::ExecutionContext::builder()
        .execution_id(uuid::Uuid::new_v4())
        .session_id(uuid::Uuid::new_v4())
        .build();
    let result = kernel
        .run_action(
            friday_actions::request::ActionRequest::new(
                "mcp:filesystem",
                "write_file",
                serde_json::json!({ "path": root.join("x.md").to_string_lossy(), "content": "y" }),
            ),
            ctx,
        )
        .await;

    assert!(!result.success, "denied MCP write must not execute");
    let err = result.error.clone().unwrap_or_default();
    assert!(err.contains("permission"), "expected permission denial, got: {err}");

    std::fs::remove_dir_all(&root).ok();
}

/// M9.3: artifact tracking. A successful MCP `write_file` annotates the created
/// path; the runtime emits an `artifact.created` event (hermetic server, so it
/// runs in CI without Node). Reflection/memory can consume this uniformly.
#[tokio::test]
async fn mcp_write_emits_artifact_created() {
    use friday_mcp::server::{FakeMcpServer, FakeTransport};

    let (client_side, server_side) = FakeTransport::pair();
    let server = FakeMcpServer::new().with_call_handler(|name, _args| {
        serde_json::json!({ "content": [ { "type": "text", "text": format!("did {name}") } ], "is_error": false })
    });
    tokio::spawn(server.serve(server_side));

    let kernel = AgentKernel::new();
    kernel
        .register_mcp_server("fake", Box::new(client_side))
        .await
        .expect("register");

    let ctx = friday_actions::kernel::ExecutionContext::builder()
        .execution_id(uuid::Uuid::new_v4())
        .session_id(uuid::Uuid::new_v4())
        .build();
    let events = kernel
        .run_action(
            friday_actions::request::ActionRequest::new(
                "mcp:fake",
                "write_file",
                serde_json::json!({ "path": "/tmp/report.md", "content": "x" }),
            ),
            ctx,
        )
        .await;

    // run_action returns the EXEC_RESULT; the bus path emits artifact.created
    // separately, so assert via the public dispatch used by the kernel's
    // on_exec_action. Here we verify the artifact annotation directly.
    assert!(events.success, "write should succeed: {:?}", events.error);
    let paths = events.metadata.get("artifact_paths").and_then(|p| p.as_array());
    assert!(paths.is_some(), "write_file must annotate artifact_paths");
    assert_eq!(
        paths.unwrap()[0].as_str(),
        Some("/tmp/report.md"),
        "artifact path should match the written file"
    );
}
