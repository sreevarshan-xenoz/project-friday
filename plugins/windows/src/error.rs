//! Normalized error set. The only error vocabulary that crosses the IPC
//! boundary back into Rust. The C# host maps every `HRESULT` / `GetLastError()`
//! / COM / .NET exception into one of these; Rust never sees a raw Win32 code.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum BridgeError {
    /// Target window id is unknown / already gone.
    WindowNotFound,
    /// Operation denied by the OS (ACL, file-in-use, UIPI, etc.).
    AccessDenied,
    /// Referenced process is not running.
    ProcessNotRunning,
    /// Operation exceeded its deadline / host did not respond in time.
    Timeout,
    /// A UI Automation element could not be located.
    UIElementMissing,
    /// A window/object handle was invalid (e.g. ERROR_INVALID_WINDOW_HANDLE).
    InvalidHandle,
    /// The host service is not reachable (pipe down / no handshake).
    HostUnavailable,
    /// Protocol version or frame shape mismatch.
    ProtocolError,
    /// Catch-all with a host-provided message.
    Unknown(String),
}

impl BridgeError {
    /// Stable code string carried in `BridgeResponse.error`.
    pub fn code(&self) -> String {
        match self {
            BridgeError::WindowNotFound => "WindowNotFound".into(),
            BridgeError::AccessDenied => "AccessDenied".into(),
            BridgeError::ProcessNotRunning => "ProcessNotRunning".into(),
            BridgeError::Timeout => "Timeout".into(),
            BridgeError::UIElementMissing => "UIElementMissing".into(),
            BridgeError::InvalidHandle => "InvalidHandle".into(),
            BridgeError::HostUnavailable => "HostUnavailable".into(),
            BridgeError::ProtocolError => "ProtocolError".into(),
            BridgeError::Unknown(s) => s.clone(),
        }
    }

    /// Parse a code string back into a `BridgeError`.
    pub fn from_code(code: &str) -> Self {
        match code {
            "WindowNotFound" => BridgeError::WindowNotFound,
            "AccessDenied" => BridgeError::AccessDenied,
            "ProcessNotRunning" => BridgeError::ProcessNotRunning,
            "Timeout" => BridgeError::Timeout,
            "UIElementMissing" => BridgeError::UIElementMissing,
            "InvalidHandle" => BridgeError::InvalidHandle,
            "HostUnavailable" => BridgeError::HostUnavailable,
            "ProtocolError" => BridgeError::ProtocolError,
            other => BridgeError::Unknown(other.to_string()),
        }
    }
}
