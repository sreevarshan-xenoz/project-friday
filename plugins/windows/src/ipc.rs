//! Transport abstraction. The bridge talks to the host through [`IpcTransport`],
//! never through Win32 directly.
//!
//! - On Windows, [`NamedPipeTransport`] connects to the host's named pipe
//!   (`\\.\pipe\friday-windows-<session>`) — native, no ports, no firewall.
//! - [`MockTransport`] is a process-local stand-in used by tests (and can power
//!   a dry-run mode later) so the adapter is exercised without the C# service.

use async_trait::async_trait;

use crate::protocol::{BridgeFrame, BridgeResponse};

/// A bidirectional request/response IPC channel to the Windows host.
#[async_trait]
pub trait IpcTransport: Send + Sync {
    /// Send a request frame and await the correlated response frame.
    async fn call(&self, frame: BridgeFrame) -> Result<BridgeFrame, String>;
}

/// Process-local transport backed by a handler closure. Used in tests and for
/// dry-run; the handler stands in for the C# host.
pub struct MockTransport {
    handler: std::sync::Arc<dyn Fn(BridgeFrame) -> BridgeResponse + Send + Sync>,
}

impl MockTransport {
    pub fn new(
        handler: std::sync::Arc<dyn Fn(BridgeFrame) -> BridgeResponse + Send + Sync>,
    ) -> Self {
        Self { handler }
    }
}

#[async_trait]
impl IpcTransport for MockTransport {
    async fn call(&self, frame: BridgeFrame) -> Result<BridgeFrame, String> {
        let response = (self.handler)(frame.clone());
        Ok(frame.response_to(response))
    }
}

#[cfg(windows)]
mod pipe {
    use super::IpcTransport;
    use crate::protocol::BridgeFrame;
    use async_trait::async_trait;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::windows::named_pipe::ClientOptions;

    /// Real transport used by the runtime on Windows: a named-pipe client to the
    /// C# host. Framing is a 4-byte little-endian length prefix + UTF-8 JSON.
    pub struct NamedPipeTransport {
        pub path: String,
    }

    impl NamedPipeTransport {
        /// Connect to the host's pipe for a given session.
        pub fn session(session: &str) -> Self {
            Self {
                path: format!(r"\\.\pipe\friday-windows-{session}"),
            }
        }
    }

    #[async_trait]
    impl IpcTransport for NamedPipeTransport {
        async fn call(&self, frame: BridgeFrame) -> Result<BridgeFrame, String> {
            let mut client = ClientOptions::new()
                .open(&self.path)
                .map_err(|e| format!("host_unavailable: {e}"))?;
            let bytes = serde_json::to_vec(&frame).map_err(|e| e.to_string())?;
            let len = (bytes.len() as u32).to_le_bytes();
            client.write_all(&len).await.map_err(|e| e.to_string())?;
            client.write_all(&bytes).await.map_err(|e| e.to_string())?;
            client.flush().await.map_err(|e| e.to_string())?;

            let mut len_buf = [0u8; 4];
            client
                .read_exact(&mut len_buf)
                .await
                .map_err(|e| e.to_string())?;
            let resp_len = u32::from_le_bytes(len_buf) as usize;
            let mut buf = vec![0u8; resp_len];
            client
                .read_exact(&mut buf)
                .await
                .map_err(|e| e.to_string())?;
            serde_json::from_slice(&buf).map_err(|e| format!("protocol_error: {e}"))
        }
    }
}

#[cfg(windows)]
pub use pipe::NamedPipeTransport;
