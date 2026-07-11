//! FRIDAY Windows Bridge (M4.5.2).
//!
//! A platform-agnostic [`ActionAdapter`] that translates `windows.*` intent
//! into IPC frames for a standalone C# Windows Host Service. The Rust side
//! never sees a Win32 handle, an `HWND`, or an `HRESULT`. See
//! `docs/WindowsBridge.md` for the locked contract.
//!
//! Layout:
//! - [`protocol`] — versioned `BridgeFrame` envelope (wraps core `ProtocolMessage`)
//! - [`error`]    — normalized [`BridgeError`] set (no Win32 codes cross back)
//! - [`model`]    — `Window` / `Bounds` / `WindowState` abstraction
//! - [`actions`]  — intent catalog the adapter advertises
//! - [`ipc`]      — transport abstraction (Named Pipes on Windows, mock for tests)
//! - [`manifest`] — capability manifest for runtime discovery (feeds M4.6)
//! - [`bridge`]   — the `WindowsAdapter` implementing `ActionAdapter`

pub mod actions;
pub mod bridge;
pub mod error;
pub mod ipc;
pub mod manifest;
pub mod model;
pub mod protocol;
pub mod sim;

pub use bridge::WindowsAdapter;
pub use manifest::CAPABILITY_MANIFEST;
pub use sim::sim_registry;

use friday_capabilities::model::{Capability, Platform};

/// Capabilities this bridge advertises to the Capability Intelligence registry.
pub fn capabilities() -> Vec<Capability> {
    CAPABILITY_MANIFEST
        .capabilities
        .iter()
        .map(|c| {
            let verb = c.strip_prefix("windows.").unwrap_or(c);
            Capability::new(*c, *c, "Windows")
                .with_actions(&[verb])
                .with_description(format!("Windows bridge intent {verb}"))
                .with_platform(Platform::Windows)
                .with_latency(30)
                .with_reliability(0.9)
                .with_confidence(0.9)
        })
        .collect()
}
