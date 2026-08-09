use async_trait::async_trait;
use friday_cognitive::workspace::Topic;
use friday_cognitive::WorkspaceEvent;

use crate::Sensor;

/// Tracks the active foreground window on Windows.
///
/// Uses Win32 `GetForegroundWindow` + `GetWindowTextW` to detect
/// when the user switches applications.
pub struct WindowsFocusSensor {
    last_title: String,
    last_pid: u32,
}

impl WindowsFocusSensor {
    pub fn new() -> Self {
        Self {
            last_title: String::new(),
            last_pid: 0,
        }
    }
}

impl Default for WindowsFocusSensor {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Sensor for WindowsFocusSensor {
    fn name(&self) -> &str {
        "windows_focus"
    }

    fn topic_prefix(&self) -> &str {
        "sensor.window"
    }

    async fn poll(&mut self) -> Vec<WorkspaceEvent> {
        let (title, pid) = get_foreground_window();

        if title == self.last_title && pid == self.last_pid {
            return Vec::new();
        }

        self.last_title = title.clone();
        self.last_pid = pid;

        vec![WorkspaceEvent::new(
            Topic::WindowFocusChanged,
            serde_json::json!({
                "title": title,
                "pid": pid,
            }),
        )]
    }
}

fn get_foreground_window() -> (String, u32) {
    use windows::Win32::UI::WindowsAndMessaging::{
        GetForegroundWindow, GetWindowTextLengthW, GetWindowTextW, GetWindowThreadProcessId,
    };

    unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd.0.is_null() {
            return (String::new(), 0);
        }

        let len = GetWindowTextLengthW(hwnd);
        let mut buf = vec![0u16; (len + 1) as usize];
        GetWindowTextW(hwnd, &mut buf);
        let title = String::from_utf16_lossy(&buf[..len as usize]);

        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));

        (title, pid)
    }
}
