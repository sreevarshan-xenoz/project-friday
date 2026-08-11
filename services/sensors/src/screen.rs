//! Real screen sensing — the first genuine perception organ (MasterPlan #13).
//!
//! The constitution's missing piece: *"Perception is synthetic RNG events, not
//! sensing … no continuous screen diff."* [`WindowsFocusSensor`](crate::windows_focus)
//! already tracks a bare title/pid focus switch; this sensor goes further and
//! captures the **real screen state**:
//!
//! ```text
//! Win32 foreground window
//!      ↓
//! ScreenObservation {
//!     title, pid, process,        ← identity
//!     bounds {x,y,w,h},           ← geometry (GetWindowRect)
//!     state (normal/minimized/…), ← IsIconic / IsZoomed
//!     content_hash,               ← client-area pixels (BitBlt + GetDIBits → FNV-1a)
//! }
//!      ↓  continuous diff: focus switch OR content hash change
//! Topic::Screen event → SensorLoop → GlobalWorkspace
//! ```
//!
//! The `content_hash` is the *continuous screen diff*: any visible change in
//! the foreground window's client area flips the hash, so FRIDAY perceives
//! content motion, not just focus switches — the input to observe → understand
//! → act → verify. Capture failure (e.g. a composited window that refuses
//! `BitBlt`) degrades to `content_hash: None` and identity/geometry changes
//! still fire events — the sensor is honest about what it could see.
//!
//! Only compiled on Windows (`cfg(target_os = "windows")`); the pure change-
//! detection and event-building logic is platform-neutral and tested everywhere.

use async_trait::async_trait;
use friday_cognitive::workspace::Topic;
use friday_cognitive::WorkspaceEvent;
use windows::Win32::Foundation::HWND;

use crate::Sensor;

/// A window's visible state — the durable screen truth the sensor diff's.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScreenObservation {
    pub title: String,
    pub pid: u32,
    /// Process image file name stem (e.g. `Code`, `explorer`) — the human
    /// answer to "what application is in front?".
    pub process: String,
    /// `(x, y, width, height)` of the window rectangle in screen coordinates.
    pub bounds: (i32, i32, i32, i32),
    /// `normal`, `minimized`, or `maximized`.
    pub state: String,
    /// FNV-1a fingerprint of the client-area pixels. `None` when the pixels
    /// could not be captured (composited/GPU windows) — identity + geometry
    /// changes still fire.
    pub content_hash: Option<u64>,
}

impl ScreenObservation {
    /// The observable signature for change detection: everything except the
    /// content hash (which is diff'd separately). Pure.
    pub fn signature(&self) -> (String, u32, String, (i32, i32, i32, i32), String) {
        (
            self.title.clone(),
            self.pid,
            self.process.clone(),
            self.bounds,
            self.state.clone(),
        )
    }
}

/// What changed since the last observation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScreenChange {
    /// A different window came to the foreground.
    Focus,
    /// The same window's visible content changed (continuous screen diff).
    Content,
}

impl ScreenChange {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Focus => "focus",
            Self::Content => "content",
        }
    }
}

/// The real screen sensor: polls the foreground window and emits a
/// [`Topic::Screen`] event only when something visibly changed — the first
/// poll always emits the initial observation (there is no prior state to
/// deduplicate against), subsequent polls are deduplicated against the last
/// observation.
pub struct WindowsScreenSensor {
    last: Option<ScreenObservation>,
}

impl WindowsScreenSensor {
    pub fn new() -> Self {
        Self { last: None }
    }
}

impl Default for WindowsScreenSensor {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Sensor for WindowsScreenSensor {
    fn name(&self) -> &str {
        "windows_screen"
    }

    fn topic_prefix(&self) -> &str {
        "sensor.screen"
    }

