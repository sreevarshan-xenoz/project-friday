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

pub use bridge::WindowsAdapter;
pub use manifest::CAPABILITY_MANIFEST;
