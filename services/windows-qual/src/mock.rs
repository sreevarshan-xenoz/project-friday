//! In-process mock of the C# `windows-host`, used by `--mock` so the harness can
//! be exercised end-to-end without .NET or a running service. Mirrors the real
//! handler set plus a synthetic `__crash` intent used to simulate a process
//! crash for the injection scenario.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use friday_windows::ipc::MockTransport;
use friday_windows::model::{Bounds, Window, WindowState};
use friday_windows::protocol::{BridgeFrame, BridgeResponse};
use friday_windows::WindowsAdapter;
use serde_json::json;

#[derive(Default)]
struct State {
    windows: HashMap<String, Window>,
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

    fn open_window(&self, st: &mut State, path: &str, title: &str) -> Window {
        st.counter += 1;
        let id = format!("win-{}", st.counter);
        let process = path.rsplit(['\\', '/']).next().unwrap_or(path).to_string();
        let win = Window {
            id: id.clone(),
            title: if title.is_empty() {
                process.clone()
            } else {
                title.into()
            },
            process,
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
        win
    }

    fn handle(&self, frame: BridgeFrame) -> BridgeResponse {
        let intent = frame.message.action.as_str();
        let p = &frame.message.payload;
        let mut st = self.state.lock().unwrap();
        match intent {
            "windows.open" => {
                let path = p.get("path").and_then(|v| v.as_str()).unwrap_or("app.exe");
                let title = p.get("title").and_then(|v| v.as_str()).unwrap_or("");
                let win = self.open_window(&mut st, path, title);
                BridgeResponse::ok(serde_json::to_value(&win).unwrap())
            }
            "windows.__crash" => {
                if let Some(id) = p.get("window_id").and_then(|v| v.as_str()) {
                    st.windows.remove(id);
                    if st.foreground.as_deref() == Some(id) {
                        st.foreground = None;
                    }
                }
                BridgeResponse::ok(json!({ "crashed": true }))
            }
            "windows.list" => {
                let list: Vec<Window> = st.windows.values().cloned().collect();
                BridgeResponse::ok(json!(list))
            }
            "windows.foreground" => match &st.foreground {
                Some(id) => match st.windows.get(id) {
                    Some(w) => BridgeResponse::ok(serde_json::to_value(w).unwrap()),
                    None => BridgeResponse::err("WindowNotFound"),
                },
                None => BridgeResponse::err("WindowNotFound"),
            },
            "windows.close" => {
                let id = p.get("window_id").and_then(|v| v.as_str()).unwrap_or("");
                if st.windows.remove(id).is_some() {
                    if st.foreground.as_deref() == Some(id) {
                        st.foreground = None;
                    }
                    BridgeResponse::ok(json!({ "closed": id }))
                } else {
                    BridgeResponse::err("WindowNotFound")
                }
            }
            "windows.focus" => {
                let id = p.get("window_id").and_then(|v| v.as_str()).unwrap_or("");
                if st.windows.contains_key(id) {
                    st.foreground = Some(id.to_string());
                    BridgeResponse::ok(json!({ "focused": id }))
                } else {
                    BridgeResponse::err("WindowNotFound")
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
                    None => BridgeResponse::err("WindowNotFound"),
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
                    None => BridgeResponse::err("WindowNotFound"),
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
                    BridgeResponse::err("WindowNotFound")
                }
            }
            "windows.click" | "windows.type" | "windows.scroll" => {
                let id = p.get("window_id").and_then(|v| v.as_str()).unwrap_or("");
                if st.windows.contains_key(id) {
                    BridgeResponse::ok(json!({ "ok": true }))
                } else {
                    BridgeResponse::err("WindowNotFound")
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
            other => BridgeResponse::err(&format!("unknown intent {other}")),
        }
    }
}

/// Build an adapter backed by the mock host. For the injection scenario the
/// harness calls `windows.__crash` to simulate a process crash.
pub fn mock_adapter() -> WindowsAdapter {
    let host = Arc::new(MockWindowsHost::new());
    let handler: Arc<dyn Fn(BridgeFrame) -> BridgeResponse + Send + Sync> =
        Arc::new(move |f| host.handle(f));
    WindowsAdapter::new(Arc::new(MockTransport::new(handler)))
}
