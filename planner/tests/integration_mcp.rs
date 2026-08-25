//! End-to-end MCP integration test (the "vertical slice" review).
//!
//! This proves the whole FRIDAY stack can plan, resolve, bind, and *execute* an
//! MCP capability against a fake MCP server — no Node, no network:
//!
//! ```text
//! GOAL_RAW
//!   -> IntentEngine -> PlanningEngine (LLM planner via MockBackend)
//!   -> TaskGraph(capability = "mcp:filesystem.read_file")
//!   -> CapabilityRegistry.resolve -> ExecutionBinding{ adapter:"mcp:filesystem", action:"read_file" }
//!   -> GoalSupervisor dispatches EXEC_ACTION
//!   -> AgentKernel (real action runtime) -> McpAdapter (just another ActionAdapter)
//!   -> McpClient::call_tool("read_file")
//!   -> Transport (FakeTransport) -> FakeMcpServer
//!   -> EXEC_RESULT { capability:"mcp:filesystem.read_file", output:"Hello FRIDAY" }
//!   -> GoalSupervisor -> VerificationEngine -> GOAL_COMPLETED
//! ```
//!
//! The point of M8: the supervisor and planner know *nothing* about MCP. The
//! `AgentKernel` dispatches `exec.action` by tool key, and the `McpAdapter`
//! (registered under `mcp:filesystem`) is indistinguishable from a native
//! adapter. There is no `mcp:`-special-casing shim anywhere in this slice.

use friday_core::event::Event;
use friday_core::Friday;
use friday_mcp::adapter::register_server_capabilities;
use friday_mcp::protocol::McpTool;
use friday_mcp::server::{FakeMcpServer, FakeTransport};
use friday_mcp::transport::Transport;
use friday_planner::events;
use friday_planner::planning::planners::backend::MockBackend;
use friday_planner::register_engines_with_llm;
use serde_json::json;

/// Collected bus events for assertions.
struct Run {
    events: Vec<Event>,
}

async fn run_mcp_goal(text: &str, plan_json: &str) -> Run {
    friday_core::init_telemetry();
    let mut friday = Friday::new();
    let backend = Some(Box::new(MockBackend::new(plan_json)) as Box<_>);
    let registry = register_engines_with_llm(&mut friday, backend)
        .await
        .expect("register planner stack");

    // Register the MCP capability into the *same* registry the supervisor resolves.
    let tool = McpTool {
        name: "read_file".to_string(),
        description: "Read a file from disk".to_string(),
        input_schema: json!({ "type": "object", "properties": { "path": { "type": "string" } } }),
    };
    register_server_capabilities("filesystem", std::slice::from_ref(&tool), &registry);

    // Stand up the fake MCP server. The client-side transport is handed to the
    // AgentKernel, which initializes + lists tools itself when registering the
    // McpAdapter (M8: MCP is just another ActionAdapter, no pre-initialized
    // client threaded through the test).
    let (client_side, server_side) = FakeTransport::pair();
    let server = FakeMcpServer::new().with_tools(vec![tool]).with_call_handler(|name, args| {
        json!({
            "content": [ { "type": "text", "text": format!("Hello FRIDAY from {name}({args})") } ],
            "is_error": false
        })
    });
    tokio::spawn(async move { server.serve(server_side).await });

    // M8: register the MCP server as a first-class ActionAdapter under
    // `mcp:filesystem`. The real AgentKernel now routes `exec.action` for
    // `mcp:filesystem.read_file` to it — no shim, no CapabilitySource branching.
    let kernel = friday_actions::runtime::AgentKernel::new();
    kernel
        .register_mcp_server("filesystem", Box::new(client_side))
        .await
        .expect("mcp server registers as adapter");
    friday.register_plugin(Box::new(kernel)).await.unwrap();

    let bus = friday.bus().clone();
    let mut rx = bus.subscribe();
    bus.publish(Event::new(
        events::GOAL_RAW,
        "user",
        json!({ "text": text }),
    ))
    .await
    .unwrap();

    let handle = tokio::spawn(async move {
        friday
            .run_with_timeout(std::time::Duration::from_secs(12))
            .await
    });

    let mut collected = Vec::new();
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(3);
    while tokio::time::Instant::now() < deadline {
        if let Ok(Ok(ev)) =
            tokio::time::timeout(std::time::Duration::from_millis(100), rx.recv()).await
        {
            collected.push(ev);
        }
    }
    let _ = handle.await.unwrap();
    Run { events: collected }
}

