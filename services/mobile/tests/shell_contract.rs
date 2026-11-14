//! R5-1-1 vertical-slice contract test.
//!
//! Proves the "one controlled command" surface end-to-end over a live
//! WebSocket, exactly as `friday-linux-daemon` wires it:
//!
//!   1. loopback `WsServer` (auth off — the shell runs on the same host)
//!   2. a per-run session token gate (the fine-grained trust boundary)
//!   3. `CommandGateway` translates `planner.goal` into a bus event
//!   4. the accepted command is echoed into a `state.json` snapshot
//!
//! A command WITHOUT the token is rejected and must not reach the bus; a
//! command WITH the token is accepted, publishes `plan.goal` on the bus, and
//! is recorded in `ShellState.command` — the read contract the `friday.shell`
//! plugin renders.

use std::sync::Arc;
use std::time::Duration;

use friday_core::event::EventBus;
use friday_mobile::commands::{method, CommandGateway};
use friday_mobile::protocol::{ClientMessage, ServerMessage};
use friday_mobile::shell_state::{CommandRecord, ShellState};
use friday_mobile::transport::WsServer;
use futures::SinkExt;
use tokio::sync::Mutex;
use tokio_tungstenite::tungstenite::Message;

const PORT: u16 = 18441;
const TOKEN: &str = "test-token-001";

async fn recv_server_msg<S>(ws: &mut S) -> ServerMessage
where
    S: futures::StreamExt<Item = Result<Message, tokio_tungstenite::tungstenite::Error>>
        + futures::SinkExt<Message>,
    S: Unpin,
{
    loop {
        let msg = ws.next().await.unwrap().unwrap();
        if let Message::Text(t) = msg {
            return serde_json::from_str(&t).unwrap();
        }
    }
}

#[tokio::test]
async fn one_controlled_command_end_to_end() {
    let addr: std::net::SocketAddr = format!("127.0.0.1:{PORT}").parse().unwrap();
    let server = WsServer::new(addr).with_require_auth(false);
    let outbound = server.broadcast_tx();
    let sessions = server.sessions();
    let mut ingress = server.subscribe_commands();

    let state = Arc::new(Mutex::new(ShellState::starting(TOKEN.to_string())));

    // The daemon's command loop: token gate -> dispatch -> echo into state.
    let dispatch_state = state.clone();
    let dispatch_token = TOKEN.to_string();
    let cmd_loop = tokio::spawn(async move {
        let bus_for_gateway = EventBus::new();
        let gateway = CommandGateway::new(bus_for_gateway.clone(), sessions.clone());
        let mut bus_sub = bus_for_gateway.subscribe();
        while let Ok(cmd) = ingress.recv().await {
            let presented = cmd
                .params
                .get("token")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            if presented.is_empty() || presented != dispatch_token {
                let _ = outbound.send(ServerMessage::CommandResult {
                    id: cmd.id,
                    ok: false,
                    result: serde_json::json!({ "error": "forbidden: missing or invalid token" }),
                });
                continue;
            }
            let is_goal = cmd.method == method::PLANNER_GOAL;
            let result = gateway.dispatch(&cmd).await;
            // A client sends a goal command; capture the bus event it produced.
            if is_goal {
                if let Ok(Ok(ev)) =
                    tokio::time::timeout(Duration::from_secs(5), bus_sub.recv()).await
                {
                    assert_eq!(ev.kind, "plan.goal");
                    assert_eq!(ev.payload["goal"], "List the files");
                }
                let mut st = dispatch_state.lock().await;
                st.command = Some(CommandRecord {
                    method: cmd.method.clone(),
                    accepted_ms: 0,
                    goal: cmd
                        .params
                        .get("goal")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string()),
                });
            }
            let _ = outbound.send(result);
        }
    });

    tokio::spawn(async move {
        server.run().await.unwrap();
    });
    tokio::time::sleep(Duration::from_millis(200)).await;

    let (mut client, _resp) = tokio_tungstenite::connect_async(format!("ws://127.0.0.1:{PORT}"))
        .await
        .unwrap();
    let _welcome = recv_server_msg(&mut client).await;

    // 1) WITHOUT a token -> rejected, state echo unchanged, no goal event.
    let bad = ClientMessage::Command {
        id: "c-bad".into(),
        method: method::PLANNER_GOAL.into(),
        params: serde_json::json!({ "goal": "should not run" }),
    };
    client
        .send(Message::Text(serde_json::to_string(&bad).unwrap()))
        .await
        .unwrap();
    let rejected = recv_server_msg(&mut client).await;
    match rejected {
        ServerMessage::CommandResult { ok: false, .. } => {}
        other => panic!("expected rejection, got {other:?}"),
    }
    {
        let st = state.lock().await;
        assert!(st.command.is_none(), "rejected command must not be echoed");
    }

    // 2) WITH the token -> accepted, goal event on bus, echoed into state.
    let good = ClientMessage::Command {
        id: "c-good".into(),
        method: method::PLANNER_GOAL.into(),
        params: serde_json::json!({ "goal": "List the files", "token": TOKEN }),
    };
    client
        .send(Message::Text(serde_json::to_string(&good).unwrap()))
        .await
        .unwrap();
    let accepted = recv_server_msg(&mut client).await;
    match accepted {
        ServerMessage::CommandResult { ok: true, .. } => {}
        other => panic!("expected acceptance, got {other:?}"),
    }

    // Give the cmd loop a beat to echo into shared state.
    tokio::time::sleep(Duration::from_millis(300)).await;
    {
        let st = state.lock().await;
        let rec = st.command.as_ref().expect("command echoed into state");
        assert_eq!(rec.method, method::PLANNER_GOAL);
        assert_eq!(rec.goal.as_deref(), Some("List the files"));
    }

    client.close(None).await.unwrap();
    cmd_loop.abort();
}