    async fn poll(&mut self) -> Vec<WorkspaceEvent> {
        let Some(observation) = capture_foreground_window() else {
            // No foreground window (e.g. the desktop/secure session): nothing
            // observable — do not invent one.
            return Vec::new();
        };

        // First observation: emit it as a Focus change — the initial screen
        // state is itself an event (observe → understand starts from here).
        // Later polls only emit when something visibly changed.
        let change = match self.last.as_ref() {
            None => Some(ScreenChange::Focus),
            Some(last) => changed_from(&observation, Some(last)),
        };
        self.last = Some(observation.clone());
        let Some(change) = change else {
            return Vec::new();
        };

        vec![build_screen_event(&observation, change)]
    }
}

// ── Pure helpers (platform-neutral, fully tested) ─────────────────

/// Whether `new` differs observably from `last`. `None` = no change.
pub fn changed_from(new: &ScreenObservation, last: Option<&ScreenObservation>) -> Option<ScreenChange> {
    let last = last?;
    if new.signature() != last.signature() {
        Some(ScreenChange::Focus)
    } else if new.content_hash.is_some() && new.content_hash != last.content_hash {
        Some(ScreenChange::Content)
    } else {
        None
    }
}

/// Build the `sensor.screen` workspace event. Pure — payload carries the full
/// observation plus the human-readable change reason.
pub fn build_screen_event(observation: &ScreenObservation, change: ScreenChange) -> WorkspaceEvent {
    let (x, y, w, h) = observation.bounds;
    WorkspaceEvent::new(
        Topic::Screen,
        serde_json::json!({
            "title": observation.title,
            "pid": observation.pid,
            "process": observation.process,
            "bounds": { "x": x, "y": y, "width": w, "height": h },
            "state": observation.state,
            "content_hash": observation.content_hash,
            "change": change.as_str(),
        }),
    )
}

/// FNV-1a 64-bit — the content fingerprint. Deterministic, no dependencies.
pub fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf29ce484222325;
    for b in bytes {
        hash ^= u64::from(*b);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

// ── Real capture (Windows only) ───────────────────────────────────

#[cfg(target_os = "windows")]
fn capture_foreground_window() -> Option<ScreenObservation> {
    use windows::Win32::UI::WindowsAndMessaging::{
        GetForegroundWindow, GetWindowRect, GetWindowTextLengthW, GetWindowTextW,
        GetWindowThreadProcessId, IsIconic, IsZoomed,
    };

    unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd.0.is_null() {
            return None;
        }

        // ── Identity ──
        let len = GetWindowTextLengthW(hwnd);
        let mut buf = vec![0u16; (len + 1) as usize];
        let _ = GetWindowTextW(hwnd, &mut buf);
        let title = String::from_utf16_lossy(&buf[..len as usize]);

        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        let process = process_name(pid).unwrap_or_default();

        // ── Geometry + state ──
        let mut rect = windows::Win32::Foundation::RECT::default();
        let _ = GetWindowRect(hwnd, &mut rect);
        let bounds = (
            rect.left,
            rect.top,
            rect.right - rect.left,
            rect.bottom - rect.top,
        );
        let state = if IsIconic(hwnd).as_bool() {
            "minimized"
        } else if IsZoomed(hwnd).as_bool() {
            "maximized"
        } else {
            "normal"
        };

        // ── Continuous screen diff: client-area pixel fingerprint ──
        // BitBlt the client area into a compatible bitmap, read the raw ARGB
        // pixels via GetDIBits, and FNV-1a them. Failure (composited/GPU
        // windows, zero-sized client) degrades to None — honest.
        let content_hash = capture_client_hash(hwnd).ok().flatten();

        Some(ScreenObservation {
            title,
            pid,
            process,
            bounds,
            state: state.to_string(),
            content_hash,
        })
    }
}

/// The client-area pixel fingerprint, or an error when capture failed.
/// Returns `Ok(None)` for a zero-sized client (nothing to hash).
#[cfg(target_os = "windows")]
unsafe fn capture_client_hash(hwnd: HWND) -> Result<Option<u64>, String> {
    use windows::Win32::Graphics::Gdi::{
        BitBlt, CreateCompatibleBitmap, CreateCompatibleDC, DeleteDC, DeleteObject, GetDC,
        GetDIBits, ReleaseDC, SelectObject, BITMAPINFO, BITMAPINFOHEADER, DIB_RGB_COLORS,
        HBITMAP, HDC, SRCCOPY, BI_RGB,
    };
    use windows::Win32::UI::WindowsAndMessaging::GetClientRect;

    let mut client = windows::Win32::Foundation::RECT::default();
    if GetClientRect(hwnd, &mut client).is_err() {
        return Err("GetClientRect failed".into());
    }
    let (w, h) = (client.right - client.left, client.bottom - client.top);
    if w <= 0 || h <= 0 {
        return Ok(None); // nothing visible to fingerprint
    }

    let hdc: HDC = GetDC(hwnd);
    if hdc.0.is_null() {
        return Err("GetDC failed".into());
    }
    let mem: HDC = CreateCompatibleDC(hdc);
    let bmp: HBITMAP = CreateCompatibleBitmap(hdc, w, h);
    if mem.0.is_null() || bmp.0.is_null() {
        let _ = ReleaseDC(hwnd, hdc);
        if !mem.0.is_null() {
            let _ = DeleteDC(mem);
        }
        if !bmp.0.is_null() {
            let _ = DeleteObject(bmp);
        }
        return Err("compatible DC/bitmap creation failed".into());
    }
    let _old = SelectObject(mem, bmp);

    let copied = BitBlt(mem, 0, 0, w, h, hdc, 0, 0, SRCCOPY);
    let _ = ReleaseDC(hwnd, hdc);
    if copied.is_err() {
        let _ = SelectObject(mem, HBITMAP::default());
        let _ = DeleteObject(bmp);
        let _ = DeleteDC(mem);
        return Err("BitBlt failed (composited window?)".into());
    }

    // Raw 32-bit BGRA scanlines, bottom-up.
    let mut buffer = vec![0u8; (w * h * 4) as usize];
    let mut bmi = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: w,
            biHeight: -h, // top-down
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        },
        ..Default::default()
    };
    let lines = GetDIBits(mem, bmp, 0, h as u32, Some(buffer.as_mut_ptr() as *mut _), &mut bmi, DIB_RGB_COLORS);
    let _ = SelectObject(mem, HBITMAP::default());
    let _ = DeleteObject(bmp);
    let _ = DeleteDC(mem);
    if lines == 0 {
        return Err("GetDIBits failed".into());
    }

    Ok(Some(fnv1a64(&buffer)))
}