fn saw(run: &Run, kind: &str) -> bool {
    run.events.iter().any(|e| e.kind == kind)
}

fn exec_result_for(run: &Run, capability: &str) -> Option<Event> {
    run.events
        .iter()
        .find(|e| {
            e.kind == events::EXEC_RESULT
                && e.payload.get("capability").and_then(|c| c.as_str()) == Some(capability)
        })
        .cloned()
}

#[tokio::test]
async fn mcp_capability_executes_end_to_end() {
    let plan = json!({
        "tasks": [
            {
                "name": "Read README",
                "capability": "mcp:filesystem.read_file",
                "params": { "path": "README.md" },
                "depends_on_indices": []
            }
        ]
    })
    .to_string();

    let run = run_mcp_goal("Read README.md", &plan).await;

    // The whole slice fired: planning, execution dispatch, MCP call, verification.
    assert!(saw(&run, events::PLAN_CREATED), "planner produced a plan");
    assert!(
        saw(&run, events::EXEC_ACTION),
        "supervisor dispatched an action"
    );
    assert!(saw(&run, events::TASK_COMPLETED), "task completed");
    assert!(
        saw(&run, events::GOAL_COMPLETED),
        "goal completed end-to-end"
    );

    // The MCP capability was actually executed by the shim against the fake server.
    let result = exec_result_for(&run, "mcp:filesystem.read_file")
        .expect("EXEC_RESULT for the mcp capability");
    assert_eq!(
        result.payload.get("success").and_then(|s| s.as_bool()),
        Some(true)
    );
    let output = result.payload.get("output").expect("output present");
    let text = output
        .get("content")
        .and_then(|c| c.get(0))
        .and_then(|c| c.get("text"))
        .and_then(|t| t.as_str())
        .expect("text content");
    assert!(
        text.contains("Hello FRIDAY"),
        "fake MCP server's response should flow back through the slice: {text}"
    );
    assert!(
        text.contains("README.md"),
        "tool arguments should reach the fake server: {text}"
    );
}

/// A server that answers `initialize` + `tools/list` then **drops the
/// connection** (simulating a crash/network failure). Used to verify the client
/// surfaces a transport error on `call_tool` rather than hanging.
async fn spawn_crashing_server() -> FakeTransport {
    let (client_side, mut server_side) = FakeTransport::pair();
    tokio::spawn(async move {
        // Answer `initialize` (echo the client's id) and `tools/list`, so the
        // kernel can register the adapter. Then crash: drop the transport so any
        // later request (e.g. tools/call) fails to get a response — simulating a
        // server that dies after the handshake.
        while let Ok(frame) = server_side.recv().await {
            let v: serde_json::Value = match serde_json::from_str(&frame) {
                Ok(v) => v,
                Err(_) => break,
            };
            let id = v.get("id").cloned().unwrap_or(serde_json::Value::Null);
            let method = v.get("method").and_then(|m| m.as_str()).unwrap_or("");
            let response = match method {
                "initialize" => serde_json::json!({
                    "jsonrpc": "2.0", "id": id,
                    "result": { "protocolVersion": "2024-11-05", "capabilities": {}, "serverInfo": {} }
                }),
                "tools/list" => {
                    serde_json::json!({ "jsonrpc": "2.0", "id": id, "result": { "tools": [] } })
                }
                _ => {
                    // Anything else (e.g. tools/call): crash.
                    break;
                }
            };
            let msg = serde_json::to_string(&response).unwrap();
            if server_side.send(&msg).await.is_err() {
                break;
            }
        }
        // Drop `server_side` -> connection closed.
    });
    client_side
}

