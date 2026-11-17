//! Integration tests for the Windows bridge using an in-process mock host.
//! The mock stands in for the C# `windows-host` service, so the adapter's
//! full request/response path, lifecycle, and error normalization are exercised
//! on every platform without Windows or .NET.

use std::sync::{Arc, Mutex};

use friday_actions::adapters::ActionAdapter;
use friday_actions::request::ActionRequest;
use serde_json::json;

use friday_windows::error::BridgeError;
use friday_windows::ipc::MockTransport;
use friday_windows::manifest::CAPABILITY_MANIFEST;
use friday_windows::model::{Bounds, Window, WindowState};
use friday_windows::protocol::{BridgeFrame, BridgeResponse};
use friday_windows::WindowsAdapter;

#[derive(Default)]
struct State {
    windows: std::collections::HashMap<String, Window>,
    counter: u32,
    foreground: Option<String>,
    clipboard: String,
}

#[derive(Default)]
struct MockWindowsHost {
    state: Mutex<State>,
}

impl MockWindowsHost {
    fn new() -> Self {
        Self::default()
    }

    fn handle(&self, frame: BridgeFrame) -> BridgeResponse {
        let intent = frame.message.action.as_str();
        let p = &frame.message.payload;
        let mut st = self.state.lock().unwrap();
        match intent {
            "windows.open" => {
                st.counter += 1;
                let id = format!("win-{}", st.counter);
                let path = p.get("path").and_then(|v| v.as_str()).unwrap_or("app.exe");
                let process = path.rsplit(['\\', '/']).next().unwrap_or(path).to_string();
                let title = p
                    .get("title")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| process.clone());
                let win = Window {
                    id: id.clone(),
                    title,
                    process: process.clone(),
                    process_id: 1000 + st.counter,
                    bounds: Bounds {
                        x: 0,
                        y: 0,
                        width: 800,
                        height: 600,
                    },
                    state: WindowState::Normal,
                    monitor: 0,
                };
                st.windows.insert(id.clone(), win.clone());
                st.foreground = Some(id);
                BridgeResponse::ok(serde_json::to_value(&win).unwrap())
            }
            "windows.list" => {
                let list: Vec<Window> = st.windows.values().cloned().collect();
                BridgeResponse::ok(json!(list))
            }
            "windows.foreground" => match &st.foreground {
                Some(id) => match st.windows.get(id) {
                    Some(w) => BridgeResponse::ok(serde_json::to_value(w).unwrap()),
                    None => BridgeResponse::err(&BridgeError::WindowNotFound.code()),
                },
                None => BridgeResponse::err(&BridgeError::WindowNotFound.code()),
            },
            "windows.close" => {
                let id = p.get("window_id").and_then(|v| v.as_str()).unwrap_or("");
                if st.windows.remove(id).is_some() {
                    if st.foreground.as_deref() == Some(id) {
                        st.foreground = None;
                    }
                    BridgeResponse::ok(json!({ "closed": id }))
                } else {
                    BridgeResponse::err(&BridgeError::WindowNotFound.code())
                }
            }
            "windows.focus" => {
                let id = p.get("window_id").and_then(|v| v.as_str()).unwrap_or("");
                if st.windows.contains_key(id) {
                    st.foreground = Some(id.to_string());
                    BridgeResponse::ok(json!({ "focused": id }))
                } else {
                    BridgeResponse::err(&BridgeError::WindowNotFound.code())
                }
            }
            "windows.move" => {
                let id = p.get("window_id").and_then(|v| v.as_str()).unwrap_or("");
                match st.windows.get_mut(id) {
                    Some(w) => {
                        w.bounds.x = p.get("x").and_then(|v| v.as_i64()).unwrap_or(0) as i32;
                        w.bounds.y = p.get("y").and_then(|v| v.as_i64()).unwrap_or(0) as i32;
                        BridgeResponse::ok(serde_json::to_value(&*w).unwrap())
                    }
                    None => BridgeResponse::err(&BridgeError::WindowNotFound.code()),
                }
            }
            "windows.resize" => {
                let id = p.get("window_id").and_then(|v| v.as_str()).unwrap_or("");
                match st.windows.get_mut(id) {
                    Some(w) => {
                        w.bounds.width =
                            p.get("width").and_then(|v| v.as_i64()).unwrap_or(0) as u32;
                        w.bounds.height =
                            p.get("height").and_then(|v| v.as_i64()).unwrap_or(0) as u32;
                        BridgeResponse::ok(serde_json::to_value(&*w).unwrap())
                    }
                    None => BridgeResponse::err(&BridgeError::WindowNotFound.code()),
                }
            }
            "windows.capture" => {
                let id = p.get("window_id").and_then(|v| v.as_str()).unwrap_or("");
                if st.windows.contains_key(id) {
                    BridgeResponse::ok(json!({
                        "data": "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==",
                        "width": 800,
                        "height": 600,
                    }))
                } else {
                    BridgeResponse::err(&BridgeError::WindowNotFound.code())
                }
            }
            "windows.click" | "windows.type" | "windows.scroll" => {
                let id = p.get("window_id").and_then(|v| v.as_str()).unwrap_or("");
                if st.windows.contains_key(id) {
                    BridgeResponse::ok(json!({ "ok": true }))
                } else {
                    BridgeResponse::err(&BridgeError::WindowNotFound.code())
                }
            }
            "windows.clipboard.read" => BridgeResponse::ok(json!({ "text": st.clipboard })),
            "windows.clipboard.write" => {
                st.clipboard = p
                    .get("text")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                BridgeResponse::ok(json!({ "ok": true }))
            }
            other => {
                BridgeResponse::err(&BridgeError::Unknown(format!("unknown intent {other}")).code())
            }
        }
    }
}

