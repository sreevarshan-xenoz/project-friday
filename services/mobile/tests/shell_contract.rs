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

use futures::SinkExt;
use friday_core::event::EventBus;
use friday_mobile::commands::{method, CommandGateway};
use friday_mobile::shell_state::{CommandRecord, ShellState};
use friday_mobile::transport::WsServer;
use friday_mobile::protocol::{ClientMessage, ServerMessage};
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
                if let Ok(Ok(ev)) = tokio::time::timeout(
                    Duration::from_secs(5),
                    bus_sub.recv(),
                ).await {
                    assert_eq!(ev.kind, "plan.goal");
                    assert_eq!(ev.payload["goal"], "List the files");
                }
                let mut st = dispatch_state.lock().await;
                st.command = Some(CommandRecord {
                    method: cmd.method.clone(),
                    accepted_ms: 0,
                    goal: cmd.params.get("goal").and_then(|v| v.as_str()).map(|s| s.to_string()),
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
