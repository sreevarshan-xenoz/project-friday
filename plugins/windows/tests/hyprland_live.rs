//! Live qualification of the Hyprland host against a real Hyprland session.
//! Ignored by default — it needs a running compositor:
//!
//! ```text
//! cargo test -p friday-windows --test hyprland_live -- --ignored --test-threads=1
//! ```
//!
//! Window-moving tests use a probe window launched onto a hidden special
//! workspace, so the session's visible layout and focus are untouched.
//! `open_launches_a_real_app` does take focus briefly; it closes its window
//! and refocuses the previous one.

use std::process::Command;
use std::sync::Arc;
use std::time::{Duration, Instant};

use friday_actions::adapters::ActionAdapter;
use friday_actions::request::{ActionRequest, ActionResult};
use friday_windows::hyprland::{binary_on_path, HyprlandTransport};
use friday_windows::WindowsAdapter;
use serde_json::{json, Value};

const PROBE_CLASS: &str = "friday-hyprland-probe";

fn adapter() -> WindowsAdapter {
    WindowsAdapter::new(Arc::new(HyprlandTransport::new()))
}

async fn exec(adapter: &WindowsAdapter, verb: &str, params: Value) -> ActionResult {
    adapter
        .execute(ActionRequest::new("windows", verb, params))
        .await
}

/// A terminal parked on a hidden special workspace; closed on drop.
struct Probe(String);

impl Probe {
    fn spawn() -> Self {
        let terminal = ["kitty", "foot", "alacritty"]
            .into_iter()
            .find(|t| binary_on_path(t))
            .expect("a terminal (kitty/foot/alacritty) to host the probe window");
        let class_flag = if terminal == "alacritty" {
            "--class"
        } else {
            "--app-id"
        };
        let class_flag = if terminal == "kitty" {
            "--class"
        } else {
            class_flag
        };
        let lua = format!(
            "hl.exec_cmd(\"{terminal} {class_flag} {PROBE_CLASS} -e sleep 60\", \
             {{ workspace = \"special:fridayprobe silent\", float = true }})"
        );
        let out = Command::new("hyprctl")
            .args(["eval", &lua])
            .output()
            .unwrap();
        assert!(out.status.success());
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let out = Command::new("hyprctl")
                .args(["-j", "clients"])
                .output()
                .unwrap();
            let clients: Value = serde_json::from_slice(&out.stdout).unwrap();
            if let Some(c) = clients
                .as_array()
                .unwrap()
                .iter()
                .find(|c| c["class"] == PROBE_CLASS)
            {
                return Probe(c["address"].as_str().unwrap().to_string());
            }
            assert!(Instant::now() < deadline, "probe window never appeared");
            std::thread::sleep(Duration::from_millis(100));
        }
    }
}

impl Drop for Probe {
    fn drop(&mut self) {
        let lua = format!("hl.dsp.window.close({{ window = \"address:{}\" }})", self.0);
        let _ = Command::new("hyprctl").args(["dispatch", &lua]).output();
    }
}

#[tokio::test]
#[ignore = "needs a live Hyprland session"]
async fn read_only_verbs_describe_the_real_desktop() {
    let a = adapter();
    let list = exec(&a, "list", json!({})).await;
    assert!(list.success, "{:?}", list.error);
    let windows = list.output.as_array().unwrap();
    assert!(!windows.is_empty());
    assert!(windows
        .iter()
        .all(|w| w["id"].as_str().unwrap().starts_with("0x")));

    let fg = exec(&a, "foreground", json!({})).await;
    assert!(fg.success, "{:?}", fg.error);
    let id = fg.output["id"].as_str().unwrap().to_string();

    let shot = exec(&a, "capture", json!({ "window_id": id })).await;
    assert!(shot.success, "{:?}", shot.error);
    assert!(
        shot.output["data"]
            .as_str()
            .unwrap()
            .starts_with("iVBORw0KGgo"),
        "PNG"
    );

    let clip = exec(&a, "clipboard.read", json!({})).await;
    assert!(clip.success, "{:?}", clip.error);
}

#[tokio::test]
#[ignore = "needs a live Hyprland session"]
async fn move_resize_and_close_a_real_window() {
    let probe = Probe::spawn();
    let a = adapter();

    let moved = exec(
        &a,
        "move",
        json!({ "window_id": probe.0, "x": 140, "y": 160 }),
    )
    .await;
    assert!(moved.success, "{:?}", moved.error);
    assert_eq!(moved.output["bounds"]["x"], 140);
    assert_eq!(moved.output["bounds"]["y"], 160);

    let resized = exec(
        &a,
        "resize",
        json!({ "window_id": probe.0, "width": 640, "height": 400 }),
    )
    .await;
    assert!(resized.success, "{:?}", resized.error);
    assert_eq!(resized.output["bounds"]["width"], 640);
    assert_eq!(resized.output["bounds"]["height"], 400);

    let closed = exec(&a, "close", json!({ "window_id": probe.0 })).await;
    assert!(closed.success, "{:?}", closed.error);
    tokio::time::sleep(Duration::from_millis(500)).await;
    let gone = exec(&a, "focus", json!({ "window_id": probe.0 })).await;
    assert_eq!(gone.error.as_deref(), Some("WindowNotFound"));
}

#[tokio::test]
#[ignore = "needs a live Hyprland session; briefly takes focus"]
async fn open_launches_a_real_app() {
    let a = adapter();
    let previous = exec(&a, "foreground", json!({})).await;

    let opened = exec(&a, "open", json!({ "path": "terminal" })).await;
    assert!(opened.success, "{:?}", opened.error);
    assert_eq!(opened.metadata["launch_resolution"], "New Instance");
    let id = opened.output["id"].as_str().unwrap().to_string();

    let closed = exec(&a, "close", json!({ "window_id": id })).await;
    assert!(closed.success, "{:?}", closed.error);
    if let Some(prev) = previous.output["id"].as_str() {
        let _ = exec(&a, "focus", json!({ "window_id": prev })).await;
    }
}