#[tokio::test]
async fn mcp_server_disappears_mid_call_fails_cleanly() {
    // Planner picks the capability; supervisor dispatches; the real AgentKernel
    // (via the McpAdapter) calls a server that has already gone away; the call
    // must fail (not hang) and surface as an unsuccessful EXEC_RESULT. No shim,
    // no CapabilitySource branching.
    let mut friday = Friday::new();
    let backend = Some(Box::new(MockBackend::new(
        json!({ "tasks": [ { "name": "Read", "capability": "mcp:fs.read_file", "params": {}, "depends_on_indices": [] } ] })
            .to_string(),
    )) as Box<_>);
    let registry = register_engines_with_llm(&mut friday, backend)
        .await
        .unwrap();

    let tool = friday_mcp::protocol::McpTool {
        name: "read_file".to_string(),
        description: "Read".to_string(),
        input_schema: json!({}),
    };
    register_server_capabilities("fs", std::slice::from_ref(&tool), &registry);

    let client_side = spawn_crashing_server().await;
    let kernel = friday_actions::runtime::AgentKernel::new();
    kernel
        .register_mcp_server("fs", Box::new(client_side))
        .await
        .expect("mcp server registers before the crash");
    friday.register_plugin(Box::new(kernel)).await.unwrap();

    let bus = friday.bus().clone();
    let mut rx = bus.subscribe();
    bus.publish(Event::new(
        events::GOAL_RAW,
        "user",
        json!({ "text": "read" }),
    ))
    .await
    .unwrap();
    let handle = tokio::spawn(async move {
        friday
            .run_with_timeout(std::time::Duration::from_secs(10))
            .await
    });

    let mut collected = Vec::new();
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(3);
    while tokio::time::Instant::now() < deadline {
        if let Ok(Ok(ev)) =
            tokio::time::timeout(std::time::Duration::from_millis(100), rx.recv()).await
        {
            collected.push(ev);
        }
    }
    let _ = handle.await.unwrap();

    // The failed call is reported (not silently dropped, not hung).
    let result = collected
        .iter()
        .find(|e| {
            e.kind == events::EXEC_RESULT
                && e.payload.get("capability").and_then(|c| c.as_str()) == Some("mcp:fs.read_file")
        })
        .expect("EXEC_RESULT for the crashed server");
    assert_eq!(
        result.payload.get("success").and_then(|s| s.as_bool()),
        Some(false),
        "a vanished server must fail the call, not hang"
    );
}

#[tokio::test]
async fn mcp_health_recovers_via_probe() {
    // Health ownership lives in the probe, not discovery. A probe watching a
    // server that goes down then comes back must flip Healthy -> Offline ->
    // Healthy, and the registry (hence planner search) must reflect it. A single
    // persistent in-process MCP WebSocket server is used so "up" is reliable
    // across probe cycles; the factory returns `None` to simulate the outage.
    use friday_capabilities::spec::CapabilityHealth;
    use friday_mcp::discovery::start_health_probe;
    use friday_mcp::protocol::McpTool;
    use futures::{SinkExt, StreamExt};
    use tokio::net::TcpListener;
    use tokio_tungstenite::{accept_async, tungstenite::Message};

    let registry = std::sync::Arc::new(
        friday_planner::execution::capability_registry::CapabilityRegistry::new(),
    );
    let id = "mcp:fs.read_file".to_string();
    registry.register(friday_mcp::adapter::tool_to_spec(
        "fs",
        &McpTool {
            name: "read_file".into(),
            description: "Read".into(),
            input_schema: json!({}),
        },
    ));

    // Persistent fake MCP server over WebSocket.
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        loop {
            let (sock, _) = listener.accept().await.unwrap();
            tokio::spawn(async move {
                let mut ws = match accept_async(sock).await {
                    Ok(ws) => ws,
                    Err(_) => return,
                };
                while let Some(Ok(msg)) = ws.next().await {
                    let text = match msg {
                        Message::Text(t) => t.to_string(),
                        Message::Close(_) => break,
                        _ => continue,
                    };
                    let v: serde_json::Value = match serde_json::from_str(&text) {
                        Ok(v) => v,
                        Err(_) => continue,
                    };
                    let rid = v.get("id").cloned().unwrap_or(serde_json::Value::Null);
                    let method = v.get("method").and_then(|x| x.as_str()).unwrap_or("");
                    let response = match method {
                        "initialize" => serde_json::json!({
                            "jsonrpc": "2.0", "id": rid,
                            "result": { "protocolVersion": "2024-11-05", "capabilities": {}, "serverInfo": {} }
                        }),
                        "tools/list" => {
                            serde_json::json!({ "jsonrpc": "2.0", "id": rid, "result": { "tools": [] } })
                        }
                        _ => {
                            serde_json::json!({ "jsonrpc": "2.0", "id": rid, "error": { "code": -32601, "message": "x" } })
                        }
                    };
                    let _ = ws
                        .send(Message::Text(serde_json::to_string(&response).unwrap()))
                        .await;
                }
            });
        }
    });

    let url = format!("ws://{addr}/mcp");
    // 0 = up, 1 = down, 2 = up again.
    let state = std::sync::Arc::new(std::sync::atomic::AtomicU8::new(0));
    let s = state.clone();
    let handle = start_health_probe(
        "fs".to_string(),
        registry.clone(),
        move || {
            let s = s.clone();
            let url = url.clone();
            async move {
                if s.load(std::sync::atomic::Ordering::SeqCst) == 1 {
                    return None;
                }
                friday_mcp::transport_ws::websocket_transport_for(&url)
                    .await
                    .ok()
            }
        },
        vec![id.clone()],
        std::time::Duration::from_millis(30),
    );

    tokio::time::sleep(std::time::Duration::from_millis(120)).await;
    assert_eq!(registry.health_of(&id), CapabilityHealth::Healthy);
    state.store(1, std::sync::atomic::Ordering::SeqCst);
    tokio::time::sleep(std::time::Duration::from_millis(120)).await;
    assert_eq!(registry.health_of(&id), CapabilityHealth::Offline);
    state.store(2, std::sync::atomic::Ordering::SeqCst);
    tokio::time::sleep(std::time::Duration::from_millis(120)).await;
    assert_eq!(registry.health_of(&id), CapabilityHealth::Healthy);
    handle.abort();
}

