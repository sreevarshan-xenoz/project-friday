//! Error type for the mobile service.

use thiserror::Error;

#[derive(Debug, Error)]
pub enum MobileError {
    #[error("websocket error: {0}")]
    WebSocket(String),

    #[error("auth error: {0}")]
    Auth(String),

    #[error("transport error: {0}")]
    Transport(String),

    #[error("serialization error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("pairing expired or unknown")]
    PairingUnknown,

    #[error("invalid signature")]
    InvalidSignature,

    #[error("connection closed")]
    Closed,
}

impl From<tokio_tungstenite::tungstenite::Error> for MobileError {
    fn from(e: tokio_tungstenite::tungstenite::Error) -> Self {
        MobileError::WebSocket(e.to_string())
    }
}
