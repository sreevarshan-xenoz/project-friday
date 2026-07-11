use thiserror::Error;

/// Unified error type for the FRIDAY core. Plugins and services return
/// `Result<T>` which aliases to this.
#[derive(Debug, Error)]
pub enum FridayError {
    #[error("event bus error: {0}")]
    Bus(String),

    #[error("plugin '{0}' error: {1}")]
    Plugin(String, String),

    #[error("plugin '{0}' not found")]
    PluginNotFound(String),

    #[error("memory layer '{0}' error: {1}")]
    Memory(String, String),

    #[error("serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    #[error("transport error: {0}")]
    Transport(String),

    #[error("configuration error: {0}")]
    Config(String),

    #[error("internal error: {0}")]
    Internal(String),
}

pub type Result<T> = std::result::Result<T, FridayError>;