#[derive(Clone)]
struct BlockingSeam {
    started: Arc<tokio::sync::Notify>,
    release: Arc<tokio::sync::Notify>,
}

#[async_trait::async_trait]
impl friday_mobile::context_read::ProviderSeam for BlockingSeam {
    async fn send(&self, payload: &str) -> Result<String, String> {
        self.send_streaming(payload, &|_| {}).await
    }

    async fn send_streaming(
        &self,
        _payload: &str,
        progress: &(dyn for<'s> Fn(&'s str) + Send + Sync),
    ) -> Result<String, String> {
        progress("analyzing...");
        self.started.notify_one();
        self.release.notified().await;
        Ok("ok".to_string())
    }
}

fn fake_collector(
) -> friday_context_collector::ContextCollector<friday_context_collector::FakeCommandRunner> {
    const PNG: &[u8] = b"\x89PNG\r\n\x1a\nreal-screen-bytes";
    let full = friday_context_collector::screen_command(None);
    let runner = friday_context_collector::FakeCommandRunner::new().script_stdout(
        &full.0,
        &full.1.iter().map(String::as_str).collect::<Vec<_>>(),
        PNG,
    );
    friday_context_collector::ContextCollector::new(runner)
}

#[tokio::test]
async fn screen_ask_concurrent_and_cancellation_end_to_end() {
    use friday_mobile::context_read::{ScreenAskManager, ScreenReader};

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server = WsServer::new(addr).with_require_auth(false);
    let outbound = server.broadcast_tx();
    let sessions = server.sessions();
    let mut ingress = server.subscribe_commands();

    let state = Arc::new(Mutex::new(ShellState::starting(TOKEN.to_string())));
    let state_path = std::env::temp_dir().join(format!("state-{}.json", uuid::Uuid::new_v4()));

    let started = Arc::new(tokio::sync::Notify::new());
    let release = Arc::new(tokio::sync::Notify::new());
    let seam = BlockingSeam {
        started: started.clone(),
        release: release.clone(),
    };
    let screen_reader = Arc::new(ScreenReader::new(fake_collector(), seam, "fabric"));
    let screen_ask = ScreenAskManager::new(screen_reader);

    let dispatch_state = state.clone();
    let dispatch_path = state_path.clone();
    let dispatch_token = TOKEN.to_string();
    let cmd_loop = tokio::spawn(async move {
        let bus = EventBus::new();
        let gateway = CommandGateway::new(bus.clone(), sessions.clone());
        while let Ok(cmd) = ingress.recv().await {
            let presented = cmd
                .params
                .get("token")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            if presented.is_empty() || presented != dispatch_token {
                let _ = outbound.send(ServerMessage::CommandResult {
                    id: cmd.id,
                    ok: false,
                    result: serde_json::json!({ "error": "forbidden: missing or invalid token" }),
                });
                continue;
            }
            if cmd.method == method::CONTEXT_READ {
                if let Err(err_msg) = screen_ask
                    .try_start(
                        &cmd,
                        outbound.clone(),
                        dispatch_state.clone(),
                        dispatch_path.clone(),
                    )
                    .await
                {
                    let _ = outbound.send(err_msg);
                }
                continue;
            }
            if cmd.method == method::TASK_CANCEL {
                let task_id = cmd
                    .params
                    .get("task_id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                screen_ask
                    .cancel(task_id, &outbound, &dispatch_state, &dispatch_path)
                    .await;
            }
            let result = gateway.dispatch(&cmd).await;
            let _ = outbound.send(result);
        }
    });

    tokio::spawn(async move {
        server.run_on(listener).await.unwrap();
    });

    let (mut client, _resp) = tokio_tungstenite::connect_async(format!("ws://{addr}"))
        .await
        .unwrap();
    let _welcome = recv_server_msg(&mut client).await;

    // 1. Send context.read
    let ask_cmd = ClientMessage::Command {
        id: "ask-1".into(),
        method: method::CONTEXT_READ.into(),
        params: serde_json::json!({ "token": TOKEN, "consent": true }),
    };
    client
        .send(Message::Text(serde_json::to_string(&ask_cmd).unwrap()))
        .await
        .unwrap();

    // Wait until ask-1 is running and suspended in provider
    started.notified().await;

    // 2. While ask-1 is in flight, send system.info: must be answered immediately
    let (mut client2, _) = tokio_tungstenite::connect_async(format!("ws://{addr}"))
        .await
        .unwrap();
    let _welcome2 = recv_server_msg(&mut client2).await;
    let info_cmd = ClientMessage::Command {
        id: "info-1".into(),
        method: method::SYSTEM_INFO.into(),
        params: serde_json::json!({ "token": TOKEN }),
    };
    client2
        .send(Message::Text(serde_json::to_string(&info_cmd).unwrap()))
        .await
        .unwrap();

    // client2 receives immediate result for info-1
    let info_res = recv_server_msg(&mut client2).await;
    match info_res {
        ServerMessage::CommandResult { id, ok, .. } => {
            assert_eq!(id, "info-1");
            assert!(ok, "system.info must succeed while screen ask is in flight");
        }
        other => panic!("expected CommandResult for info-1, got {other:?}"),
    }

    // 3. Second concurrent context.read: must be refused immediately
    let ask_cmd2 = ClientMessage::Command {
        id: "ask-2".into(),
        method: method::CONTEXT_READ.into(),
        params: serde_json::json!({ "token": TOKEN, "consent": true }),
    };
    client2
        .send(Message::Text(serde_json::to_string(&ask_cmd2).unwrap()))
        .await
        .unwrap();
    let ask2_res = recv_server_msg(&mut client2).await;
    match ask2_res {
        ServerMessage::CommandResult { id, ok, result } => {
            assert_eq!(id, "ask-2");
            assert!(!ok);
            assert_eq!(result["error"], "FRIDAY is already looking at your screen");
        }
        other => panic!("expected CommandResult for ask-2, got {other:?}"),
    }

    // 4. Send task.cancel to stop ask-1
    let cancel_cmd = ClientMessage::Command {
        id: "cancel-1".into(),
        method: method::TASK_CANCEL.into(),
        params: serde_json::json!({ "token": TOKEN, "task_id": "ask-1" }),
    };
    client2
        .send(Message::Text(serde_json::to_string(&cancel_cmd).unwrap()))
        .await
        .unwrap();

    // client2 gets cancel ack
    loop {
        let msg = recv_server_msg(&mut client2).await;
        if let ServerMessage::CommandResult { id, ok, .. } = msg {
            if id == "cancel-1" {
                assert!(ok);
                break;
            }
        }
    }

    // client1 receives CommandResult for ask-1 with cancelled shape
    loop {
        let msg = recv_server_msg(&mut client).await;
        if let ServerMessage::CommandResult { id, ok, result } = msg {
            if id == "ask-1" {
                assert!(!ok);
                assert_eq!(result["error"], "cancelled_by_human");
                break;
            }
        }
    }

    let _ = std::fs::remove_file(&state_path);
    cmd_loop.abort();
}
