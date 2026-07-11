//! Window abstraction. No raw handle (`HWND`) is ever exposed to Rust; the C#
//! host maps its internal handle to a stable `id` and only returns this model.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Bounds {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum WindowState {
    Normal,
    Minimized,
    Maximized,
    Hidden,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Window {
    /// Host-generated stable id (NOT an `HWND`).
    pub id: String,
    pub title: String,
    /// Process image name (e.g. `notepad.exe`).
    pub process: String,
    pub process_id: u32,
    pub bounds: Bounds,
    pub state: WindowState,
    /// Monitor index the window is on.
    pub monitor: u32,
}
