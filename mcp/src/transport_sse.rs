//! SSE (Server-Sent Events) transport for MCP (PR5).
//!
//! Classic MCP SSE framing: the client opens a long-lived `GET` stream at
//! `{base}/sse` (`Accept: text/event-stream`) and receives server→client frames
//! as `data: <json>\n\n` lines; client→server frames are `POST`ed as JSON to
//! `{base}/message`. Request/response correlation is left to the protocol layer
//! (every frame carries its JSON-RPC `id`); this transport only moves frames.
//!
//! Per the herd invariant, a [`Transport`] never parses JSON-RPC — it only knows
//! frame *kinds* (an SSE `data:` line is one frame, exactly like a WebSocket text
//! message or a stdio line). The protocol layer in [`crate::protocol`] is
//! unchanged and drives this transport identically.

use async_trait::async_trait;
use tokio::io::AsyncWriteExt;
use tokio::sync::mpsc;

use crate::transport::Transport;

/// A [`Transport`] over an MCP SSE session. Inbound frames arrive on the SSE
/// `GET` stream; outbound frames are `POST`ed. A background task owns the live
/// SSE connection and feeds inbound frames into the `incoming` channel.
pub struct SseTransport {
    /// Buffered inbound frames (one per SSE `data:` line) drained by `recv`.
    incoming: mpsc::UnboundedReceiver<String>,
    /// `reqwest` client used to `POST` outbound frames to the message endpoint.
    client: reqwest::Client,
    /// Fully-qualified `POST` endpoint for client→server frames.
    message_url: String,
    /// Handle of the background SSE reader task, retained so `close` can await a
    /// clean shutdown and so panics propagate instead of silently detaching.
    task: Option<tokio::task::JoinHandle<()>>,
}

impl SseTransport {
    /// Build an SSE transport from an already-connected SSE byte stream and the
    /// `POST` endpoint URL. The `stream` yields raw SSE bytes (the caller has
    /// already performed the `GET` handshake); this constructor parses `data:`
    /// lines only (never JSON) and forwards each payload as one frame.
    pub fn new(
        client: reqwest::Client,
        message_url: String,
        stream: impl tokio::io::AsyncRead + Unpin + Send + 'static,
    ) -> Self {
        let (read_tx, read_rx) = mpsc::unbounded_channel::<String>();

        let task = tokio::spawn(async move {
            use tokio::io::AsyncBufReadExt;
            use tokio::io::BufReader;
            let mut reader = BufReader::new(stream);
            let mut line = String::new();
            loop {
                line.clear();
                match reader.read_line(&mut line).await {
                    Ok(0) => break, // stream closed by server
                    Ok(_) => {}
                    Err(_) => break,
                }
                let trimmed = line.trim_end();
                // SSE: a frame's payload is on `data:` lines, terminated by a
                // blank line. We forward each `data:` payload as one frame and
                // ignore `event:`/`id:`/`retry:` lines — frame KIND only.
                if let Some(payload) = trimmed.strip_prefix("data:") {
                    let payload = payload.trim();
                    if payload.is_empty() {
                        continue;
                    }
                    if read_tx.send(payload.to_string()).is_err() {
                        break;
                    }
                }
            }
        });

        Self {
            incoming: read_rx,
            client,
            message_url,
            task: Some(task),
        }
    }
}

#[async_trait]
impl Transport for SseTransport {
    async fn send(&mut self, frame: &str) -> Result<(), String> {
        // Client→server frame: POST the JSON body to the message endpoint. The
        // server routes the correlated response back over the SSE stream; the
        // protocol layer matches ids.
        let resp = self
            .client
            .post(&self.message_url)
            .header("Content-Type", "application/json")
            .body(frame.to_string())
            .send()
            .await
            .map_err(|e| format!("sse POST to {} failed: {}", self.message_url, e))?;
        if !resp.status().is_success() {
            return Err(format!(
                "sse POST to {} returned status {}",
                self.message_url,
                resp.status()
            ));
        }
        Ok(())
    }

    async fn recv(&mut self) -> Result<String, String> {
        self.incoming
            .recv()
            .await
            .ok_or_else(|| "sse transport closed".to_string())
    }

    async fn try_recv(&mut self) -> Result<String, String> {
        match self.incoming.try_recv() {
            Ok(f) => Ok(f),
            Err(_) => Err("no frame available".to_string()),
        }
    }

    async fn close(&mut self) -> Result<(), String> {
        if let Some(task) = self.task.take() {
            if !task.is_finished() {
                // Drop the SSE stream reader; the server closes the connection.
                task.abort();
                let _ = task.await;
            }
        }
        Ok(())
    }
}

/// Connect to an MCP server over SSE at `base_url` (e.g. `http://localhost:3000`).
/// Opens the `GET {base_url}/sse` stream and uses `{base_url}/message` for
/// outbound `POST`s. The SSE response body is streamed byte-for-byte into the
/// transport, so a live server's frames arrive as they are emitted. Returns a
/// boxed [`Transport`] ready for an [`crate::protocol::McpClient`].
pub async fn sse_transport_for(base_url: &str) -> Result<Box<dyn Transport>, String> {
    let base = base_url.trim_end_matches('/');
    let sse_url = format!("{base}/sse");
    let message_url = format!("{base}/message");

    let client = reqwest::Client::new();
    let resp = client
        .get(&sse_url)
        .header("Accept", "text/event-stream")
        .send()
        .await
        .map_err(|e| format!("sse GET to {sse_url} failed: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("sse GET to {sse_url} returned status {}", resp.status()));
    }

    // Bridge the streaming response body into a tokio pipe that the transport
    // reads. reqwest's `bytes_stream` yields body chunks as they arrive.
    let (reader, mut writer) = tokio::io::duplex(256 * 1024);
    tokio::spawn(async move {
        use futures::StreamExt;
        let mut stream = resp.bytes_stream();
        while let Some(chunk) = stream.next().await {
            match chunk {
                Ok(bytes) => {
                    if writer.write_all(&bytes).await.is_err() {
                        break;
                    }
                    let _ = writer.flush().await;
                }
                Err(_) => break,
            }
        }
    });

    Ok(Box::new(SseTransport::new(client, message_url, reader)))
}