fn adapter_with_host(host: Arc<MockWindowsHost>) -> WindowsAdapter {
    let handler: Arc<dyn Fn(BridgeFrame) -> BridgeResponse + Send + Sync> =
        Arc::new(move |f| host.handle(f));
    WindowsAdapter::new(Arc::new(MockTransport::new(handler)))
}

#[tokio::test]
async fn open_list_focus_type_capture_close_lifecycle() {
    let host = Arc::new(MockWindowsHost::new());
    let adapter = adapter_with_host(host.clone());

    let open = adapter
        .execute(ActionRequest::new(
            "windows",
            "open",
            json!({ "path": "notepad.exe", "title": "Untitled" }),
        ))
        .await;
    assert!(open.success, "open failed: {:?}", open.error);
    let win: Window = serde_json::from_value(open.output).unwrap();
    assert_eq!(win.process, "notepad.exe");
    let id = win.id.clone();

    let list = adapter
        .execute(ActionRequest::new("windows", "list", json!({})))
        .await;
    assert!(list.success);
    let windows: Vec<Window> = serde_json::from_value(list.output).unwrap();
    assert_eq!(windows.len(), 1);

    let focus = adapter
        .execute(ActionRequest::new(
            "windows",
            "focus",
            json!({ "window_id": id }),
        ))
        .await;
    assert!(focus.success);

    let capture = adapter
        .execute(ActionRequest::new(
            "windows",
            "capture",
            json!({ "window_id": id }),
        ))
        .await;
    assert!(capture.success);
    assert!(capture.output.get("data").is_some());

    let close = adapter
        .execute(ActionRequest::new(
            "windows",
            "close",
            json!({ "window_id": id }),
        ))
        .await;
    assert!(close.success);

    let list2 = adapter
        .execute(ActionRequest::new("windows", "list", json!({})))
        .await;
    let windows2: Vec<Window> = serde_json::from_value(list2.output).unwrap();
    assert!(windows2.is_empty(), "window should be gone after close");
}