/// The image file name stem of `pid`, or `None` (permission/vanished process).
#[cfg(target_os = "windows")]
fn process_name(pid: u32) -> Option<String> {
    use windows::Win32::System::Threading::{
        OpenProcess, PROCESS_NAME_FORMAT, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
    };
    use windows::core::PWSTR;

    if pid == 0 {
        return None;
    }
    unsafe {
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut buf = [0u16; 1024];
        let mut size = buf.len() as u32;
        if QueryFullProcessImageNameW(handle, PROCESS_NAME_FORMAT(0), PWSTR(buf.as_mut_ptr()), &mut size).is_err() {
            return None;
        }
        let path = String::from_utf16_lossy(&buf[..size as usize]);
        std::path::Path::new(&path)
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn observation(title: &str, hash: Option<u64>) -> ScreenObservation {
        ScreenObservation {
            title: title.to_string(),
            pid: 1234,
            process: "app".to_string(),
            bounds: (0, 0, 800, 600),
            state: "normal".to_string(),
            content_hash: hash,
        }
    }

    #[test]
    fn fnv1a_is_deterministic_and_sensitive() {
        let a = fnv1a64(b"hello screen");
        let b = fnv1a64(b"hello screen");
        let c = fnv1a64(b"hello scren!"); // one byte changed
        assert_eq!(a, b);
        assert_ne!(a, c);
        assert_ne!(fnv1a64(&[]), fnv1a64(&[0]));
    }

    #[test]
    fn first_poll_state_is_unknown_but_changed_from_none_is_none() {
        // changed_from(None) never reports a change — the first poll always
        // emits because poll() emits when last is None, handled above it.
        assert!(changed_from(&observation("a", Some(1)), None).is_none());
    }

    #[test]
    fn focus_switch_is_a_focus_change() {
        let new = observation("b", Some(1));
        let last = observation("a", Some(1));
        assert_eq!(changed_from(&new, Some(&last)), Some(ScreenChange::Focus));
    }

    #[test]
    fn content_hash_flip_is_a_content_change() {
        let new = observation("a", Some(2));
        let last = observation("a", Some(1));
        assert_eq!(changed_from(&new, Some(&last)), Some(ScreenChange::Content));
    }

    #[test]
    fn identical_observations_are_no_change() {
        let new = observation("a", Some(1));
        let last = observation("a", Some(1));
        assert_eq!(changed_from(&new, Some(&last)), None);
    }

    #[test]
    fn missing_hash_cannot_fire_content_change() {
        // Without a fingerprint the sensor cannot claim content motion —
        // only identity/geometry changes fire.
        let new = observation("a", None);
        let last = observation("a", None);
        assert_eq!(changed_from(&new, Some(&last)), None);
        let moved = ScreenObservation {
            bounds: (10, 10, 800, 600),
            ..observation("a", None)
        };
        assert_eq!(changed_from(&moved, Some(&last)), Some(ScreenChange::Focus));
    }

    #[test]
    fn screen_event_carries_the_full_observation() {
        let obs = observation("Terminal", Some(42));
        let event = build_screen_event(&obs, ScreenChange::Content);
        assert_eq!(event.topic, Topic::Screen);
        assert_eq!(event.payload["title"], "Terminal");
        assert_eq!(event.payload["process"], "app");
        assert_eq!(event.payload["state"], "normal");
        assert_eq!(event.payload["content_hash"], 42);
        assert_eq!(event.payload["change"], "content");
        assert_eq!(event.payload["bounds"]["width"], 800);
        assert_eq!(event.payload["pid"], 1234);
    }

    #[cfg(target_os = "windows")]
    #[tokio::test]
    async fn real_screen_poll_is_tolerant_and_well_formed() {
        // On a real desktop this returns exactly one event (first poll) or
        // none when there is no observable foreground window (headless/secured
        // session). Either way it must never panic and the shape must hold.
        let mut sensor = WindowsScreenSensor::new();
        let events = sensor.poll().await;
        assert!(events.len() <= 1, "first poll emits at most one event");
        if let Some(event) = events.first() {
            assert_eq!(event.topic, Topic::Screen);
            let p = &event.payload;
            assert!(p["title"].is_string());
            assert!(p["pid"].is_u64());
            assert!(p["process"].is_string());
            assert!(p["state"].is_string());
            assert!(p["change"].is_string());
            assert!(p["bounds"]["width"].is_i64());
        }
        // A second poll is a no-op unless something visibly changed.
        let events2 = sensor.poll().await;
        assert!(events2.is_empty() || events2.len() == 1);
    }
}