#[test]
fn mcp_duplicate_servers_have_distinct_provenance() {
    // Two servers exposing the same tool name must produce two capabilities with
    // distinct ids/provenance, so the planner can reason about them separately.
    use friday_mcp::protocol::McpTool;
    let registry = friday_planner::execution::capability_registry::CapabilityRegistry::new();
    let tool = McpTool {
        name: "read_file".into(),
        description: "Read".into(),
        input_schema: json!({}),
    };
    register_server_capabilities("fsA", std::slice::from_ref(&tool), &registry);
    register_server_capabilities("fsB", &[tool], &registry);

    let a = registry.resolve("mcp:fsA.read_file").expect("fsA resolves");
    let b = registry.resolve("mcp:fsB.read_file").expect("fsB resolves");
    // Distinct provenance is carried in the adapter routing key (mcp:<server>).
    assert_eq!(a.adapter, "mcp:fsA");
    assert_eq!(b.adapter, "mcp:fsB");
    assert_ne!(a.adapter, b.adapter);
}

#[tokio::test]
async fn mcp_high_risk_tool_requires_approval() {
    // A capability explicitly marked High must pause for approval rather than be
    // dispatched — even though name-based inference would call it Medium.
    use friday_capabilities::spec::{CapabilitySource, CapabilitySpec, ExecutionBinding, Risk};

    let mut friday = Friday::new();
    let backend = Some(Box::new(MockBackend::new(
        json!({ "tasks": [ { "name": "Delete", "capability": "mcp:fs.delete_dir", "params": {}, "depends_on_indices": [] } ] })
            .to_string(),
    )) as Box<_>);
    let registry = register_engines_with_llm(&mut friday, backend)
        .await
        .unwrap();

    let spec = CapabilitySpec {
        id: "mcp:fs.delete_dir".to_string(),
        name: "delete_dir".to_string(),
        description: "Delete a directory".to_string(),
        source: CapabilitySource::Mcp("fs".to_string()),
        binding: ExecutionBinding {
            adapter: "mcp:fs".to_string(),
            action: "delete_dir".to_string(),
            schema: json!({}),
            permissions: vec![],
            risk: Risk::High,
        },
    };
    registry.register(spec);

    // No executor needed: a High-risk task must pause before any dispatch.
    let bus = friday.bus().clone();
    let mut rx = bus.subscribe();
    bus.publish(Event::new(
        events::GOAL_RAW,
        "user",
        json!({ "text": "delete dir" }),
    ))
    .await
    .unwrap();
    let handle = tokio::spawn(async move {
        friday
            .run_with_timeout(std::time::Duration::from_secs(10))
            .await
    });

    let mut collected = Vec::new();
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(3);
    while tokio::time::Instant::now() < deadline {
        if let Ok(Ok(ev)) =
            tokio::time::timeout(std::time::Duration::from_millis(100), rx.recv()).await
        {
            collected.push(ev);
        }
    }
    let _ = handle.await.unwrap();

    // Supervisor must pause (require approval) and must NOT dispatch the action.
    assert!(
        collected
            .iter()
            .any(|e| e.kind == events::PERMISSION_PAUSED || e.kind == events::GOAL_PAUSED),
        "high-risk MCP capability should pause for approval"
    );
    assert!(
        !collected.iter().any(|e| e.kind == events::EXEC_ACTION
            && e.payload.get("capability").and_then(|c| c.as_str()) == Some("mcp:fs.delete_dir")),
        "high-risk capability must not be dispatched without approval"
    );
}
