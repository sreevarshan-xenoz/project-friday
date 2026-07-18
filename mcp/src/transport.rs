//! MCP transports.
//!
//! A [`Transport`] only moves *framed* messages (one JSON object per frame, the
//! protocol layer handles JSON-RPC correlation). This keeps each concrete
//! transport tiny and the protocol logic in exactly one place.
//!
//! PR1 ships the [`StdioTransport`] (newline-delimited JSON over a child
//! process's stdin/stdout). WebSocket/SSE arrive later as more `impl Transport`
//! — no protocol changes. Process lifecycle lives in [`ProcessLauncher`], fully
//! separate from the transport so the same transport can back local subprocesses,
//! remote servers, or future SSH bridges.

use async_trait::async_trait;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};

/// A framed message channel. Implementors move opaque frames; framing is
/// transport-defined (newline for stdio, a WebSocket text message, an SSE
/// `data:` line).
///
/// **Invariant — transports never interpret JSON.** A transport knows only
/// frame *kinds*: text, binary, ping, pong, close. It must never parse or
/// reason about JSON-RPC contents such as `{"jsonrpc":"2.0"}`; request/response
/// correlation, method routing, and notification handling belong exclusively to
/// [`crate::protocol::McpClient`]. Keeping this boundary prevents protocol
/// semantics from leaking into transports (and vice-versa) as new transports
/// are added.
#[async_trait]
pub trait Transport: Send + Sync {
    /// Send one framed message.
    async fn send(&mut self, frame: &str) -> Result<(), String>;
    /// Receive the next framed message (blocks until one arrives).
    async fn recv(&mut self) -> Result<String, String>;
    /// Non-blocking receive; returns `Err` if no frame is immediately available.
    async fn try_recv(&mut self) -> Result<String, String> {
        // Default: not supported; callers fall back to `recv`.
        Err("try_recv not supported".to_string())
    }
    /// Tear down the underlying channel (e.g. kill a child process).
    async fn close(&mut self) -> Result<(), String> {
        Ok(())
    }
}

/// stdio transport: newline-delimited JSON over a child process's stdin/stdout.
/// Frames are individual lines; blank lines are ignored.
pub struct StdioTransport {
    stdin: Option<ChildStdin>,
    reader: BufReader<ChildStdout>,
}

impl StdioTransport {
    pub fn new(stdin: ChildStdin, stdout: ChildStdout) -> Self {
        Self {
            stdin: Some(stdin),
            reader: BufReader::new(stdout),
        }
    }
}

#[async_trait]
impl Transport for StdioTransport {
    async fn send(&mut self, frame: &str) -> Result<(), String> {
        let stdin = self
            .stdin
            .as_mut()
            .ok_or_else(|| "stdio transport closed".to_string())?;
        // Newline-delimited framing.
        stdin
            .write_all(frame.as_bytes())
            .await
            .map_err(|e| e.to_string())?;
        stdin.write_all(b"\n").await.map_err(|e| e.to_string())?;
        stdin.flush().await.map_err(|e| e.to_string())?;
        Ok(())
    }

    async fn recv(&mut self) -> Result<String, String> {
        loop {
            let mut line = String::new();
            let n = self
                .reader
                .read_line(&mut line)
                .await
                .map_err(|e| e.to_string())?;
            if n == 0 {
                return Err("stdio transport closed (EOF)".to_string());
            }
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            return Ok(trimmed.to_string());
        }
    }

    async fn try_recv(&mut self) -> Result<String, String> {
        // BufReader has no non-blocking read; emulate by peeking via a short
        // poll is non-trivial, so we simply report unavailable. The protocol
        // layer only uses try_recv for best-effort notification draining after
        // initialize, where falling back to blocking recv is acceptable.
        Err("stdio transport has no non-blocking recv".to_string())
    }

    async fn close(&mut self) -> Result<(), String> {
        self.stdin = None;
        Ok(())
    }
}

/// Spawns MCP server processes. Deliberately separate from [`Transport`]: a
/// launcher produces the child's stdio handles, and a transport consumes them.
/// This lets the same [`StdioTransport`] back a local `npx` server, a remote
/// stdio bridge, or a future SSH-forwarded stream without transport changes.
pub struct ProcessLauncher;

impl ProcessLauncher {
    /// Launch `command` (e.g. `npx @modelcontextprotocol/server-filesystem`)
    /// with the given args, returning the child plus its stdio handles wired for
    /// JSON-RPC. stdout/stderr are inherited by default except stdout which is
    /// captured for the transport; stdin is piped.
    pub fn launch(
        command: &str,
        args: &[String],
    ) -> std::io::Result<(Child, ChildStdin, ChildStdout)> {
        let mut child = Command::new(command)
            .args(args)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::inherit())
            .kill_on_drop(true)
            .spawn()?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| std::io::Error::other("no stdin"))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| std::io::Error::other("no stdout"))?;
        Ok((child, stdin, stdout))
    }
}

/// Build a stdio [`Transport`] for a server process: launch it, wrap its stdio.
pub fn stdio_transport_for(
    command: &str,
    args: &[String],
) -> std::io::Result<(Box<dyn Transport>, Child)> {
    let (child, stdin, stdout) = ProcessLauncher::launch(command, args)?;
    let transport: Box<dyn Transport> = Box::new(StdioTransport::new(stdin, stdout));
    Ok((transport, child))
}

/// `Box<dyn Transport>` is itself a [`Transport`], so boxed transports can be
/// passed wherever a concrete `T: Transport` is expected (e.g. the protocol
/// client). This is what lets `stdio_transport_for` return a trait object.
#[async_trait]
impl Transport for Box<dyn Transport> {
    async fn send(&mut self, frame: &str) -> Result<(), String> {
        (**self).send(frame).await
    }
    async fn recv(&mut self) -> Result<String, String> {
        (**self).recv().await
    }
    async fn try_recv(&mut self) -> Result<String, String> {
        (**self).try_recv().await
    }
    async fn close(&mut self) -> Result<(), String> {
        (**self).close().await
    }
}
