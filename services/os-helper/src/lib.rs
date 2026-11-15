//! FRIDAY OS root helper service (`friday-os-helper`).
//!
//! Provides a minimal, auditable D-Bus interface (`org.friday.Helper1`)
//! for FRIDAY OS to perform system updates and package management.
//! Every privileged method is gated by polkit; FRIDAY itself never runs as root
//! and never gets a generic way to run arbitrary commands.

#![forbid(unsafe_code)]

#[cfg(target_os = "linux")]
pub mod auth;
#[cfg(target_os = "linux")]
pub mod caller;
#[cfg(target_os = "linux")]
pub mod error;
#[cfg(target_os = "linux")]
pub mod jobs;
#[cfg(target_os = "linux")]
pub mod runner;
#[cfg(target_os = "linux")]
pub mod service;
#[cfg(target_os = "linux")]
pub mod validator;

#[cfg(target_os = "linux")]
pub use service::{run_daemon, Helper, HelperIface};
