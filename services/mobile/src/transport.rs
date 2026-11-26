//! WebSocket transport for the mobile console (M6.1).
//!
//! `WsServer` is the single phone-facing surface. It:
//! - accepts WebSocket connections,
//! - performs the QR-pair handshake (via [`AuthRegistry`]),
//! - forwards inbound [`ClientMessage::Command`] to the FRIDAY app through an mpsc
//!   channel ([`WsServer::subscribe_commands`]),
//! - relays outbound [`ServerMessage`]s (events, telemetry, health) to every
//!   authenticated client via a `broadcast` channel ([`WsServer::broadcast`]).

use std::collections::HashSet;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use futures::{SinkExt, StreamExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::broadcast;
use tokio_tungstenite::tungstenite::handshake::server::{ErrorResponse, Request, Response};
use tokio_tungstenite::tungstenite::http::StatusCode;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::WebSocketStream;
use uuid::Uuid;

use crate::auth::AuthRegistry;
use crate::protocol::{self, ClientMessage, ServerMessage};
use crate::session::SessionRegistry;

/// A command a phone issued, forwarded to the FRIDAY app for handling.
#[derive(Debug, Clone)]
pub struct Command {
    pub session_id: String,
    pub id: String,
    pub method: String,
    pub params: serde_json::Value,
}

/// The mobile WebSocket gateway.
pub struct WsServer {
    addr: SocketAddr,
    require_auth: bool,
    auth: Arc<AuthRegistry>,
    outbound: broadcast::Sender<ServerMessage>,
    commands: broadcast::Sender<Command>,
    sessions: Arc<SessionRegistry>,
    client_token: Option<Arc<str>>,
}

impl WsServer {
    pub fn new(addr: SocketAddr) -> Self {
        let (outbound, _) = broadcast::channel(256);
        let (commands, _) = broadcast::channel(256);
        Self {
            addr,
            // Default: QR-pairing required (the phone-facing protocol contract).
            // Local development can opt out via `with_require_auth(false)`.
            require_auth: true,
            auth: Arc::new(AuthRegistry::new()),
            outbound,
            commands,
            sessions: SessionRegistry::new(),
            client_token: None,
        }
    }

    /// Loopback mode's gate for what FRIDAY sends out (questions, page and
    /// screen text, answers): a client gets nothing until it has sent a
    /// command carrying this token (friday-ctl always does), and a browser
    /// (any handshake with an `Origin` header) is refused, so neither a web
    /// page nor another local program can listen in.
    pub fn with_client_token(mut self, token: impl Into<String>) -> Self {
        self.client_token = Some(Arc::from(token.into()));
        self
    }

    /// Configure whether the handshake demands QR-pairing. `false` = open mode
    /// for local development (the `welcome` advertises `requires_auth: false`
    /// and unauthenticated clients go straight to the relay phase).
    pub fn with_require_auth(mut self, require_auth: bool) -> Self {
        self.require_auth = require_auth;
        self
    }

    /// The live session registry (so the daemon can inspect connected devices).
    pub fn sessions(&self) -> Arc<SessionRegistry> {
        self.sessions.clone()
    }

    /// Access the auth registry (e.g. to begin a QR pairing from the PC side).
    pub fn auth(&self) -> &AuthRegistry {
        &self.auth
    }

    /// Begin a QR pairing on the PC; returns the challenge to render as a QR.
    pub fn begin_pairing(&self) -> String {
        self.auth.begin_pairing()
    }

    /// Receive commands issued by connected phones.
    pub fn subscribe_commands(&self) -> broadcast::Receiver<Command> {
        self.commands.subscribe()
    }

    /// Broadcast a server message to all authenticated clients.
    pub fn broadcast(&self, msg: ServerMessage) {
        let _ = self.outbound.send(msg);
    }

    /// A cloneable sender for outbound messages (e.g. for tests or other tasks
    /// that outlive the server handle).
    pub fn broadcast_tx(&self) -> broadcast::Sender<ServerMessage> {
        self.outbound.clone()
    }

    /// Bind and serve forever.
    pub async fn run(self) -> Result<(), crate::error::MobileError> {
        let listener = TcpListener::bind(self.addr).await.map_err(|e| {
            crate::error::MobileError::Transport(format!("bind {}: {}", self.addr, e))
        })?;
        self.run_on(listener).await
    }

    /// Serve forever on a listener the caller already bound (so it can learn
    /// the real address first, e.g. after binding port 0).
    pub async fn run_on(self, listener: TcpListener) -> Result<(), crate::error::MobileError> {
        let local = listener
            .local_addr()
            .map(|a| a.to_string())
            .unwrap_or_else(|_| self.addr.to_string());
        tracing::info!("mobile ws listening on {local}");
        loop {
            let (sock, peer) = listener
                .accept()
                .await
                .map_err(|e| crate::error::MobileError::Transport(format!("accept: {}", e)))?;
            let auth = self.auth.clone();
            let outbound = self.outbound.clone();
            let commands = self.commands.clone();
            let sessions = self.sessions.clone();
            let require_auth = self.require_auth;
            let client_token = self.client_token.clone();
            tokio::spawn(async move {
                if let Err(e) = handle_conn(
                    sock,
                    auth,
                    require_auth,
                    client_token,
                    outbound,
                    commands,
                    sessions,
                )
                .await
                {
                    tracing::debug!("conn {peer} ended: {e}");
                }
            });
        }
    }
}

type WsStream = WebSocketStream<TcpStream>;

/// Takes the connection's session out of the registry however the connection ends: a
/// dropped network or a client that just exits ends it with an error, not a close frame.
struct SessionGuard {
    sessions: Arc<SessionRegistry>,
    id: String,
}

impl Drop for SessionGuard {
    fn drop(&mut self) {
        self.sessions.remove(&self.id);
    }
}

// (The Origin check's callback type is tungstenite's, large error and all.)
#[allow(clippy::result_large_err)]
async fn handle_conn(
    sock: TcpStream,
    auth: Arc<AuthRegistry>,
    require_auth: bool,
    client_token: Option<Arc<str>>,
    outbound: broadcast::Sender<ServerMessage>,
    commands: broadcast::Sender<Command>,
    sessions: Arc<SessionRegistry>,
) -> Result<(), crate::error::MobileError> {
    let mut ws: WsStream = if client_token.is_some() {
        // Browsers always send Origin; FRIDAY's own clients never do.
        let refuse_browsers = |req: &Request, resp: Response| -> Result<Response, ErrorResponse> {
            if req.headers().contains_key("origin") {
                let mut no = ErrorResponse::new(Some("browsers can't connect".into()));
                *no.status_mut() = StatusCode::FORBIDDEN;
                return Err(no);
            }
            Ok(resp)
        };
        tokio_tungstenite::accept_hdr_async(sock, refuse_browsers).await?
    } else {
        tokio_tungstenite::accept_async(sock).await?
    };
    let session_id = Uuid::new_v4().to_string();
    sessions.insert(crate::session::MobileSession::new(session_id.clone()));
    let _registered = SessionGuard {
        sessions: sessions.clone(),
        id: session_id.clone(),
    };

    send(
        &mut ws,
        ServerMessage::Welcome {
            session_id: session_id.clone(),
            requires_auth: require_auth,
            protocol: crate::protocol::PROTOCOL_VERSION.to_string(),
        },
    )
    .await?;

    // Open mode (local dev): the client is trusted from the welcome — it goes
    // straight to the relay phase (and its session is marked authenticated so
    // downstream gates treat it like any paired phone). Pairing mode: handshake
    // until authenticated.
    let mut authed = !require_auth;
    if authed {
        sessions.update(&session_id, |s| s.authenticated = true);
    }
    let mut topics: HashSet<String> = HashSet::new();
    let mut conn_challenge: Option<String> = None;

    // --- handshake phase: read until authenticated ---
    while !authed {
        let msg = match ws.next().await {
            Some(Ok(m)) => m,
            Some(Err(e)) => return Err(e.into()),
            None => {
                sessions.remove(&session_id);
                return Ok(());
            }
        };
        match msg {
            Message::Text(t) => match protocol::decode(&t) {
                Ok(ClientMessage::Hello { client, token, .. }) => {
                    record_device(&sessions, &session_id, &client);
                    if let Some(tok) = token {
                        if auth.validate_token(&tok) {
                            authed = true;
                            sessions.update(&session_id, |s| s.authenticated = true);
                        } else {
                            send(
                                &mut ws,
                                ServerMessage::Error {
                                    code: "auth".into(),
                                    message: "invalid token".into(),
                                },
                            )
                            .await?;
                        }
                    } else {
                        // Unpaired device: the pairing code goes to the PC's screen,
                        // never down this socket. The phone gets only a challenge,
                        // signs it with the code its user types, and answers.
                        if let Some(code) = auth.ensure_pairing() {
                            tracing::warn!("a phone is waiting to pair: enter code {code} on it (valid 2 minutes)");
                        }
                        let challenge = auth.new_challenge();
                        send(
                            &mut ws,
                            ServerMessage::AuthChallenge {
                                challenge: challenge.clone(),
                            },
                        )
                        .await?;
                        conn_challenge = Some(challenge);
                    }
                }
                Ok(ClientMessage::AuthResponse { code, signature }) => {
                    let Some(challenge) = conn_challenge.as_deref() else {
                        send(
                            &mut ws,
                            ServerMessage::Error {
                                code: "bad_request".into(),
                                message: "say hello before answering".into(),
                            },
                        )
                        .await?;
                        continue;
                    };
                    match auth.complete_pairing(&code, challenge, &signature) {
                        Ok(token) => {
                            authed = true;
                            sessions.update(&session_id, |s| s.authenticated = true);
                            send(&mut ws, ServerMessage::AuthComplete { token }).await?;
                        }
                        Err(e) => {
                            send(
                                &mut ws,
                                ServerMessage::Error {
                                    code: e.code().into(),
                                    message: e.message().into(),
                                },
                            )
                            .await?;
                        }
                    }
                }
                Ok(ClientMessage::Ping) => send(&mut ws, ServerMessage::Pong).await?,
                Ok(_) => { /* ignore other client messages pre-auth */ }
                Err(e) => {
                    send(
                        &mut ws,
                        ServerMessage::Error {
                            code: "bad_request".into(),
                            message: e.to_string(),
                        },
                    )
                    .await?;
                }
            },
            Message::Ping(_) => send(&mut ws, ServerMessage::Pong).await?,
            Message::Close(_) => {
                sessions.remove(&session_id);
                return Ok(());
            }
            _ => {}
        }
    }

    // --- relay phase: client <-> FRIDAY ---
    let mut rx = outbound.subscribe();
    // With a client token, nothing goes out until the client proved it has it.
    let mut trusted = client_token.is_none();
    let mut ping = tokio::time::interval(Duration::from_secs(30));
    loop {
        tokio::select! {
            incoming = ws.next() => {
                let msg = match incoming {
                    Some(Ok(m)) => m,
                    Some(Err(e)) => return Err(e.into()),
                    None => {
                        sessions.remove(&session_id);
                        return Ok(());
                    }
                };
                match msg {
                    Message::Text(t) => {
                        if !trusted {
                            if let (Some(want), Ok(ClientMessage::Command { params, .. })) =
                                (client_token.as_deref(), protocol::decode(&t))
                            {
                                trusted = params.get("token").and_then(|v| v.as_str()) == Some(want);
                            }
                        }
                        handle_client_text(
                            &mut ws,
                            &session_id,
                            &commands,
                            &sessions,
                            &mut topics,
                            &t,
                        )
                        .await?
                    }
                    Message::Ping(_) => send(&mut ws, ServerMessage::Pong).await?,
                    Message::Close(_) => {
                        sessions.remove(&session_id);
                        return Ok(());
                    }
                    _ => {}
                }
            }
            outgoing = rx.recv() => {
                match outgoing {
                    Ok(sm) if trusted => {
                        // Topic routing is a later refinement; for Alpha we forward
                        // all outbound server messages to the authenticated client.
                        send(&mut ws, sm).await?;
                    }
                    Ok(_) => { /* not trusted (yet): nothing goes out */ }
                    Err(_) => { /* all senders gone; harmless */ }
                }
            }
            _ = ping.tick() => {
                ws.send(Message::Ping(vec![])).await?;
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
async fn handle_client_text(
    ws: &mut WsStream,
    session_id: &str,
    commands: &broadcast::Sender<Command>,
    sessions: &Arc<SessionRegistry>,
    topics: &mut HashSet<String>,
    text: &str,
) -> Result<(), crate::error::MobileError> {
    match protocol::decode(text) {
        Ok(ClientMessage::Command { id, method, params }) => {
            let task_id = params
                .get("task_id")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            if commands
                .send(Command {
                    session_id: session_id.to_string(),
                    id,
                    method: method.clone(),
                    params,
                })
                .is_err()
            {
                send(
                    ws,
                    ServerMessage::Error {
                        code: "internal".into(),
                        message: "command channel closed".into(),
                    },
                )
                .await?;
            }
            // Track active tasks so the session model stays current.
            if let Some(tid) = task_id {
                if method == "task.cancel" || method == "task.pause" {
                    sessions.update(session_id, |s| {
                        s.active_tasks.remove(&tid);
                    });
                } else if method == "task.approve" {
                    sessions.update(session_id, |s| {
                        s.pending_approvals.retain(|t| t != &tid);
                    });
                }
            }
        }
        Ok(ClientMessage::Subscribe { topics: t }) => {
            *topics = t.into_iter().collect();
            let subs = topics.clone();
            sessions.update(session_id, |s| s.subscribed_events = subs);
        }
        Ok(ClientMessage::Ping) => send(ws, ServerMessage::Pong).await?,
        Ok(ClientMessage::Hello { client, .. }) => {
            // Open-mode sessions skip the handshake loop, so the hello arrives
            // here — capture device metadata just like the pairing path does.
            record_device(sessions, session_id, &client);
        }
        Ok(ClientMessage::AuthResponse { .. } | ClientMessage::CommandAck { .. }) => {
            // no-op post-auth
        }
        Err(e) => {
            send(
                ws,
                ServerMessage::Error {
                    code: "bad_request".into(),
                    message: e.to_string(),
                },
            )
            .await?;
        }
    }
    Ok(())
}

async fn send(ws: &mut WsStream, msg: ServerMessage) -> Result<(), crate::error::MobileError> {
    let text = protocol::encode(&msg).map_err(crate::error::MobileError::Json)?;
    ws.send(Message::Text(text)).await?;
    Ok(())
}

/// Record the device identity a client announced in its `Hello`.
fn record_device(
    sessions: &Arc<SessionRegistry>,
    session_id: &str,
    client: &crate::protocol::ClientInfo,
) {
    sessions.update(session_id, |s| {
        s.device = client.device.clone();
        s.model = client.model.clone();
        s.app_version = client.app_version.clone();
        if !client.device.is_empty() {
            s.phone_id = client.device.clone();
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::sign_pairing;
    use crate::protocol::{ClientInfo, ClientMessage, ServerMessage};
    use futures::{SinkExt, Stream, StreamExt};
    use tokio_tungstenite::tungstenite::Message;

    async fn recv_server_msg<S>(ws: &mut S) -> ServerMessage
    where
        S: Stream<Item = Result<Message, tokio_tungstenite::tungstenite::Error>> + Unpin,
    {
        loop {
            match ws.next().await {
                Some(Ok(Message::Text(t))) => {
                    return protocol::decode_server(&t).expect("valid server message")
                }
                Some(Ok(_)) => continue,
                _ => panic!("connection closed while expecting a server message"),
            }
        }
    }

    type Client = tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<TcpStream>>;

    async fn send_client(ws: &mut Client, msg: &ClientMessage) {
        ws.send(Message::Text(serde_json::to_string(msg).unwrap()))
            .await
            .unwrap();
    }

    /// An unpaired phone: Hello, read the challenge frame (raw text, to see
    /// everything on the wire), answer it with the code the user typed.
    async fn pair_with(ws: &mut Client, code: &str) -> (String, ServerMessage) {
        let hello = ClientMessage::Hello {
            client: ClientInfo {
                device: "test".into(),
                model: "test".into(),
                app_version: "0".into(),
            },
            token: None,
            protocol: protocol::PROTOCOL_VERSION.to_string(),
        };
        send_client(ws, &hello).await;
        let raw = loop {
            if let Some(Ok(Message::Text(t))) = ws.next().await {
                break t;
            }
        };
        let ServerMessage::AuthChallenge { challenge } = protocol::decode_server(&raw).unwrap()
        else {
            panic!("expected a challenge, got {raw}");
        };
        let answer = ClientMessage::AuthResponse {
            code: code.into(),
            signature: sign_pairing(&challenge, code),
        };
        send_client(ws, &answer).await;
        (raw, recv_server_msg(ws).await)
    }

    #[tokio::test]
    async fn the_pairing_code_never_goes_down_the_socket() {
        let server = WsServer::new("127.0.0.1:0".parse().unwrap());
        let code = server.begin_pairing();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("ws://{}", listener.local_addr().unwrap());
        tokio::spawn(async move {
            let _ = server.run_on(listener).await;
        });

        let (mut phone, _) = tokio_tungstenite::connect_async(url.as_str())
            .await
            .unwrap();
        let _welcome = recv_server_msg(&mut phone).await;
        // Someone who never saw the PC's screen can't pair, however often they say hello.
        let (raw, reply) = pair_with(&mut phone, "00000000").await;
        assert!(
            !raw.contains(&code),
            "the challenge frame leaked the code: {raw}"
        );
        assert!(!raw.contains("pairing_code"), "{raw}");
        assert!(matches!(reply, ServerMessage::Error { .. }), "{reply:?}");
        // The user who read it off the PC can.
        let (_, reply) = pair_with(&mut phone, &code).await;
        assert!(
            matches!(reply, ServerMessage::AuthComplete { .. }),
            "{reply:?}"
        );
    }

    #[tokio::test]
    async fn full_handshake_command_and_broadcast() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = WsServer::new(addr);
        let mut cmd_rx = server.subscribe_commands();
        let outbound = server.broadcast_tx();
        let code = server.begin_pairing();

        tokio::spawn(async move {
            server.run_on(listener).await.unwrap();
        });
        tokio::time::sleep(Duration::from_millis(150)).await;

        let url = format!("ws://{addr}");
        let (mut client, _resp) = tokio_tungstenite::connect_async(url).await.unwrap();

        // 1. Welcome
        let welcome = recv_server_msg(&mut client).await;
        match welcome {
            ServerMessage::Welcome { requires_auth, .. } => assert!(requires_auth),
            _ => panic!("expected welcome"),
        }

        // 2. Hello, challenge, typed code -> AuthComplete with token
        let (_, ac) = pair_with(&mut client, &code).await;
        let _token = match ac {
            ServerMessage::AuthComplete { token } => token,
            _ => panic!("expected auth complete, got {ac:?}"),
        };

        // 3. Issue a command -> server receives it
        let cmd = ClientMessage::Command {
            id: "c1".into(),
            method: "task.run".into(),
            params: serde_json::json!({ "name": "cargo test" }),
        };
        client
            .send(Message::Text(serde_json::to_string(&cmd).unwrap()))
            .await
            .unwrap();
        let received = cmd_rx.recv().await.unwrap();
        assert_eq!(received.method, "task.run");
        assert_eq!(received.params["name"], "cargo test");

        // 4. Server broadcasts an event -> client receives it
        outbound
            .send(ServerMessage::Event {
                topic: "notifications".into(),
                payload: serde_json::json!({ "title": "Cargo test failed" }),
            })
            .unwrap();
        let evt = recv_server_msg(&mut client).await;
        match evt {
            ServerMessage::Event { topic, .. } => assert_eq!(topic, "notifications"),
            _ => panic!("expected event, got {evt:?}"),
        }

        // 5. Heartbeat round-trip
        client
            .send(Message::Text(
                serde_json::to_string(&ClientMessage::Ping).unwrap(),
            ))
            .await
            .unwrap();
        let pong = recv_server_msg(&mut client).await;
        assert!(matches!(pong, ServerMessage::Pong));

        client.close(None).await.unwrap();
    }

    #[tokio::test]
    async fn open_mode_skips_pairing_and_delivers_commands() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = WsServer::new(addr).with_require_auth(false);
        let mut cmd_rx = server.subscribe_commands();
        let outbound = server.broadcast_tx();
        tokio::spawn(async move {
            server.run_on(listener).await.unwrap();
        });
        tokio::time::sleep(Duration::from_millis(150)).await;

        let (mut client, _resp) = tokio_tungstenite::connect_async(format!("ws://{addr}"))
            .await
            .unwrap();

        // Welcome advertises open mode — no pairing step.
        let welcome = recv_server_msg(&mut client).await;
        match welcome {
            ServerMessage::Welcome { requires_auth, .. } => assert!(!requires_auth),
            _ => panic!("expected welcome"),
        }

        // A command is delivered WITHOUT any auth_response.
        let cmd = ClientMessage::Command {
            id: "c-open".into(),
            method: "task.run".into(),
            params: serde_json::json!({}),
        };
        client
            .send(Message::Text(serde_json::to_string(&cmd).unwrap()))
            .await
            .unwrap();
        let received = cmd_rx.recv().await.unwrap();
        assert_eq!(received.method, "task.run");

        // Broadcasts reach the open client.
        outbound
            .send(ServerMessage::Event {
                topic: "notifications".into(),
                payload: serde_json::json!({}),
            })
            .unwrap();
        let evt = recv_server_msg(&mut client).await;
        assert!(matches!(evt, ServerMessage::Event { .. }));

        client.close(None).await.unwrap();
    }

    #[tokio::test]
    async fn unauthenticated_command_is_ignored() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = WsServer::new(addr);
        let mut cmd_rx = server.subscribe_commands();
        tokio::spawn(async move {
            server.run_on(listener).await.unwrap();
        });
        tokio::time::sleep(Duration::from_millis(150)).await;

        let (mut client, _resp) = tokio_tungstenite::connect_async(format!("ws://{addr}"))
            .await
            .unwrap();
        let _welcome = recv_server_msg(&mut client).await;

        let cmd = ClientMessage::Command {
            id: "x".into(),
            method: "task.run".into(),
            params: serde_json::json!({}),
        };
        client
            .send(Message::Text(serde_json::to_string(&cmd).unwrap()))
            .await
            .unwrap();
        // No command should reach the app channel.
        let timed_out = tokio::time::timeout(Duration::from_millis(200), cmd_rx.recv())
            .await
            .is_err();
        assert!(timed_out, "unauthenticated command must not be delivered");
        client.close(None).await.unwrap();
    }

    /// A loopback server with a client token, and a broadcast handle.
    async fn token_server() -> (String, broadcast::Sender<ServerMessage>) {
        let server = WsServer::new("127.0.0.1:0".parse().unwrap())
            .with_require_auth(false)
            .with_client_token("tok-123");
        let outbound = server.broadcast_tx();
        let commands = server.subscribe_commands();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("ws://{}", listener.local_addr().unwrap());
        tokio::spawn(async move {
            let _keep_commands_open = commands;
            let _ = server.run_on(listener).await;
        });
        (url, outbound)
    }

    fn secret() -> ServerMessage {
        ServerMessage::Error {
            code: "answer".into(),
            message: "the secret answer".into(),
        }
    }

    /// The next text frame within `ms`, or None.
    async fn next_text(ws: &mut Client, ms: u64) -> Option<String> {
        let wait = tokio::time::timeout(Duration::from_millis(ms), async {
            loop {
                match ws.next().await {
                    Some(Ok(Message::Text(t))) => return Some(t),
                    Some(Ok(_)) => continue,
                    _ => return None,
                }
            }
        });
        wait.await.ok().flatten()
    }

    #[tokio::test]
    async fn without_the_token_a_local_client_hears_nothing() {
        let (url, outbound) = token_server().await;
        let (mut client, _) = tokio_tungstenite::connect_async(url.as_str())
            .await
            .unwrap();
        let _welcome = recv_server_msg(&mut client).await;
        tokio::time::sleep(Duration::from_millis(100)).await;
        let _ = outbound.send(secret());
        assert_eq!(
            next_text(&mut client, 400).await,
            None,
            "an untrusted client got an event"
        );

        // A command with the wrong token doesn't open it either.
        let wrong = ClientMessage::Command {
            id: "1".into(),
            method: "plan.goal".into(),
            params: serde_json::json!({ "token": "guess" }),
        };
        send_client(&mut client, &wrong).await;
        tokio::time::sleep(Duration::from_millis(100)).await;
        let _ = outbound.send(secret());
        assert_eq!(
            next_text(&mut client, 400).await,
            None,
            "a wrong token opened the stream"
        );
    }

    #[tokio::test]
    async fn with_the_token_friday_ctl_hears_its_answer() {
        let (url, outbound) = token_server().await;
        let (mut client, _) = tokio_tungstenite::connect_async(url.as_str())
            .await
            .unwrap();
        let _welcome = recv_server_msg(&mut client).await;
        let cmd = ClientMessage::Command {
            id: "1".into(),
            method: "plan.goal".into(),
            params: serde_json::json!({ "token": "tok-123", "goal": "hi" }),
        };
        send_client(&mut client, &cmd).await;
        tokio::time::sleep(Duration::from_millis(100)).await;
        let _ = outbound.send(secret());
        let got = next_text(&mut client, 2000)
            .await
            .expect("the trusted client got nothing");
        assert!(got.contains("the secret answer"));
    }

    #[tokio::test]
    async fn a_browser_cannot_connect() {
        use tokio_tungstenite::tungstenite::client::IntoClientRequest;
        let (url, _outbound) = token_server().await;
        let mut req = url.as_str().into_client_request().unwrap();
        req.headers_mut()
            .insert("Origin", "https://evil.example.com".parse().unwrap());
        assert!(
            tokio_tungstenite::connect_async(req).await.is_err(),
            "a browser page connected"
        );
        // FRIDAY's own client (no Origin) still can.
        assert!(tokio_tungstenite::connect_async(url.as_str()).await.is_ok());
    }

    #[tokio::test]
    async fn a_client_that_vanishes_without_a_close_frame_leaves_no_session() {
        let server = WsServer::new("127.0.0.1:0".parse().unwrap()).with_require_auth(false);
        let sessions = server.sessions();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("ws://{}", listener.local_addr().unwrap());
        tokio::spawn(async move {
            let _ = server.run_on(listener).await;
        });

        let (mut client, _resp) = tokio_tungstenite::connect_async(url.as_str())
            .await
            .unwrap();
        let _welcome = recv_server_msg(&mut client).await;
        assert_eq!(sessions.count(), 1);

        // No close handshake: the socket is just dropped.
        drop(client);
        for _ in 0..50 {
            if sessions.count() == 0 {
                return;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        panic!("the session of a dropped connection stayed registered");
    }

    #[tokio::test]
    async fn bus_bridge_survives_publish_with_zero_clients() {
        // Regression: the bus-bridge used to `break` (die) when its first
        // `outbound.send` errored — and tokio broadcast errors when ZERO
        // clients are connected, which the probe's 2s telemetry tick reliably
        // hit before anyone connected. The bridge must outlive that; a client
        // connecting later still receives the live stream.
        use crate::bridge::BusBridge;
        use friday_core::event::{Event, EventBus};

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let bus = EventBus::new();
        let server = WsServer::new(addr).with_require_auth(false);
        let outbound = server.broadcast_tx();
        // Bridge starts BEFORE any client exists.
        BusBridge::new(bus.clone(), outbound.clone()).start();

        tokio::spawn(async move {
            server.run_on(listener).await.unwrap();
        });
        tokio::time::sleep(Duration::from_millis(150)).await;

        // Publish with zero connected clients — send errors; bridge must live.
        bus.publish(Event::new(
            "telemetry.system",
            "system-probe",
            serde_json::json!({}),
        ))
        .await
        .unwrap();

        let (mut client, _resp) = tokio_tungstenite::connect_async(format!("ws://{addr}"))
            .await
            .unwrap();
        let _welcome = recv_server_msg(&mut client).await;

        // A subsequent publish reaches the client — proof the bridge survived.
        bus.publish(Event::new(
            "telemetry.system",
            "system-probe",
            serde_json::json!({ "cpu_percent": 7 }),
        ))
        .await
        .unwrap();
        let evt = recv_server_msg(&mut client).await;
        match evt {
            ServerMessage::Event { topic, payload } => {
                assert_eq!(topic, "telemetry.system");
                assert_eq!(payload["cpu_percent"], 7);
            }
            _ => panic!("expected forwarded event, got {evt:?}"),
        }
        client.close(None).await.unwrap();
    }

    // M6.2a: prove the live integration. A phone connects, pairs, then receives a
    // REAL host-telemetry event that flowed bus -> bridge -> websocket, and a
    // structured `planner.goal` command that round-trips bus -> bridge -> phone.
    #[tokio::test]
    async fn live_integration_real_telemetry_and_command() {
        use crate::bridge::BusBridge;
        use crate::commands::{method::PLANNER_GOAL, CommandGateway};
        use crate::probe::SystemProbe;
        use friday_core::event::EventBus;

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let bus = EventBus::new();
        let server = WsServer::new(addr);
        let outbound = server.broadcast_tx();
        let sessions = server.sessions();
        let mut ingress = server.subscribe_commands();
        let code = server.begin_pairing();

        // Bus bridge subscribes (never polls) and forwards to phones.
        BusBridge::new(bus.clone(), outbound.clone()).start();

        // Command gateway: phone commands -> real bus events.
        let gateway = CommandGateway::new(bus.clone(), sessions.clone());
        let result_out = outbound.clone();
        tokio::spawn(async move {
            while let Ok(cmd) = ingress.recv().await {
                let r = gateway.dispatch(&cmd).await;
                if result_out.send(r).is_err() {
                    break;
                }
            }
        });

        tokio::spawn(async move {
            server.run_on(listener).await.unwrap();
        });
        tokio::time::sleep(Duration::from_millis(150)).await;

        let url = format!("ws://{addr}");
        let (mut client, _resp) = tokio_tungstenite::connect_async(url).await.unwrap();
        let _welcome = recv_server_msg(&mut client).await;

        // Pair with the code shown on the PC -> authenticated.
        let (_, _auth_complete) = pair_with(&mut client, &code).await;

        // Publish a REAL host telemetry sample onto the bus (sysinfo).
        let sample = SystemProbe::sample();
        let payload = serde_json::to_value(&sample).unwrap();
        bus.publish(friday_core::event::Event::new(
            "telemetry.system",
            "system-probe",
            payload,
        ))
        .await
        .unwrap();

        // Issue a structured planner goal command.
        client
            .send(Message::Text(
                serde_json::to_string(&ClientMessage::Command {
                    id: "g1".into(),
                    method: PLANNER_GOAL.into(),
                    params: serde_json::json!({ "goal": "Run cargo test" }),
                })
                .unwrap(),
            ))
            .await
            .unwrap();

        // Collect messages until we've seen real telemetry + the planner goal echo.
        let mut saw_telemetry = false;
        let mut saw_goal = false;
        let mut saw_result = false;
        for _ in 0..10 {
            let msg = match tokio::time::timeout(
                Duration::from_millis(500),
                recv_server_msg(&mut client),
            )
            .await
            {
                Ok(m) => m,
                Err(_) => break,
            };
            match msg {
                ServerMessage::Event { topic, payload } => {
                    if topic == "telemetry.system" {
                        saw_telemetry = payload.get("memory_total_mb").is_some();
                    } else if topic == "tasks" {
                        // plan.goal is bridged under the "tasks" topic.
                        saw_goal =
                            payload.get("goal").and_then(|v| v.as_str()) == Some("Run cargo test");
                    }
                }
                ServerMessage::CommandResult { id, ok, .. } if id == "g1" => {
                    saw_result = ok;
                }
                _ => {}
            }
            if saw_telemetry && saw_goal && saw_result {
                break;
            }
        }

        assert!(
            saw_telemetry,
            "phone must receive REAL telemetry.system from the bus"
        );
        assert!(
            saw_goal,
            "phone must receive the plan.goal event echoed from the bus"
        );
        assert!(
            saw_result,
            "phone must receive a CommandResult for its goal"
        );
        client.close(None).await.unwrap();
    }
}