#[tokio::test]
async fn unknown_window_returns_normalized_error() {
    let host = Arc::new(MockWindowsHost::new());
    let adapter = adapter_with_host(host);
    let res = adapter
        .execute(ActionRequest::new(
            "windows",
            "close",
            json!({ "window_id": "win-999" }),
        ))
        .await;
    assert!(!res.success);
    assert_eq!(res.error.as_deref(), Some("WindowNotFound"));
}

#[tokio::test]
async fn clipboard_round_trip() {
    let host = Arc::new(MockWindowsHost::new());
    let adapter = adapter_with_host(host);
    let write = adapter
        .execute(ActionRequest::new(
            "windows",
            "clipboard.write",
            json!({ "text": "hello friday" }),
        ))
        .await;
    assert!(write.success);
    let read = adapter
        .execute(ActionRequest::new("windows", "clipboard.read", json!({})))
        .await;
    assert!(read.success);
    assert_eq!(
        read.output.get("text").and_then(|v| v.as_str()),
        Some("hello friday")
    );
}

#[test]
fn manifest_declares_all_intents() {
    assert_eq!(CAPABILITY_MANIFEST.bridge, "windows");
    assert_eq!(CAPABILITY_MANIFEST.protocol_version, 1);
    assert_eq!(CAPABILITY_MANIFEST.capabilities.len(), 20);
    assert!(CAPABILITY_MANIFEST.capabilities.contains(&"windows.click"));
}

#[test]
fn frame_serde_round_trip() {
    let frame = BridgeFrame::request("windows.open", json!({ "path": "notepad.exe" }));
    let bytes = serde_json::to_vec(&frame).unwrap();
    let back: BridgeFrame = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(back.protocol_version, 1);
    assert_eq!(back.message.action, "windows.open");
}

#[test]
fn error_code_round_trip() {
    assert_eq!(
        BridgeError::from_code("AccessDenied"),
        BridgeError::AccessDenied
    );
    assert_eq!(BridgeError::AccessDenied.code(), "AccessDenied");
    assert_eq!(
        BridgeError::from_code("SomethingOdd"),
        BridgeError::Unknown("SomethingOdd".into())
    );
}

/// Real-application acceptance harness: launch Notepad via the C# host, verify
/// the window exists, focus, type, capture, close, verify exit. Requires the
/// `windows-host` service running. Skipped in CI (no dotnet, no Windows host).
#[tokio::test]
async fn notepad_real_host_harness() {
    if std::env::var("FRIDAY_WINDOWS_HOST_TEST").is_err() {
        eprintln!(
            "skipping real-host harness (set FRIDAY_WINDOWS_HOST_TEST=1 and run windows-host)"
        );
        return;
    }
    #[cfg(windows)]
    {
        let adapter = friday_windows::WindowsAdapter::for_session("default");

        let open = adapter
            .execute(ActionRequest::new(
                "windows",
                "open",
                json!({ "path": "notepad.exe" }),
            ))
            .await;
        assert!(open.success, "open failed: {:?}", open.error);
        let win: Window = serde_json::from_value(open.output).unwrap();
        let id = win.id.clone();

        let focus = adapter
            .execute(ActionRequest::new(
                "windows",
                "focus",
                json!({ "window_id": id }),
            ))
            .await;
        assert!(focus.success, "focus: {:?}", focus.error);

        let typed = adapter
            .execute(ActionRequest::new(
                "windows",
                "type",
                json!({ "window_id": id, "text": "hello friday" }),
            ))
            .await;
        assert!(typed.success, "type: {:?}", typed.error);

        let capture = adapter
            .execute(ActionRequest::new(
                "windows",
                "capture",
                json!({ "window_id": id }),
            ))
            .await;
        assert!(capture.success, "capture: {:?}", capture.error);

        let close = adapter
            .execute(ActionRequest::new(
                "windows",
                "close",
                json!({ "window_id": id }),
            ))
            .await;
        assert!(close.success, "close: {:?}", close.error);
    }
}
