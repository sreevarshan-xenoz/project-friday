//! Environment capture for Evidence Packs.
//!
//! `environment.json` records the machine/host context a qualification run
//! happened in, so a failure years later is debuggable without reproducing the
//! environment. Gathered via PowerShell on Windows; mock mode records the same
//! shape with `mode = "mock"`.

use serde::Serialize;
use std::process::Command;

use crate::harness::Mode;

#[derive(Debug, Clone, Serialize)]
pub struct Environment {
    pub mode: String,
    pub timestamp: String,
    pub friday_version: String,
    pub windows_host_version: String,
    pub windows_version: String,
    pub desktop_session: String,
    pub dpi: String,
    pub monitor_count: String,
    pub elevation: String,
    pub foreground_process: String,
    pub os: String,
}

/// Run a PowerShell query, returning "unknown" if it fails or is empty.
fn ps(query: &str) -> String {
    let out = Command::new("powershell")
        .args([
            "-NoProfile",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            query,
        ])
        .output();
    match out {
        Ok(o) if o.status.success() => {
            let s = String::from_utf8_lossy(&o.stdout).trim().to_string();
            if s.is_empty() {
                "unknown".into()
            } else {
                s
            }
        }
        _ => "unknown".into(),
    }
}

pub fn capture(mode: Mode) -> Environment {
    let mode_str = match mode {
        Mode::Mock => "mock",
        Mode::Real => "real",
    };
    let windows_version = ps("(Get-CimInstance Win32_OperatingSystem).Version");
    let elevation = ps("([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)");
    let monitor_count =
        ps("Add-Type -AssemblyName System.Windows.Forms; [System.Windows.Forms.Screen]::AllScreens.Count");
    let dpi = ps("(Get-ItemProperty 'HKCU:\\Control Panel\\Desktop\\WindowMetrics' -Name AppliedDPI -ErrorAction SilentlyContinue).AppliedDPI");
    let desktop_session = ps("([System.Diagnostics.Process]::GetCurrentProcess()).SessionId");
    let foreground_process = ps(
        "Add-Type -MemberDefinition '[DllImport(\"user32.dll\")] public static extern IntPtr GetForegroundWindow(); [DllImport(\"user32.dll\")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint p);' -Name W -Namespace Win32; \
         $h=[Win32.W]::GetForegroundWindow(); $p=0; [Win32.W]::GetWindowThreadProcessId($h,[ref]$p); \
         try { (Get-Process -Id $p -ErrorAction Stop).ProcessName } catch { 'unknown' }",
    );
    Environment {
        mode: mode_str.into(),
        timestamp: chrono::Utc::now().to_rfc3339(),
        friday_version: env!("CARGO_PKG_VERSION").into(),
        windows_host_version: if mode == Mode::Real {
            "real (C# host)".into()
        } else {
            "mock".into()
        },
        windows_version,
        desktop_session,
        dpi,
        monitor_count,
        elevation,
        foreground_process,
        os: std::env::consts::OS.into(),
    }
}
