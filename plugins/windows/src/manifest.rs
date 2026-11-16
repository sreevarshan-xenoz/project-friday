//! Capability manifest. The host serves this at startup; the bridge ships a
//! static copy and may refresh it over IPC. The runtime uses it to discover
//! `windows.*` capabilities dynamically (feeds M4.6 Capability Registry).

use serde::Serialize;

/// Static capability manifest for the Windows bridge.
#[derive(Debug, Clone, Serialize)]
pub struct CapabilityManifest {
    pub bridge: &'static str,
    pub version: &'static str,
    pub protocol_version: u8,
    pub capabilities: &'static [&'static str],
}

/// The manifest the adapter advertises. Mirrors `windows-host`'s served manifest.
pub const CAPABILITY_MANIFEST: CapabilityManifest = CapabilityManifest {
    bridge: "windows",
    version: "1.0",
    protocol_version: 1,
    capabilities: &[
        "windows.open",
        "windows.close",
        "windows.focus",
        "windows.move",
        "windows.resize",
        "windows.capture",
        "windows.list",
        "windows.foreground",
        "windows.click",
        "windows.type",
        "windows.scroll",
        "windows.clipboard.read",
        "windows.clipboard.write",
        "windows.media",
        "windows.nowplaying",
        "windows.volume",
        "windows.brightness",
        "windows.desktops",
    ],
};
