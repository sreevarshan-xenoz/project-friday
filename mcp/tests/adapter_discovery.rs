//! PR2/PR3 hermetic tests: adapter + dynamic discovery + health, no Node.

use std::sync::Arc;

use friday_capabilities::spec::{CapabilityHealth, CapabilitySource};
use friday_mcp::adapter::{capability_id, register_server_capabilities, tool_to_spec};
use friday_mcp::discovery::{discover_and_register, filesystem_server_config};
use friday_mcp::protocol::McpTool;
use friday_mcp::server::{FakeMcpServer, FakeTransport};
use friday_planner::execution::capability_registry::CapabilityRegistry;

fn fake_server_with_tools() -> FakeMcpServer {
    FakeMcpServer::new().with_tools(vec![
        McpTool {
            name: "read_file".into(),
            description: "Read a file".into(),
            input_schema: serde_json::json!({ "type": "object" }),
        },
        McpTool {
            name: "write_file".into(),
            description: "Write a file".into(),
            input_schema: serde_json::json!({ "type": "object" }),
        },
    ])
}

#[test]
fn tool_to_spec_carries_mcp_provenance_and_risk() {
    let tool = McpTool {
        name: "read_file".into(),
        description: "Read a file".into(),
        input_schema: serde_json::json!({}),
    };
    let spec = tool_to_spec("filesystem", &tool);
    assert_eq!(spec.id, "mcp:filesystem.read_file");
    assert!(matches!(spec.source, CapabilitySource::Mcp(ref s) if s == "filesystem"));
    // Reads are inferred Safe; writes Medium.
    assert_eq!(spec.binding.risk, friday_capabilities::spec::Risk::Safe);
    assert_eq!(spec.binding.adapter, "mcp:filesystem");
    assert_eq!(spec.binding.action, "read_file");

    let write = tool_to_spec("filesystem", &McpTool {
        name: "write_file".into(),
        description: "Write".into(),
        input_schema: serde_json::json!({}),
    });
    assert_eq!(write.binding.risk, friday_capabilities::spec::Risk::Medium);
}

#[test]
fn register_server_capabilities_populates_registry() {
    let reg = CapabilityRegistry::new();
    let tools = fake_server_with_tools().tools();
    let specs = register_server_capabilities("filesystem", &tools, &reg);
    assert_eq!(specs.len(), 2);
    assert!(reg.resolve("mcp:filesystem.read_file").is_some());
    assert!(reg.resolve("mcp:filesystem.write_file").is_some());
    // Discovery surfaces them like any native capability.
    let ranked = reg.search("read a file from filesystem", 10);
    assert!(ranked.iter().any(|r| r.spec.id == "mcp:filesystem.read_file"));
}

#[tokio::test]
async fn discover_and_register_via_fake_server() {
    let (client_side, server_side) = FakeTransport::pair();
    let server = fake_server_with_tools();
    tokio::spawn(async move { server.serve(server_side).await });

    let reg = CapabilityRegistry::new();
    let tools = discover_and_register("filesystem", client_side, &reg)
        .await
        .expect("discovery succeeds");
    assert_eq!(tools.len(), 2);
    assert!(reg.resolve("mcp:filesystem.read_file").is_some());
    // Health defaults to Healthy on registration.
    assert_eq!(
        reg.health_of("mcp:filesystem.read_file"),
        CapabilityHealth::Healthy
    );
}

#[test]
fn capability_id_format_matches_source_tag() {
    assert_eq!(capability_id("vscode", "open"), "mcp:vscode.open");
    // The id prefix aligns with the CapabilitySource::Mcp tag.
    let spec = tool_to_spec("vscode", &McpTool {
        name: "open".into(),
        description: "".into(),
        input_schema: serde_json::json!({}),
    });
    assert_eq!(spec.source.tag(), "mcp:vscode");
}

#[test]
fn filesystem_config_is_well_formed() {
    let cfg = filesystem_server_config("/tmp");
    assert_eq!(cfg.name, "filesystem");
    match cfg.transport {
        friday_mcp::discovery::McpTransportConfig::Stdio { command, args } => {
            assert_eq!(command, "npx");
            assert!(args.iter().any(|a| a.contains("server-filesystem")));
            assert!(args.iter().any(|a| a == "/tmp"));
        }
        _ => panic!("expected stdio config"),
    }
    let _ = Arc::new(CapabilityRegistry::new()); // ensure types line up
}

