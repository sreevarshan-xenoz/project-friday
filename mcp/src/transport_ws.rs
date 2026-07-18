//! WebSocket transport for MCP (PR4).
//!
//! Frames are JSON-RPC text messages over a WebSocket connection, per the MCP
//! Streamable HTTP / WebSocket framing (one JSON object per text message). This
//! is a thin [`Transport`] impl: no JSON-RPC logic, just `send`/`recv` of
//! framed text. The protocol layer in [`crate::protocol`] is unchanged.

use async_trait::async_trait;
use futures::{SinkExt, StreamExt};
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::WebSocketStream;

use crate::transport::Transport;

/// A [`Transport`] over a connected WebSocket stream. One JSON-RPC message == one
/// text `Message`. The background task owns the live socket and answers server
/// pings, so connections stay alive; only text frames carry protocol payloads.
pub struct WebSocketTransport {
    /// Buffered inbound text frames drained by [`Transport::recv`]. A background
    /// reader task owns the live socket and feeds this, so the protocol layer
    /// can block on a single `recv`.
    incoming: mpsc::UnboundedReceiver<String>,
    /// Channel to the background writer task; kept alive so the task runs until
    /// this handle is dropped, and used to signal a close.
    writer: mpsc::UnboundedSender<Message>,
    /// Handle of the background socket task. Retained so [`WebSocketTransport`]
    /// can await graceful shutdown in [`Transport::close`]; dropping it detached
    /// would forfeit cleanup confirmation and panic propagation.
    task: Option<tokio::task::JoinHandle<()>>,
}

impl WebSocketTransport {
    /// Wrap an already-connected WebSocket stream. The socket is moved into a
    /// background reader/writer task (retained via [`Self::task`]); this handle
    /// keeps the channels alive and can drive a graceful shutdown.
    pub fn new<S>(ws: WebSocketStream<S>) -> Self
    where
        S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
    {
        let (write_tx, mut write_rx) = mpsc::unbounded_channel::<Message>();
        let (read_tx, read_rx) = mpsc::unbounded_channel::<String>();

        // Reader/writer task owns the live socket. It answers server pings with
        // pongs, performs the close handshake on `Message::Close`, and exits when
        // either the socket or the writer channel closes.
        let task = tokio::spawn(async move {
            let mut ws = ws;
            loop {
                tokio::select! {
                    maybe_msg = ws.next() => {
                        let msg = match maybe_msg {
                            Some(Ok(m)) => m,
                            Some(Err(_)) | None => break,
                        };
                        match msg {
                            Message::Text(t) => {
                                if read_tx.send(t.to_string()).is_err() {
                                    break;
                                }
                            }
                            // Transport responsibility: answer pings so the server
                            // does not drop an idle connection.
                            Message::Ping(payload) => {
                                if ws.send(Message::Pong(payload)).await.is_err() {
                                    break;
                                }
                            }
                            Message::Pong(_) => { /* peer answered our ping: ignore */ }
                            Message::Close(_) => {
                                // RFC 6455: echo the close, flush, then exit.
                                let _ = ws.send(Message::Close(None)).await;
                                let _ = ws.flush().await;
                                break;
                            }
                            _ => { /* binary / other: not used by MCP JSON-RPC */ }
                        }
                    }
                    maybe_out = write_rx.recv() => {
                        match maybe_out {
                            Some(m @ Message::Close(_)) => {
                                // Our own close request: send, then exit.
                                let _ = ws.send(m).await;
                                let _ = ws.flush().await;
                                break;
                            }
                            Some(m) => {
                                if ws.send(m).await.is_err() {
                                    break;
                                }
                            }
                            None => break,
                        }
                    }
                }
            }
        });

        Self {
            incoming: read_rx,
            writer: write_tx,
            task: Some(task),
        }
    }
}

#[async_trait]
impl Transport for WebSocketTransport {
    async fn send(&mut self, frame: &str) -> Result<(), String> {
        self.writer
            .send(Message::Text(frame.to_string()))
            .map_err(|e| e.to_string())
    }

    async fn recv(&mut self) -> Result<String, String> {
        self.incoming
            .recv()
            .await
            .ok_or_else(|| "websocket transport closed".to_string())
    }

    async fn try_recv(&mut self) -> Result<String, String> {
        match self.incoming.try_recv() {
            Ok(f) => Ok(f),
            Err(_) => Err("no frame available".to_string()),
        }
    }

    async fn close(&mut self) -> Result<(), String> {
        // Signal the background task to send the close handshake and exit, then
        // await it so the caller gets confirmation the socket is torn down
        // (graceful shutdown rather than a detached task).
        let _ = self.writer.send(Message::Close(None));
        if let Some(task) = self.task.take() {
            if !task.is_finished() {
                let _ = task.await;
            }
        }
        Ok(())
    }
}

/// Connect to an MCP server over WebSocket at `url` (e.g.
/// `ws://localhost:3000/mcp`). Returns a boxed [`Transport`] ready for an
/// [`crate::protocol::McpClient`].
pub async fn websocket_transport_for(
    url: &str,
) -> Result<Box<dyn Transport>, String> {
    use tokio_tungstenite::connect_async;
    let (ws, _resp) = connect_async(url)
        .await
        .map_err(|e| format!("websocket connect to {url} failed: {e}"))?;
    Ok(Box::new(WebSocketTransport::new(ws)))
}
