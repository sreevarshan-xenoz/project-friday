//! The desktop adapter against a live Hyprland: launch a window, find it, move
//! it to another workspace, switch there, close it. Ignored by default (it
//! opens and closes a terminal on the session it finds); run it on a session
//! made for testing, such as the FRIDAY OS test VM:
//!
//! ```text
//! FRIDAY_TEST_REAL_HYPRLAND=1 cargo test -p friday-actions --test real_hyprland -- --ignored
//! ```

use friday_actions::{ActionAdapter, ActionRequest, DesktopAdapter};
use serde_json::{json, Value};

async fn act(a: &DesktopAdapter, verb: &str, params: Value) -> Value {
    let r = a.execute(ActionRequest::new("desktop", verb, params)).await;
    assert!(r.success, "{verb} failed: {:?}", r.error);
    r.output
}

async fn windows_of(a: &DesktopAdapter, app: &str) -> Vec<Value> {
    act(a, "windows", json!({}))
        .await
        .as_array()
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .filter(|w| w["app"] == app)
        .collect()
}

async fn wait_for(a: &DesktopAdapter, app: &str, present: bool) -> Vec<Value> {
    for _ in 0..50 {
        let w = windows_of(a, app).await;
        if w.is_empty() != present {
            return w;
        }
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    }
    panic!(
        "a {app} window never {}",
        if present { "appeared" } else { "went away" }
    );
}

#[tokio::test]
#[ignore = "drives a live Hyprland session"]
async fn launch_move_switch_and_close_a_window_on_a_real_compositor() {
    if std::env::var("FRIDAY_TEST_REAL_HYPRLAND").as_deref() != Ok("1") {
        eprintln!("set FRIDAY_TEST_REAL_HYPRLAND=1 to run this against the live session");
        return;
    }
    assert!(
        friday_actions::hypr_signature().is_some(),
        "no Hyprland session found"
    );
    let a = DesktopAdapter::new();
    assert!(
        windows_of(&a, "foot").await.is_empty(),
        "close the foot windows first: this test closes the one it opens"
    );

    act(&a, "launch_app", json!({"app": "foot"})).await;
    let opened = wait_for(&a, "foot", true).await;
    assert_eq!(opened.len(), 1);

    act(
        &a,
        "move_window",
        json!({"target": "foot", "workspace": "7"}),
    )
    .await;
    let moved = windows_of(&a, "foot").await;
    assert_eq!(
        moved[0]["workspace"], "7",
        "the window did not move: {moved:?}"
    );

    act(&a, "switch_workspace", json!({"workspace": "7"})).await;
    let spaces = act(&a, "workspaces", json!({})).await;
    assert!(spaces.as_array().unwrap().iter().any(|w| w["name"] == "7"));

    // The window is named by what it is, and the adapter closes only that one.
    act(&a, "focus_window", json!({"target": "foot"})).await;
    act(&a, "close_window", json!({"target": "foot"})).await;
    wait_for(&a, "foot", false).await;
}