#[tokio::test]
async fn register_configured_servers_returns_ids_for_supervision() {
    // Discovery returns one RegisteredServer per successful server, carrying the
    // capability ids the integration layer needs to start health probes. It does
    // NOT start probes itself. Uses a self-contained in-process MCP WebSocket
    // server so register_configured_servers can be driven through its real
    // (Remote) transport path without Node.
    use friday_mcp::discovery::{register_configured_servers, McpServerConfig, McpTransportConfig};
    use futures::{SinkExt, StreamExt};
    use tokio::net::TcpListener;
    use tokio_tungstenite::{accept_async, tungstenite::Message};

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let (sock, _) = listener.accept().await.unwrap();
        let mut ws = accept_async(sock).await.unwrap();
        let tools = fake_server_with_tools().tools();
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
            let id = v.get("id").cloned().unwrap_or(serde_json::Value::Null);
            let method = v.get("method").and_then(|x| x.as_str()).unwrap_or("");
            let response = match method {
                "initialize" => serde_json::json!({
                    "jsonrpc": "2.0", "id": id,
                    "result": { "protocolVersion": "2024-11-05", "capabilities": {}, "serverInfo": { "name": "fake", "version": "0" } }
                }),
                "tools/list" => {
                    let list: Vec<serde_json::Value> = tools
                        .iter()
                        .map(|t| serde_json::json!({ "name": t.name, "description": t.description, "inputSchema": t.input_schema }))
                        .collect();
                    serde_json::json!({ "jsonrpc": "2.0", "id": id, "result": { "tools": list } })
                }
                _ => serde_json::json!({ "jsonrpc": "2.0", "id": id, "error": { "code": -32601, "message": "method not found" } }),
            };
            let _ = ws
                .send(Message::Text(serde_json::to_string(&response).unwrap().into()))
                .await;
        }
    });

    let cfg = McpServerConfig {
        name: "filesystem".to_string(),
        transport: McpTransportConfig::Remote {
            url: format!("ws://{addr}/mcp"),
        },
    };
    let reg = CapabilityRegistry::new();
    let registered = register_configured_servers(&reg, &[cfg]).await;
    assert_eq!(registered.len(), 1);
    assert_eq!(registered[0].name, "filesystem");
    assert_eq!(registered[0].capability_ids.len(), 2);
    assert!(registered[0]
        .capability_ids
        .contains(&"mcp:filesystem.read_file".to_string()));
    // After registration, capabilities are resolvable and start Healthy.
    assert!(reg.resolve("mcp:filesystem.read_file").is_some());
    assert_eq!(
        reg.health_of("mcp:filesystem.read_file"),
        CapabilityHealth::Healthy
    );
}

#[tokio::test]
async fn integration_owned_probe_flips_server_offline() {
    // Supervision lives in the integration layer, not discovery. Starting a probe
    // with a transport factory that can no longer connect must flip the server's
    // capabilities to Offline; a healthy factory keeps them Healthy.
    let reg = Arc::new(CapabilityRegistry::new());
    reg.register(tool_to_spec(
        "filesystem",
        &McpTool {
            name: "read_file".into(),
            description: "Read".into(),
            input_schema: serde_json::json!({}),
        },
    ));
    let id = capability_id("filesystem", "read_file");
    assert_eq!(reg.health_of(&id), CapabilityHealth::Healthy);

    // Factory that always reports the server unreachable.
    let handle = friday_mcp::discovery::start_health_probe(
        "filesystem".to_string(),
        Arc::clone(&reg),
        || async { None },
        vec![id.clone()],
        std::time::Duration::from_millis(20),
    );

    // After at least one probe cycle, health must be Offline.
    tokio::time::sleep(std::time::Duration::from_millis(80)).await;
    assert_eq!(reg.health_of(&id), CapabilityHealth::Offline);
    handle.abort();
}
