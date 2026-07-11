//! Intent catalog. The adapter advertises these verbs; the runtime composes
//! them as `windows.<verb>` capabilities. The host maps each verb to Win32 /
//! UI Automation / COM / PowerShell internally — Rust never learns which.

/// Verbs understood by the Windows adapter. Each becomes capability
/// `windows.<verb>` (e.g. `windows.open`).
pub const ACTIONS: &[&str] = &[
    "open",
    "close",
    "focus",
    "move",
    "resize",
    "capture",
    "list",
    "foreground",
    "click",
    "type",
    "scroll",
    "clipboard.read",
    "clipboard.write",
];

/// Build the capability intent string for an adapter verb.
pub fn intent_for(verb: &str) -> String {
    format!("windows.{verb}")
}
